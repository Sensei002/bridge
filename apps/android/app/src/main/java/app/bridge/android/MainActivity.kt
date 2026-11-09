package app.bridge.android

import android.os.Bundle
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Handler
import android.os.Looper
import java.net.ServerSocket
import java.net.Socket
import java.net.InetSocketAddress
import java.io.BufferedReader
import java.io.InputStreamReader
import java.io.PrintWriter
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import android.net.Uri
import java.io.File

import androidx.compose.foundation.Image
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.layout.ContentScale
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.*
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

private val Background = Color(0xFF0A0B0F)
private val Sidebar = Color(0xFF0D0F13)
private val Panel = Color(0xFF14181D)
private val PanelAlt = Color(0xFF1A2025)
private val Border = Color(0xFF2B3238)
private val Accent = Color(0xFF22D3EE)
private val Amber = Color(0xFFFBBF24)
private val Muted = Color(0xFF929AA1)
private val Good = Color(0xFF77D397)
private val Ink = Color(0xFF11150D)

private enum class BridgeTab(val label: String, val symbol: String) {
    Home("Home", "⌂"),
    Devices("Devices", "⌁"),
    Transfers("Files", "⇧"),
    Clipboard("Clip", "▤"),
    Tools("Tools", "⌘"),
    Settings("Settings", "⚙")
}

class MainActivity : ComponentActivity() {
    @Volatile private var destroyed = false
    @Volatile private var discovery: BridgeDiscovery? = null
    @Volatile private var pairingController: BridgePairingController? = null
    private val discoveredPeers = mutableStateListOf<BridgePeer>()
    private val pendingPairings = mutableStateListOf<BridgePendingPairing>()
    private val trustedDevices = mutableStateListOf<BridgeTrustedDevice>()
    private val connectedSessionHandles = mutableStateMapOf<String, Long>()
    private val incomingOffers = mutableStateListOf<BridgeIncomingOffer>()
    private val receivedFiles = mutableStateListOf<File>()
    private var pendingExportFile: File? = null
    private lateinit var exportFileLauncher: ActivityResultLauncher<String>
    private lateinit var sendFileLauncher: ActivityResultLauncher<Array<String>>
    private var pendingSendHandle: Long? = null
    private var transferStatus by mutableStateOf("Waiting for a connected trusted device.")
    private var updateStatus by mutableStateOf("Not checked yet.")
    @Volatile private var transferReceiver: BridgeTransferReceiver? = null
    private var pairingStatus by mutableStateOf("Preparing secure device identity…")
    private var wifiCallbackRegistered = false
    private val wifiNetworkCallback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            runOnUiThread {
                if (destroyed) return@runOnUiThread
                pairingStatus = "Wi-Fi connected. Looking for trusted Bridge devices…"
                discovery?.restart()
            }
        }

        override fun onLost(network: Network) {
            runOnUiThread {
                if (destroyed) return@runOnUiThread
                pairingController?.onNetworkLost()
                transferReceiver?.cancelAll()
                transferReceiver?.stop()
                connectedSessionHandles.clear()
                pairingStatus = "Wi-Fi disconnected. Waiting to reconnect…"
            }
        }
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
            pairingStatus = "Automatic Wi-Fi recovery unavailable: ${error.message}"
        }
    }

    private fun unregisterWifiNetworkCallback() {
        if (!wifiCallbackRegistered) return
        try {
            getSystemService(ConnectivityManager::class.java)?.unregisterNetworkCallback(wifiNetworkCallback)
        } catch (_: Exception) { }
        wifiCallbackRegistered = false
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        sendFileLauncher = registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri: Uri? ->
            val handle = pendingSendHandle
            pendingSendHandle = null
            if (uri != null && handle != null) transferReceiver?.sendFile(uri, handle)
        }
        exportFileLauncher = registerForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri: Uri? ->
            val source = pendingExportFile
            pendingExportFile = null
            if (uri != null && source != null) {
                Thread({
                    try {
                        contentResolver.openOutputStream(uri)?.use { output ->
                            source.inputStream().use { input -> input.copyTo(output) }
                        } ?: error("Could not open the selected destination.")
                        runOnUiThread { transferStatus = "Exported ${source.name} successfully." }
                    } catch (error: Exception) {
                        runOnUiThread { transferStatus = "Could not export ${source.name}: ${error.message}" }
                    }
                }, "BridgeFileExport").apply { isDaemon = true; start() }
            }
        }
        refreshReceivedFiles()

        // Show the native UI immediately; Keystore and JNI initialization may
        // take time on first launch and must not block Android's main thread.
        setContent {
            BridgeApp(
                discoveredPeers = discoveredPeers,
                pendingPairings = pendingPairings,
                trustedDevices = trustedDevices,
                connectedSessionHandles = connectedSessionHandles,
                pairingStatus = pairingStatus,
                incomingOffers = incomingOffers,
                transferStatus = transferStatus,
                receivedFiles = receivedFiles,
                updateStatus = updateStatus,
                onCheckUpdates = { channel ->
                    BridgeUpdater.checkAndInstall(this, channel) { message -> updateStatus = message }
                },
                onExportFile = { file -> pendingExportFile = file; exportFileLauncher.launch(file.name) },
                onPickFile = { handle -> pendingSendHandle = handle; sendFileLauncher.launch(arrayOf("*/*")) },
                onCancelTransfer = { transferReceiver?.cancelAll() },
                onAcceptTransfer = { offer -> transferReceiver?.accept(offer); incomingOffers.remove(offer) },
                onRejectTransfer = { offer -> transferReceiver?.reject(offer); incomingOffers.remove(offer) },
                onPair = { peer -> pairingController?.pair(peer) },
                onApprove = { pending ->
                    pairingController?.approve(pending)
                    pendingPairings.remove(pending)
                    trustedDevices.clear()
                    trustedDevices.addAll(pairingController?.trustedDevices().orEmpty())
                },
                onReject = { pending ->
                    pairingController?.reject(pending)
                    pendingPairings.remove(pending)
                },
                onRevoke = { deviceId ->
                    pairingController?.revoke(deviceId)
                    trustedDevices.clear()
                    trustedDevices.addAll(pairingController?.trustedDevices().orEmpty())
                }
            )
        }

        Thread({
            try {
                val privateKey = BridgeIdentityStore.loadOrCreate(this)
                val keyHex = privateKey.joinToString("") { "%02x".format(it.toInt() and 0xff) }
                val fingerprintResult = org.json.JSONObject(BridgeNative.identityFingerprint(keyHex))
                check(fingerprintResult.optBoolean("ok")) { fingerprintResult.optString("error") }
                val fingerprint = fingerprintResult.getString("fingerprint")
                val identityPreferences = getSharedPreferences("bridge_secure_identity", MODE_PRIVATE)
                val deviceId = identityPreferences.getString("device_id", null) ?: java.util.UUID.randomUUID().toString().also {
                    check(identityPreferences.edit().putString("device_id", it).commit())
                }

                val receiver = BridgeTransferReceiver(
                    this,
                    onOffer = { offer -> incomingOffers.add(offer) },
                    onStatus = { message ->
                        transferStatus = message
                        if (message.startsWith("Received ")) refreshReceivedFiles()
                    },
                    onSessionEnded = { peerId, handle ->
                        runOnUiThread {
                            pairingController?.onSessionEnded(peerId, handle)
                            if (connectedSessionHandles[peerId] == handle) {
                                connectedSessionHandles.remove(peerId)
                            }
                            // Retry the trusted peer after the stale handle is removed.
                            // Fingerprint verification remains enforced by pair().
                            android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
                                if (!destroyed && !connectedSessionHandles.containsKey(peerId)) {
                                    val peer = discoveredPeers.firstOrNull { it.deviceId == peerId }
                                    if (peer != null) {
                                        val trusted = pairingController?.trustedDevices()
                                            ?.firstOrNull { it.deviceId == peerId }
                                        if (trusted != null && trusted.fingerprint.equals(peer.fingerprint, ignoreCase = true)) {
                                            pairingController?.pair(peer, trusted.fingerprint)
                                        }
                                    }
                                }
                            }, 2000)
                        }
                    }
                )
                transferReceiver = receiver
                val controller = BridgePairingController(
                    this, privateKey, fingerprint, deviceId,
                    onPending = { pendingPairings.add(it) },
                    onStatus = { message ->
                        pairingStatus = message
                        if (message.endsWith("Device connected.")) {
                            trustedDevices.clear()
                            trustedDevices.addAll(pairingController?.trustedDevices().orEmpty())
                        }
                    }
                )
                val restoredDevices = controller.trustedDevices()
                if (destroyed) {
                    controller.stop()
                    return@Thread
                }
                pairingController = controller
                controller.onSessionReady = { peerId, handle ->
                    runOnUiThread {
                        if (!destroyed) {
                            connectedSessionHandles[peerId] = handle
                            receiver.attach(peerId, handle)
                        } else {
                            BridgeNative.sessionClose(handle)
                        }
                    }
                }
                controller.start()

                val networkDiscovery = BridgeDiscovery(this, { peer ->
                    if (peer.deviceId != deviceId) {
                        val existing = discoveredPeers.indexOfFirst { it.deviceId == peer.deviceId }
                        if (existing >= 0) discoveredPeers[existing] = peer else discoveredPeers.add(peer)
                        // Discovery is also the automatic reconnect trigger. Only
                        // reconnect devices whose saved fingerprint still matches.
                        val trustedPeer = try {
                            controller.trustedDevices().firstOrNull { it.deviceId == peer.deviceId }
                        } catch (_: Exception) { null }
                        if (trustedPeer != null &&
                            trustedPeer.fingerprint.equals(peer.fingerprint, ignoreCase = true) &&
                            !connectedSessionHandles.containsKey(peer.deviceId)) {
                            controller.pair(peer, trustedPeer.fingerprint)
                        }
                    }
                }, { serviceName ->
                    discoveredPeers.removeAll { it.serviceName == serviceName }
                }, { message -> pairingStatus = message })
                if (destroyed) {
                    controller.stop()
                    return@Thread
                }
                discovery = networkDiscovery
                networkDiscovery.start(deviceId, android.os.Build.MODEL ?: "Bridge Android", fingerprint)
                registerWifiNetworkCallback()

                runOnUiThread {
                    if (!destroyed) {
                        trustedDevices.addAll(restoredDevices)
                        pairingStatus = "Identity protected by Android Keystore · Noise XX ready."
                    }
                }
            } catch (error: Throwable) {
                runOnUiThread {
                    if (!destroyed) {
                        val detail = error.message?.takeIf { it.isNotBlank() } ?: error.javaClass.simpleName
                        pairingStatus = "Startup failed (${error.javaClass.simpleName}): $detail"
                    }
                }
            }
        }, "BridgeStartup").apply { isDaemon = true; start() }
    }

    override fun onResume() {
        super.onResume()
        refreshReceivedFiles()
    }

    private fun refreshReceivedFiles() {
        val directory = File(filesDir, "Bridge/Received")
        val files = directory.listFiles()?.filter { it.isFile && !it.name.startsWith(".bridge-") }
            ?.sortedByDescending { it.lastModified() }.orEmpty()
        runOnUiThread {
            receivedFiles.clear()
            receivedFiles.addAll(files)
        }
    }

    override fun onDestroy() {
        destroyed = true
        unregisterWifiNetworkCallback()
        pairingController?.stop()
        transferReceiver?.stop()
        transferReceiver = null
        discovery?.stop()
        discovery = null
        super.onDestroy()
    }
}

private const val BRIDGE_PORT = 45821


private val mainHandler = Handler(Looper.getMainLooper())

private fun startBridgeListener(onStatus: (String) -> Unit) {
    Thread {
        try {
            ServerSocket(BRIDGE_PORT).use { server ->
                mainHandler.post { onStatus("Listening for Bridge tests on TCP $BRIDGE_PORT") }
                while (!server.isClosed) {
                    val socket = server.accept()
                    Thread {
                        socket.use { peer ->
                            try {
                                peer.soTimeout = 5000
                                val request = BufferedReader(InputStreamReader(peer.getInputStream())).readLine()
                                if (request == "BRIDGE_PING/1") {
                                    PrintWriter(peer.getOutputStream(), true).println("BRIDGE_PONG/1")
                                    mainHandler.post { onStatus("Network test succeeded with ${peer.inetAddress.hostAddress}") }
                                } else {
                                    mainHandler.post { onStatus("Received an unknown test message") }
                                }
                            } catch (error: Exception) {
                                mainHandler.post { onStatus("Connection test error: ${error.message}") }
                            }
                        }
                    }.start()
                }
            }
        } catch (error: Exception) {
            mainHandler.post { onStatus("Network listener unavailable: ${error.message}") }
        }
    }.apply { isDaemon = true; name = "BridgeNetworkListener" }.start()
}

private fun testBridgeConnection(host: String, onStatus: (String) -> Unit) {
    Thread {
        try {
            Socket().use { socket ->
                socket.connect(InetSocketAddress(host, BRIDGE_PORT), 4000)
                socket.soTimeout = 4000
                PrintWriter(socket.getOutputStream(), true).println("BRIDGE_PING/1")
                val response = BufferedReader(InputStreamReader(socket.getInputStream())).readLine()
                if (response == "BRIDGE_PONG/1") {
                    mainHandler.post { onStatus("Connection test succeeded with $host") }
                } else {
                    mainHandler.post { onStatus("Unexpected response from $host") }
                }
            }
        } catch (error: Exception) {
            mainHandler.post { onStatus("Could not connect to $host:$BRIDGE_PORT — ${error.message}") }
        }
    }.apply { isDaemon = true; name = "BridgeNetworkTest" }.start()
}

@Composable
private fun BridgeApp(
    discoveredPeers: List<BridgePeer>,
    pendingPairings: List<BridgePendingPairing>,
    trustedDevices: List<BridgeTrustedDevice>,
    connectedSessionHandles: Map<String, Long>,
    pairingStatus: String,
    incomingOffers: List<BridgeIncomingOffer>,
    transferStatus: String,
    receivedFiles: List<File>,
    onExportFile: (File) -> Unit,
    onPickFile: (Long) -> Unit,
    onCancelTransfer: () -> Unit,
    onAcceptTransfer: (BridgeIncomingOffer) -> Unit,
    onRejectTransfer: (BridgeIncomingOffer) -> Unit,
    onPair: (BridgePeer) -> Unit,
    onApprove: (BridgePendingPairing) -> Unit,
    onReject: (BridgePendingPairing) -> Unit,
    onRevoke: (String) -> Unit,
    updateStatus: String,
    onCheckUpdates: (String) -> Unit
) {
    val context = LocalContext.current
    val updatePreferences = remember { context.getSharedPreferences("bridge_settings", android.content.Context.MODE_PRIVATE) }
    var updateChannel by remember {
        mutableStateOf(updatePreferences.getString("update_channel", "development") ?: "development")
    }
    var selected by remember { mutableStateOf(BridgeTab.Home) }
    var showPairDialog by remember { mutableStateOf(false) }
    var clipboardSync by remember { mutableStateOf(true) }
    var notifications by remember { mutableStateOf(true) }
    var autoAccept by remember { mutableStateOf(false) }
    var networkStatus by remember { mutableStateOf("Secure local pairing is ready.") }

    MaterialTheme(colorScheme = darkColorScheme(
        primary = Accent,
        onPrimary = Ink,
        background = Background,
        surface = Panel,
        onSurface = Color.White
    )) {
        Surface(modifier = Modifier.fillMaxSize(), color = Background) {
            Column {
                HeaderBar()
                Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
                    when (selected) {
                        BridgeTab.Home -> HomeScreen(
                            onPair = { showPairDialog = true },
                            onNavigate = { selected = it }
                        )
                        BridgeTab.Devices -> DevicesScreen(
                            peers = discoveredPeers,
                            trusted = trustedDevices,
                            pairingStatus = pairingStatus,
                            onPair = onPair,
                            onApprove = onApprove,
                            onReject = onReject,
                            onRevoke = onRevoke
                        )
                        BridgeTab.Transfers -> TransfersScreen(
                            incomingOffers = incomingOffers,
                            trustedDevices = trustedDevices,
                            connectedSessionHandles = connectedSessionHandles,
                            onPickFile = onPickFile,
                            onCancelTransfer = onCancelTransfer,
                            transferStatus = transferStatus,
                            receivedFiles = receivedFiles,
                            onExportFile = onExportFile,
                            onAccept = onAcceptTransfer,
                            onReject = onRejectTransfer,
                            onPair = { showPairDialog = true }
                        )
                        BridgeTab.Clipboard -> ClipboardScreen(
                            enabled = clipboardSync,
                            onEnabledChange = { clipboardSync = it }
                        )
                        BridgeTab.Tools -> ToolsScreen()
                        BridgeTab.Settings -> SettingsScreen(
                            notifications = notifications,
                            onNotificationsChange = { notifications = it },
                            autoAccept = autoAccept,
                            onAutoAcceptChange = { autoAccept = it },
                            updateChannel = updateChannel,
                            onUpdateChannelChange = {
                                updateChannel = it
                                updatePreferences.edit().putString("update_channel", it).apply()
                            },
                            updateStatus = updateStatus,
                            onCheckUpdates = { onCheckUpdates(updateChannel) }
                        )
                    }
                }
                BottomNavigation(selected = selected, onSelect = { selected = it })
            }
        }
    }

    if (pendingPairings.isNotEmpty()) {
        val request = pendingPairings.first()
        AlertDialog(
            onDismissRequest = {},
            containerColor = PanelAlt,
            title = { Text("Pairing request", color = Color.White, fontWeight = FontWeight.Bold) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("${request.name} wants to connect to this device.", color = Color.White, fontSize = 14.sp)
                    Text("${request.platform} · authenticated encrypted connection", color = Muted, fontSize = 11.sp)
                    Text("Only accept if you recognize this device.", color = Amber, fontSize = 11.sp)
                }
            },
            confirmButton = {
                Button(onClick = { onApprove(request) }, colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = Ink)) {
                    Text("Accept", fontWeight = FontWeight.Bold)
                }
            },
            dismissButton = {
                Button(onClick = { onReject(request) }, colors = ButtonDefaults.buttonColors(containerColor = Panel, contentColor = Amber)) {
                    Text("Reject")
                }
            }
        )
    }

    val incomingOffer = incomingOffers.firstOrNull()
    if (incomingOffer != null) {
        val senderName = trustedDevices.firstOrNull { it.deviceId == incomingOffer.deviceId }?.name
            ?: "Trusted device " + incomingOffer.deviceId.take(8)
        AlertDialog(
            onDismissRequest = {},
            containerColor = PanelAlt,
            title = { Text("Incoming file", color = Color.White, fontWeight = FontWeight.Bold) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("$senderName wants to send you a file.", color = Color.White, fontSize = 13.sp)
                    Text(incomingOffer.fileName, color = Accent, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                    Text(
                        if (incomingOffer.fileSize == 0L) "Empty file"
                        else "%.2f MB".format(incomingOffer.fileSize / 1048576.0),
                        color = Muted,
                        fontSize = 12.sp
                    )
                    Text("The file is encrypted in transit and its SHA-256 checksum is verified before saving.", color = Muted, fontSize = 11.sp, lineHeight = 16.sp)
                    Text("Accept only if you trust the sender and expected this file.", color = Amber, fontSize = 11.sp)
                }
            },
            confirmButton = {
                Button(
                    onClick = { onAcceptTransfer(incomingOffer) },
                    colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = Ink)
                ) { Text("Accept file", fontWeight = FontWeight.Bold) }
            },
            dismissButton = {
                Button(
                    onClick = { onRejectTransfer(incomingOffer) },
                    colors = ButtonDefaults.buttonColors(containerColor = Panel, contentColor = Amber)
                ) { Text("Decline") }
            }
        )
    }
    if (showPairDialog) {
        AlertDialog(
            onDismissRequest = { showPairDialog = false },
            containerColor = PanelAlt,
            titleContentColor = Color.White,
            textContentColor = Muted,
            title = { Text("Pair a device", fontWeight = FontWeight.Bold) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("Choose a nearby Bridge device. It will receive a secure pairing request and can accept or reject it on that device.", color = Color.White, fontSize = 13.sp, lineHeight = 19.sp)
                    Surface(color = Panel, shape = RoundedCornerShape(14.dp), modifier = Modifier.fillMaxWidth()) {
                        Column(modifier = Modifier.padding(18.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                            Text("▦", color = Accent, fontSize = 34.sp)
                            Text("Authenticated pairing", color = Color.White, fontWeight = FontWeight.SemiBold)
                            Text(pairingStatus, color = Amber, fontSize = 11.sp)
                        }
                    }
                    PrimaryAction("Open Devices", { showPairDialog = false; selected = BridgeTab.Devices }, Modifier.fillMaxWidth())
                }
            },
            confirmButton = {
                Button(onClick = { showPairDialog = false }, colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = Ink)) {
                    Text("Got it", fontWeight = FontWeight.Bold)
                }
            }
        )
    }
}

@Composable
private fun HeaderBar() {
    Row(
        modifier = Modifier.fillMaxWidth().background(Sidebar).padding(horizontal = 20.dp, vertical = 15.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.SpaceBetween
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Image(
                painter = painterResource(id = R.drawable.bridge_app_icon),
                contentDescription = "Bridge logo",
                contentScale = ContentScale.Fit,
                modifier = Modifier.size(40.dp)
            )
            Column(verticalArrangement = Arrangement.spacedBy(1.dp)) {
                Text("BRIDGE", color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.Black, letterSpacing = 2.sp)
                Text("EVERY DEVICE. ONE SPACE.", color = Muted, fontSize = 8.sp, letterSpacing = 1.sp)
            }
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            Surface(color = Good.copy(alpha = 0.12f), shape = CircleShape, modifier = Modifier.size(7.dp)) {}
            Text("LOCAL MODE", color = Good, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 0.8.sp)
        }
    }
}

@Composable
private fun BottomNavigation(selected: BridgeTab, onSelect: (BridgeTab) -> Unit) {
    Row(
        modifier = Modifier.fillMaxWidth().background(Sidebar).padding(horizontal = 4.dp, vertical = 7.dp),
        horizontalArrangement = Arrangement.SpaceEvenly,
        verticalAlignment = Alignment.CenterVertically
    ) {
        BridgeTab.values().forEach { tab ->
            val active = selected == tab
            Column(
                modifier = Modifier.weight(1f).clickable { onSelect(tab) }.padding(vertical = 5.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(3.dp)
            ) {
                Text(tab.symbol, color = if (active) Accent else Muted, fontSize = 19.sp, fontWeight = FontWeight.Bold)
                Text(tab.label, color = if (active) Accent else Muted, fontSize = 9.sp, fontWeight = if (active) FontWeight.Bold else FontWeight.Medium, maxLines = 1)
                if (active) Surface(color = Accent, shape = CircleShape, modifier = Modifier.width(14.dp).height(2.dp)) {}
                else Spacer(Modifier.height(2.dp))
            }
        }
    }
}

@Composable
private fun ScreenContent(content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 18.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
        content = content
    )
}

@Composable
private fun Eyebrow(text: String) {
    Text(text.uppercase(), color = Accent, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 1.7.sp)
}

@Composable
private fun ScreenHeading(kicker: String, title: String, subtitle: String) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Eyebrow(kicker)
        Text(title, color = Color.White, fontSize = 27.sp, lineHeight = 32.sp, fontWeight = FontWeight.Bold, letterSpacing = (-0.6).sp)
        Text(subtitle, color = Muted, fontSize = 13.sp, lineHeight = 19.sp)
    }
}

@Composable
private fun Panel(content: @Composable ColumnScope.() -> Unit) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = Panel,
        shape = RoundedCornerShape(16.dp)
    ) {
        Column(modifier = Modifier.padding(17.dp), verticalArrangement = Arrangement.spacedBy(10.dp), content = content)
    }
}

@Composable
private fun PrimaryAction(label: String, onClick: () -> Unit, modifier: Modifier = Modifier) {
    Button(
        onClick = onClick,
        modifier = modifier.height(46.dp),
        shape = RoundedCornerShape(10.dp),
        colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = Ink)
    ) {
        Text(label, fontWeight = FontWeight.Bold, fontSize = 13.sp)
    }
}

@Composable
private fun StatusTag(text: String, color: Color = Muted) {
    Surface(color = color.copy(alpha = 0.10f), shape = RoundedCornerShape(30.dp)) {
        Text(text, color = color, fontSize = 9.sp, fontWeight = FontWeight.Bold, letterSpacing = 0.6.sp, modifier = Modifier.padding(horizontal = 9.dp, vertical = 5.dp))
    }
}

@Composable
private fun FeatureLine(symbol: String, title: String, detail: String, status: String = "PLANNED") {
    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Surface(color = PanelAlt, shape = RoundedCornerShape(10.dp), modifier = Modifier.size(38.dp)) {
            Box(contentAlignment = Alignment.Center) {
                Text(symbol, color = Accent, fontSize = 18.sp, fontWeight = FontWeight.SemiBold)
            }
        }
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(title, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            Text(detail, color = Muted, fontSize = 11.sp, lineHeight = 16.sp)
        }
        StatusTag(status)
    }
}

@Composable
private fun HomeScreen(onPair: () -> Unit, onNavigate: (BridgeTab) -> Unit) {
    ScreenContent {
        ScreenHeading("YOUR ECOSYSTEM", "Everything works\nbetter together.", "A private space for the devices you use every day.")
        Panel {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(7.dp)) {
                    Eyebrow("BRIDGE NETWORK")
                    Text("Ready when you are.", color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.Bold)
                    Text("No devices connected yet. Pair your first device to get started.", color = Muted, fontSize = 12.sp, lineHeight = 18.sp)
                }
                Surface(color = PanelAlt, shape = CircleShape, modifier = Modifier.size(48.dp)) {
                    Box(contentAlignment = Alignment.Center) { Text("⌁", color = Accent, fontSize = 27.sp) }
                }
            }
            PrimaryAction("＋   Pair a new device", onPair, Modifier.fillMaxWidth())
            Text("PRIVATE BY DEFAULT  ·  NO ACCOUNT REQUIRED", color = Muted, fontSize = 8.sp, letterSpacing = 1.sp)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            MiniMetric("0", "Trusted devices", Modifier.weight(1f))
            MiniMetric("0", "Active transfers", Modifier.weight(1f))
            MiniMetric("LOCAL", "Network mode", Modifier.weight(1f))
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
            Text("Quick access", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Text("YOUR WORKSPACE", color = Muted, fontSize = 8.sp, letterSpacing = 1.sp)
        }
        Panel {
            FeatureLine("⌁", "Your devices", "Pair and manage trusted devices.")
            DividerLine()
            FeatureLine("⇧", "File transfers", "Send files directly between devices.")
            DividerLine()
            FeatureLine("▤", "Universal clipboard", "Move text between your screens.")
            DividerLine()
            FeatureLine("⌘", "Device tools", "Use your phone and tablet in new ways.")
            PrimaryAction("Explore device tools  →", { onNavigate(BridgeTab.Tools) }, Modifier.fillMaxWidth())
        }
        Text("BRIDGE PREVIEW  ·  BUILT FOR YOUR DEVICES", color = Muted, fontSize = 8.sp, letterSpacing = 1.sp)
    }
}

@Composable
private fun MiniMetric(value: String, label: String, modifier: Modifier = Modifier) {
    Surface(modifier = modifier, color = Panel, shape = RoundedCornerShape(13.dp)) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(5.dp)) {
            Text(value, color = if (value == "LOCAL") Accent else Color.White, fontSize = 18.sp, fontWeight = FontWeight.Bold, maxLines = 1)
            Text(label, color = Muted, fontSize = 9.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun DevicesScreen(
    peers: List<BridgePeer>,
    trusted: List<BridgeTrustedDevice>,
    pairingStatus: String,
    onPair: (BridgePeer) -> Unit,
    onApprove: (BridgePendingPairing) -> Unit,
    onReject: (BridgePendingPairing) -> Unit,
    onRevoke: (String) -> Unit
) {
    val trustedIds = trusted.map { it.deviceId }.toSet()
    val untrustedPeers = peers.filterNot { it.deviceId in trustedIds }
    ScreenContent {
        ScreenHeading("YOUR ECOSYSTEM", "Devices", "Nearby Bridge devices appear here automatically.")
        Panel {
            Eyebrow("DISCOVERED DEVICES")
            Text("Available on your network", color = Color.White, fontSize = 18.sp, fontWeight = FontWeight.Bold)
            Text("Keep Bridge open on both devices and connect them to the same Wi-Fi network.", color = Muted, fontSize = 12.sp, lineHeight = 18.sp)
            if (untrustedPeers.isEmpty()) {
                Text("Searching for Bridge devices…", color = Muted, fontSize = 11.sp)
            } else {
                untrustedPeers.forEach { peer ->
                    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            Text(peer.name, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                            Text("${peer.platform} · Nearby", color = Muted, fontSize = 10.sp)
                        }
                        PrimaryAction("Connect", { onPair(peer) })
                    }
                }
            }
        }
        Panel {
            Eyebrow("TRUSTED DEVICES")
            if (trusted.isEmpty()) {
                Text("No trusted devices yet.", color = Muted, fontSize = 12.sp)
            } else {
                trusted.forEach { device ->
                    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            Text(device.name, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                            Text("${device.platform} · ${device.fingerprint.take(12)}", color = Muted, fontSize = 10.sp)
                        }
                        Button(onClick = { onRevoke(device.deviceId) }, colors = ButtonDefaults.buttonColors(containerColor = PanelAlt, contentColor = Amber)) {
                            Text("Revoke")
                        }
                    }
                }
            }
        }
        Text(pairingStatus, color = Amber, fontSize = 11.sp, lineHeight = 16.sp)
    }
}

@Composable
private fun TransfersScreen(
    incomingOffers: List<BridgeIncomingOffer>,
    trustedDevices: List<BridgeTrustedDevice>,
    connectedSessionHandles: Map<String, Long>,
    onPickFile: (Long) -> Unit,
    onCancelTransfer: () -> Unit,
    transferStatus: String,
    receivedFiles: List<File>,
    onExportFile: (File) -> Unit,
    onAccept: (BridgeIncomingOffer) -> Unit,
    onReject: (BridgeIncomingOffer) -> Unit,
    onPair: () -> Unit
) {
    ScreenContent {
        ScreenHeading("MOVE WITHOUT LIMITS", "File transfers", "Receive files over your authenticated, encrypted local connection.")
        Panel {
            Eyebrow("SEND A FILE")
            val connected = trustedDevices.filter { connectedSessionHandles.containsKey(it.deviceId) }
            if (connected.isEmpty()) {
                Text("Connect a trusted device first. Online devices will appear here.", color = Muted, fontSize = 12.sp)
            } else {
                Text("Choose a connected device, then select a file from your phone.", color = Muted, fontSize = 11.sp, lineHeight = 16.sp)
                connected.forEach { device ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            Text(device.name, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                            Text("Secure connection ready", color = Good, fontSize = 10.sp)
                        }
                        PrimaryAction("Choose file", { connectedSessionHandles[device.deviceId]?.let(onPickFile) })
                    }
                    DividerLine()
                }
            }
        }
        Panel {
            Eyebrow("INCOMING REQUESTS")
            if (incomingOffers.isEmpty()) {
                Text("No incoming file requests.", color = Muted, fontSize = 12.sp)
            } else {
                incomingOffers.forEach { offer ->
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text(offer.fileName, color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.Bold)
                        Text("${offer.deviceId} · ${"%.2f".format(offer.fileSize / 1048576.0)} MB", color = Muted, fontSize = 10.sp)
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            PrimaryAction("Accept", { onAccept(offer) }, Modifier.weight(1f))
                            Button(onClick = { onReject(offer) }, colors = ButtonDefaults.buttonColors(containerColor = PanelAlt, contentColor = Amber)) {
                                Text("Decline")
                            }
                        }
                    }
                    DividerLine()
                }
            }
        }
        Panel {
            Eyebrow("RECEIVED FILES")
            if (receivedFiles.isEmpty()) {
                Text("Files you accept will appear here.", color = Muted, fontSize = 12.sp)
            } else {
                receivedFiles.forEach { file ->
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(10.dp)
                    ) {
                        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                            Text(file.name, color = Color.White, fontSize = 12.sp, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            Text("%.2f MB".format(file.length() / 1048576.0), color = Muted, fontSize = 10.sp)
                        }
                        Button(onClick = { onExportFile(file) }, colors = ButtonDefaults.buttonColors(containerColor = Accent, contentColor = Ink)) {
                            Text("Save to…", fontWeight = FontWeight.Bold)
                        }
                    }
                    DividerLine()
                }
            }
        }
        Panel {
            Eyebrow("LIVE TRANSFER ACTIVITY")
            Text(transferStatus, color = Color.White, fontSize = 12.sp, lineHeight = 18.sp)
            val transferPercent = Regex("(\\d+)%").find(transferStatus)?.groupValues?.get(1)?.toFloatOrNull()?.div(100f)
            if (transferPercent != null) {
                Box(modifier = Modifier.fillMaxWidth().height(8.dp).background(PanelAlt, RoundedCornerShape(8.dp))) {
                    Box(modifier = Modifier.fillMaxWidth(transferPercent.coerceIn(0f, 1f)).fillMaxHeight().background(Accent, RoundedCornerShape(8.dp)))
                }
            }
            if (transferStatus.startsWith("Sending ") || transferStatus.startsWith("Receiving ") || transferStatus.startsWith("Waiting for the receiving device")) {
                Button(onClick = onCancelTransfer, colors = ButtonDefaults.buttonColors(containerColor = PanelAlt, contentColor = Amber)) {
                    Text("Cancel transfer")
                }
            }
            Text("Received files stay private until you choose Save to… and select a destination such as Downloads.", color = Muted, fontSize = 10.sp, lineHeight = 15.sp)
        }
        Panel {
            FeatureLine("◇", "Encrypted in transit", "Uses the authenticated Noise session.", "ACTIVE")
            DividerLine()
            FeatureLine("✓", "SHA-256 integrity", "Files are checked before being published.", "ACTIVE")
            DividerLine()
            FeatureLine("▣", "No overwrite", "Existing files are kept; received files get a numbered name.", "ACTIVE")
        }
        PrimaryAction("＋   Pair a device", onPair, Modifier.fillMaxWidth())
    }
}

@Composable
private fun ClipboardScreen(enabled: Boolean, onEnabledChange: (Boolean) -> Unit) {
    ScreenContent {
        ScreenHeading("SHARE WHAT YOU COPY", "Universal clipboard", "Copy on one device. Paste on another.")
        Panel {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween) {
                Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                    Text("Clipboard sync", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Bold)
                    Text("Allow trusted devices to share text.", color = Muted, fontSize = 11.sp, lineHeight = 16.sp)
                }
                Switch(checked = enabled, onCheckedChange = onEnabledChange)
            }
            StatusTag(if (enabled) "PREFERENCE ENABLED · SERVICE NOT CONNECTED" else "CLIPBOARD SYNC PAUSED", if (enabled) Accent else Muted)
        }
        Panel {
            Eyebrow("RECENT CLIPBOARD")
            Box(modifier = Modifier.fillMaxWidth().padding(vertical = 18.dp), contentAlignment = Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("▤", color = Accent, fontSize = 36.sp)
                    Text("Nothing copied yet", color = Color.White, fontWeight = FontWeight.SemiBold)
                    Text("Shared text will appear here when sync is connected.", color = Muted, fontSize = 11.sp)
                }
            }
        }
        Text("This switch saves a local UI preference only; clipboard monitoring is not active.", color = Muted, fontSize = 10.sp, lineHeight = 15.sp)
    }
}

@Composable
private fun ToolsScreen() {
    ScreenContent {
        ScreenHeading("MORE FROM YOUR DEVICES", "Device tools", "Make your phone, PC and tablet work as one.")
        Panel {
            FeatureLine("♫", "Phone as microphone", "Use your phone as a wireless PC input.")
            DividerLine()
            FeatureLine("▣", "Phone as webcam", "Bring your mobile camera into desktop apps.")
            DividerLine()
            FeatureLine("◖", "PC audio on phone", "Listen to your computer from another device.")
            DividerLine()
            FeatureLine("⌖", "Remote input", "Use a trusted phone or tablet as a controller.")
            DividerLine()
            FeatureLine("▤", "Phone notifications", "See selected mobile notifications on desktop.")
            DividerLine()
            FeatureLine("▦", "Second screen", "Explore a tablet as an extended workspace.")
        }
        Text("These capabilities are planned; no microphone, camera or remote-control service is running yet.", color = Muted, fontSize = 10.sp, lineHeight = 15.sp)
    }
}

@Composable
private fun SettingsScreen(
    notifications: Boolean,
    onNotificationsChange: (Boolean) -> Unit,
    autoAccept: Boolean,
    onAutoAcceptChange: (Boolean) -> Unit,
    updateChannel: String,
    onUpdateChannelChange: (String) -> Unit,
    updateStatus: String,
    onCheckUpdates: () -> Unit
) {
    ScreenContent {
        ScreenHeading("MAKE IT YOURS", "Settings", "Privacy and preferences for your Bridge workspace.")
        Panel {
            Eyebrow("THIS DEVICE")
            Text("Android device", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Text("Bridge mobile · Version ${BuildConfig.VERSION_NAME}", color = Muted, fontSize = 11.sp)
            StatusTag("NOT PAIRED")
        }
        Panel {
            Eyebrow("PREFERENCES")
            PreferenceSwitch("Notifications", "Show Bridge activity notifications.", notifications, onNotificationsChange)
            DividerLine()
            PreferenceSwitch("Auto-accept trusted devices", "Skip confirmation for trusted requests.", autoAccept, onAutoAcceptChange)
        }
        Panel {
            Eyebrow("SOFTWARE UPDATES")
            Text("Update channel", color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("development" to "Development", "stable" to "Stable").forEach { (value, label) ->
                    Button(
                        onClick = { onUpdateChannelChange(value) },
                        colors = ButtonDefaults.buttonColors(
                            containerColor = if (updateChannel == value) Accent else PanelAlt,
                            contentColor = if (updateChannel == value) Ink else Color.White
                        )
                    ) { Text(label, fontSize = 11.sp, fontWeight = FontWeight.Bold) }
                }
            }
            Text(
                if (updateChannel == "stable") "Stable releases will appear here when the Stable channel launches."
                else "Development builds contain the newest features and may be less stable.",
                color = Muted, fontSize = 10.sp, lineHeight = 15.sp
            )
            Text(updateStatus, color = Amber, fontSize = 11.sp, lineHeight = 16.sp)
            PrimaryAction("Check for updates", onCheckUpdates, Modifier.fillMaxWidth())
        }
        Panel {
            Eyebrow("PRIVACY & SECURITY")
            FeatureLine("◇", "Local-first by design", "No mandatory account or cloud relay.", "CORE")
            DividerLine()
            FeatureLine("⌑", "Per-device permissions", "Choose what each device can access.", "PLANNED")
            DividerLine()
            FeatureLine("⚿", "Encrypted connections", "Secure transport foundation in shared core.", "FOUNDATION")
        }
        Panel {
            Eyebrow("ABOUT BRIDGE")
            Text("Your devices. Your rules.", color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Bold)
            Text("Version 0.3 preview · Native Android interface", color = Muted, fontSize = 11.sp)
            Text("BRIDGE · BUILT FOR YOUR DEVICES", color = Accent, fontSize = 8.sp, letterSpacing = 1.sp)
        }
    }
}

@Composable
private fun PreferenceSwitch(title: String, detail: String, checked: Boolean, onCheckedChange: (Boolean) -> Unit) {
    Row(modifier = Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(title, color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
            Text(detail, color = Muted, fontSize = 10.sp, lineHeight = 15.sp)
        }
        Switch(checked = checked, onCheckedChange = onCheckedChange)
    }
}

@Composable
private fun DividerLine() {
    Spacer(modifier = Modifier.fillMaxWidth().height(1.dp).background(Border))
}
