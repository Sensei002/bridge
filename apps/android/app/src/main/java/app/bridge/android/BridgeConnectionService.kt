package app.bridge.android

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import java.io.File
import java.util.UUID

internal object BridgeRuntimeState {
    val discoveredPeers = mutableStateListOf<BridgePeer>()
    val pendingPairings = mutableStateListOf<BridgePendingPairing>()
    val trustedDevices = mutableStateListOf<BridgeTrustedDevice>()
    val connectedSessionHandles = mutableStateMapOf<String, Long>()
    val incomingOffers = mutableStateListOf<BridgeIncomingOffer>()
    val receivedFiles = mutableStateListOf<File>()
    val clipboardReceivedText = mutableStateOf<String?>(null)
    val clipboardReceivedFrom = mutableStateOf("No text received yet.")
    val clipboardStatus = mutableStateOf("Clipboard sharing is off.")
    val pairingStatus = mutableStateOf("Starting Bridge background connection…")
    val transferStatus = mutableStateOf("Waiting for a connected trusted device.")
    val latestDisplayFrame = mutableStateOf<BridgeDisplayFrame?>(null)
    val latestEncodedVideoFrame = mutableStateOf<BridgeEncodedVideoFrame?>(null)
}

/**
 * Owns the long-lived Bridge network runtime independently of the UI task.
 * The foreground notification is intentionally persistent while Bridge is active.
 */
class BridgeConnectionService : Service() {
    companion object {
        private const val CHANNEL_ID = "bridge_connection"
        private const val NOTIFICATION_ID = 4582

        @Volatile
        var instance: BridgeConnectionService? = null
            private set
    }

    private val mainHandler = Handler(Looper.getMainLooper())
    @Volatile private var stopping = false
    @Volatile private var discovery: BridgeDiscovery? = null
    @Volatile private var pairingController: BridgePairingController? = null
    @Volatile private var transferReceiver: BridgeTransferReceiver? = null
    private var wifiCallbackRegistered = false

    private val wifiNetworkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            mainHandler.post {
                if (stopping) return@post
                BridgeRuntimeState.pairingStatus.value = "Wi-Fi connected. Looking for trusted Bridge devices…"
                discovery?.restart()
            }
        }

        override fun onLost(network: Network) {
            mainHandler.post {
                if (stopping) return@post
                pairingController?.onNetworkLost()
                // Close active transfers, but keep the receiver reusable: new
                // authenticated session handles are attached after Wi-Fi returns.
                transferReceiver?.cancelAll()
                BridgeRuntimeState.connectedSessionHandles.clear()
                BridgeRuntimeState.pairingStatus.value = "Wi-Fi disconnected. Waiting to reconnect…"
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        instance = this
        createNotificationChannel()
        val notification = buildNotification()
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
        } else {
            0
        }
        ServiceCompat.startForeground(this, NOTIFICATION_ID, notification, type)
        startRuntime()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int = START_STICKY

    override fun onBind(intent: Intent?): IBinder? = null

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "Bridge device connection",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Keeps trusted Bridge devices connected on your local network."
                setShowBadge(false)
            }
            getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
        }
    }

    private fun buildNotification(): Notification {
        val openApp = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_data_bluetooth)
            .setContentTitle("Bridge is running")
            .setContentText("Keeping trusted devices connected on this network")
            .setContentIntent(openApp)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .build()
    }

    private fun postStatus(message: String) {
        BridgeDiagnostics.record("connection", message)
        mainHandler.post {
            if (!stopping) BridgeRuntimeState.pairingStatus.value = message
        }
    }

    private fun startRuntime() {
        Thread({
            try {
                val privateKey = BridgeIdentityStore.loadOrCreate(this)
                val keyHex = privateKey.joinToString("") { "%02x".format(it.toInt() and 0xff) }
                val fingerprintResult = org.json.JSONObject(BridgeNative.identityFingerprint(keyHex))
                check(fingerprintResult.optBoolean("ok")) { fingerprintResult.optString("error") }
                val fingerprint = fingerprintResult.getString("fingerprint")
                // Create and validate the independent signing identity before pairing starts.
                // The seed is reloaded only by the future signing path, never retained here.
                val attestationSeed = BridgeIdentityStore.loadOrCreateAttestationSeed(this)
                val attestationKeyResult = try {
                    org.json.JSONObject(BridgeNative.attestationPublicKey(attestationSeed))
                } finally {
                    attestationSeed.fill(0)
                }
                check(attestationKeyResult.optBoolean("ok")) {
                    attestationKeyResult.optString("error", "Could not derive Android attestation public key")
                }
                val attestationPublicKey = attestationKeyResult.getString("public_key")
                check(attestationPublicKey.matches(Regex("[0-9a-fA-F]{64}"))) {
                    "Native attestation key derivation returned an invalid public key"
                }
                val identityPreferences = getSharedPreferences("bridge_secure_identity", MODE_PRIVATE)
                val deviceId = identityPreferences.getString("device_id", null)
                    ?: UUID.randomUUID().toString().also {
                        check(identityPreferences.edit().putString("device_id", it).commit())
                    }

                val receiver = BridgeTransferReceiver(
                    this,
                    fingerprint,
                    onOffer = { offer ->
                        mainHandler.post {
                            if (!stopping && BridgeRuntimeState.incomingOffers.none { it.transferId == offer.transferId }) {
                                BridgeRuntimeState.incomingOffers.add(offer)
                            }
                        }
                    },
                    onDisplayFrame = { frame ->
                        if (frame.sequence == 1L || frame.sequence % 120L == 0L) {
                            BridgeDiagnostics.record("video-bgra", "Frame ${frame.sequence} · ${frame.width}x${frame.height}")
                        }
                        mainHandler.post {
                            if (!stopping) {
                                BridgeRuntimeState.latestDisplayFrame.value = frame
                                BridgeRuntimeState.transferStatus.value =
                                    "Second screen frame received · ${frame.width}×${frame.height} · #${frame.sequence}"
                            }
                        }
                    },
                    onEncodedVideoFrame = { frame ->
                        if (frame.keyframe || frame.sequence == 1L || frame.sequence % 120L == 0L) {
                            BridgeDiagnostics.record("video-h264", "Access unit ${frame.sequence} · ${frame.width}x${frame.height} · keyframe=${frame.keyframe}")
                        }
                        mainHandler.post {
                            if (!stopping) {
                                BridgeRuntimeState.latestEncodedVideoFrame.value = frame
                                BridgeRuntimeState.transferStatus.value =
                                    "H.264 access unit received · ${frame.width}×${frame.height} · #${frame.sequence}${if (frame.keyframe) " · keyframe" else ""}"
                            }
                        }
                    },
                    onClipboardText = { peerId, text ->
                        val enabled = getSharedPreferences("bridge_settings", MODE_PRIVATE)
                            .getBoolean("clipboard_sync_enabled", false)
                        if (enabled && !stopping) {
                            mainHandler.post {
                                if (!stopping) {
                                    BridgeRuntimeState.clipboardReceivedText.value = text
                                    val peerName = controllerTrustedDevices().firstOrNull { it.deviceId == peerId }?.name
                                        ?: "Trusted device " + peerId.take(8)
                                    BridgeRuntimeState.clipboardReceivedFrom.value = "Received from $peerName"
                                    BridgeRuntimeState.clipboardStatus.value = "Text received securely."
                                }
                            }
                        }
                    },
                    onStatus = { message ->
                        BridgeDiagnostics.record("transfer", message)
                        mainHandler.post {
                            if (!stopping) {
                                BridgeRuntimeState.transferStatus.value = message
                                if (message.startsWith("Received ")) refreshReceivedFiles()
                            }
                        }
                    },
                    onSessionEnded = { peerId, handle ->
                        BridgeDiagnostics.record("connection", "Session ended for peer ${peerId.take(12)}")
                        mainHandler.post {
                            if (stopping) return@post
                            pairingController?.onSessionEnded(peerId, handle)
                            if (BridgeRuntimeState.latestDisplayFrame.value?.deviceId == peerId) {
                                BridgeRuntimeState.latestDisplayFrame.value = null
                            }
                            if (BridgeRuntimeState.latestEncodedVideoFrame.value?.deviceId == peerId) {
                                BridgeRuntimeState.latestEncodedVideoFrame.value = null
                            }
                            if (BridgeRuntimeState.connectedSessionHandles[peerId] == handle) {
                                BridgeRuntimeState.connectedSessionHandles.remove(peerId)
                            }
                            mainHandler.postDelayed({
                                if (!stopping && !BridgeRuntimeState.connectedSessionHandles.containsKey(peerId)) {
                                    val peer = BridgeRuntimeState.discoveredPeers.firstOrNull { it.deviceId == peerId }
                                    val trusted = pairingController?.trustedDevices()
                                        ?.firstOrNull { it.deviceId == peerId }
                                    if (peer != null && trusted != null &&
                                        trusted.fingerprint.equals(peer.fingerprint, ignoreCase = true)) {
                                        pairingController?.pair(peer, trusted.fingerprint)
                                    }
                                }
                            }, 2000)
                        }
                    }
                )
                transferReceiver = receiver

                val controller = BridgePairingController(
                    this,
                    privateKey,
                    fingerprint,
                    deviceId,
                    attestationPublicKey,
                    onPending = { pending ->
                        mainHandler.post {
                            if (!stopping && BridgeRuntimeState.pendingPairings.none { it.handle == pending.handle }) {
                                BridgeRuntimeState.pendingPairings.add(pending)
                            }
                        }
                    },
                    onStatus = { message ->
                        mainHandler.post {
                            if (!stopping) {
                                BridgeRuntimeState.pairingStatus.value = message
                                if (message.endsWith("Device connected.")) {
                                    BridgeRuntimeState.trustedDevices.clear()
                                    BridgeRuntimeState.trustedDevices.addAll(controllerTrustedDevices())
                                }
                            }
                        }
                    }
                )
                val restoredDevices = controller.trustedDevices()
                if (stopping) {
                    controller.stop()
                    receiver.stop()
                    return@Thread
                }
                pairingController = controller
                controller.onSessionReady = { peerId, handle ->
                    mainHandler.post {
                        if (!stopping) {
                            BridgeRuntimeState.connectedSessionHandles[peerId] = handle
                            val peerPlatform = controller.trustedDevices()
                                .firstOrNull { it.deviceId == peerId }?.platform
                            receiver.attach(peerId, handle, allowDisplayAttestation = peerPlatform == "windows")
                        } else {
                            BridgeNative.sessionClose(handle)
                        }
                    }
                }
                controller.start()

                val networkDiscovery = BridgeDiscovery(
                    this,
                    { peer ->
                        mainHandler.post {
                            if (stopping || peer.deviceId == deviceId) return@post
                            // Device IDs can be regenerated after app-data reset; the
                            // authenticated Noise fingerprint remains the stable identity.
                            val existing = BridgeRuntimeState.discoveredPeers.indexOfFirst {
                                it.deviceId == peer.deviceId || it.fingerprint.equals(peer.fingerprint, ignoreCase = true)
                            }
                            if (existing >= 0) BridgeRuntimeState.discoveredPeers[existing] = peer
                            else BridgeRuntimeState.discoveredPeers.add(peer)

                            val trustedPeer = try {
                                controller.trustedDevices().firstOrNull { it.deviceId == peer.deviceId }
                            } catch (_: Exception) { null }
                            if (trustedPeer != null &&
                                trustedPeer.fingerprint.equals(peer.fingerprint, ignoreCase = true) &&
                                !BridgeRuntimeState.connectedSessionHandles.containsKey(peer.deviceId)) {
                                controller.pair(peer, trustedPeer.fingerprint)
                            }
                        }
                    },
                    { serviceName ->
                        mainHandler.post {
                            BridgeRuntimeState.discoveredPeers.removeAll { it.serviceName == serviceName }
                        }
                    },
                    { message -> postStatus(message) }
                )
                if (stopping) {
                    controller.stop()
                    receiver.stop()
                    return@Thread
                }
                discovery = networkDiscovery
                networkDiscovery.start(deviceId, Build.MODEL ?: "Bridge Android", fingerprint)
                registerWifiNetworkCallback()

                mainHandler.post {
                    if (!stopping) {
                        BridgeRuntimeState.trustedDevices.clear()
                        BridgeRuntimeState.trustedDevices.addAll(restoredDevices)
                        BridgeRuntimeState.pairingStatus.value =
                            "Noise XX ready · Android attestation identity protected by Keystore."
                        refreshReceivedFiles()
                    }
                }
            } catch (error: Throwable) {
                val detail = error.message?.takeIf { it.isNotBlank() } ?: error.javaClass.simpleName
                postStatus("Startup failed (${error.javaClass.simpleName}): $detail")
            }
        }, "BridgeConnectionStartup").apply { isDaemon = true; start() }
    }

    private fun registerWifiNetworkCallback() {
        if (wifiCallbackRegistered) return
        val manager = getSystemService(ConnectivityManager::class.java) ?: return
        try {
            manager.registerNetworkCallback(
                NetworkRequest.Builder().addTransportType(NetworkCapabilities.TRANSPORT_WIFI).build(),
                wifiNetworkCallback
            )
            wifiCallbackRegistered = true
        } catch (error: Exception) {
            postStatus("Automatic Wi-Fi recovery unavailable: ${error.message}")
        }
    }

    internal fun pair(peer: BridgePeer) {
        pairingController?.pair(peer)
    }

    internal fun approve(pending: BridgePendingPairing) {
        pairingController?.approve(pending)
        BridgeRuntimeState.pendingPairings.remove(pending)
        BridgeRuntimeState.trustedDevices.clear()
        BridgeRuntimeState.trustedDevices.addAll(controllerTrustedDevices())
    }

    internal fun reject(pending: BridgePendingPairing) {
        pairingController?.reject(pending)
        BridgeRuntimeState.pendingPairings.remove(pending)
    }

    internal fun revoke(deviceId: String) {
        pairingController?.revoke(deviceId)
        BridgeRuntimeState.trustedDevices.clear()
        BridgeRuntimeState.trustedDevices.addAll(controllerTrustedDevices())
    }

    internal fun trustedDevices(): List<BridgeTrustedDevice> = controllerTrustedDevices()

    private fun controllerTrustedDevices(): List<BridgeTrustedDevice> =
        try { pairingController?.trustedDevices().orEmpty() } catch (_: Exception) { emptyList() }

    internal fun sendFile(uri: Uri, sessionHandle: Long) {
        transferReceiver?.sendFile(uri, sessionHandle)
    }

    internal fun requestVideoKeyframe(deviceId: String, sessionHandle: Long, lastSequence: Long) {
        if (sessionHandle == 0L) return
        try {
            transferReceiver?.requestVideoKeyframe(sessionHandle, lastSequence)
        } catch (error: Exception) {
            mainHandler.post {
                if (!stopping) BridgeRuntimeState.transferStatus.value =
                    "Could not request an H.264 recovery keyframe from $deviceId: ${error.message ?: error.javaClass.simpleName}"
            }
        }
    }

    internal fun sendDisplayInput(deviceId: String, action: Int, pointerId: Int, x: Int, y: Int) {
        // Route input only to the authenticated peer that supplied the displayed frame.
        val handle = BridgeRuntimeState.connectedSessionHandles[deviceId] ?: return
        try {
            transferReceiver?.sendDisplayInput(handle, action, pointerId, x, y)
        } catch (error: Exception) {
            BridgeRuntimeState.transferStatus.value =
                "Could not forward second-screen input: ${error.message ?: error.javaClass.simpleName}"
        }
    }

    internal fun sendClipboard(text: String) {
        val enabled = getSharedPreferences("bridge_settings", MODE_PRIVATE)
            .getBoolean("clipboard_sync_enabled", false)
        if (!enabled) {
            BridgeRuntimeState.clipboardStatus.value = "Enable clipboard sharing first."
            return
        }
        val bytes = text.toByteArray(Charsets.UTF_8)
        if (bytes.isEmpty() || bytes.size > 16 * 1024) {
            BridgeRuntimeState.clipboardStatus.value = "Text must be between 1 byte and 16 KiB."
            return
        }
        val handles = BridgeRuntimeState.connectedSessionHandles.values.toSet()
        var sent = 0
        handles.forEach { handle ->
            try {
                transferReceiver?.sendClipboard(handle, text)
                sent += 1
            } catch (_: Exception) { }
        }
        BridgeRuntimeState.clipboardStatus.value =
            if (sent > 0) "Sent text to $sent connected device(s)." else "No connected trusted devices are available."
    }

    internal fun cancelTransfers() {
        transferReceiver?.cancelAll()
    }

    internal fun acceptTransfer(offer: BridgeIncomingOffer) {
        transferReceiver?.accept(offer)
        BridgeRuntimeState.incomingOffers.remove(offer)
    }

    internal fun rejectTransfer(offer: BridgeIncomingOffer) {
        transferReceiver?.reject(offer)
        BridgeRuntimeState.incomingOffers.remove(offer)
    }

    internal fun refreshReceivedFiles() {
        val directory = File(filesDir, "Bridge/Received")
        val files = directory.listFiles()?.filter { it.isFile && !it.name.startsWith(".bridge-") }
            ?.sortedByDescending { it.lastModified() }.orEmpty()
        mainHandler.post {
            BridgeRuntimeState.receivedFiles.clear()
            BridgeRuntimeState.receivedFiles.addAll(files)
        }
    }

    private fun unregisterWifiNetworkCallback() {
        if (!wifiCallbackRegistered) return
        try {
            getSystemService(ConnectivityManager::class.java)?.unregisterNetworkCallback(wifiNetworkCallback)
        } catch (_: Exception) { }
        wifiCallbackRegistered = false
    }

    override fun onDestroy() {
        stopping = true
        instance = null
        unregisterWifiNetworkCallback()
        pairingController?.stop()
        transferReceiver?.stop()
        transferReceiver = null
        discovery?.stop()
        discovery = null
        super.onDestroy()
    }
}
