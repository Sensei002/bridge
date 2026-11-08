package app.bridge.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.ConcurrentHashMap

internal data class BridgePeer(
    val name: String,
    val host: String,
    val port: Int,
    val deviceId: String,
    val fingerprint: String,
    val platform: String
)

internal data class BridgePendingPairing(
    val handle: Long,
    val fingerprint: String,
    val deviceId: String,
    val name: String,
    val platform: String
)

internal data class BridgeTrustedDevice(
    val deviceId: String,
    val name: String,
    val platform: String,
    val fingerprint: String,
    val pairedAt: Long
)

/**
 * Owns the native Noise pairing lifecycle. The six-digit code is only accepted
 * after the shared Rust Noise XX handshake has authenticated the peer key.
 */
internal class BridgePairingController(
    context: Context,
    private val privateKey: ByteArray,
    private val fingerprint: String,
    private val deviceId: String,
    private val onPending: (BridgePendingPairing) -> Unit,
    private val onStatus: (String) -> Unit
) {
    private val appContext = context.applicationContext
    private val preferences = appContext.getSharedPreferences("bridge_trusted_devices", Context.MODE_PRIVATE)
    private val mainHandler = Handler(Looper.getMainLooper())
    private val keyHex = privateKey.joinToString("") { "%02x".format(it.toInt() and 0xff) }
    private val localDescriptor = JSONObject()
        .put("id", deviceId)
        .put("display_name", android.os.Build.MODEL?.take(64) ?: "Bridge Android")
        .put("platform", "android")
        .put("protocol_version", 1)
    private val activeSessions = ConcurrentHashMap<String, Long>()
    private val pendingSessions = ConcurrentHashMap<Long, Boolean>()
    @Volatile private var stopped = false

    fun start() {
        Thread({
            while (!stopped) {
                try {
                    val response = JSONObject(BridgeNative.pairAccept(request(bindIp = "0.0.0.0")))
                    handleResult(response)
                } catch (error: Exception) {
                    if (!stopped && error.message != "pairing listener timed out") postStatus("Secure pairing listener restarted: ${error.message}")
                    try { Thread.sleep(900) } catch (_: InterruptedException) { break }
                }
            }
        }, "BridgeNoiseAccept").apply { isDaemon = true; start() }
        postStatus("Authenticated Noise XX pairing listener is ready.")
    }

    fun pair(peer: BridgePeer, expectedFingerprint: String? = null) {
        Thread({
            try {
                val trusted = trustedDevices().firstOrNull { it.deviceId == peer.deviceId }
                val expected = expectedFingerprint ?: trusted?.fingerprint
                val response = JSONObject(
                    BridgeNative.pairConnect(request(host = peer.host, expectedFingerprint = expected, expectedDeviceId = peer.deviceId, port = peer.port))
                )
                handleResult(response)
            } catch (error: Exception) {
                postStatus("Secure pairing failed: ${error.message}")
            }
        }, "BridgeNoiseConnect").apply { isDaemon = true; start() }
    }

    fun approve(pending: BridgePendingPairing): Boolean {
        val currentTrusted = try { trustedDevices() } catch (error: Exception) {
            pendingSessions.remove(pending.handle)
            BridgeNative.sessionClose(pending.handle)
            postStatus("Could not read trusted-device registry: ${error.message}")
            return false
        }
        val existing = currentTrusted.firstOrNull { it.deviceId == pending.deviceId }
        if (existing != null && existing.fingerprint != pending.fingerprint) {
            pendingSessions.remove(pending.handle)
            BridgeNative.sessionClose(pending.handle)
            postStatus("Identity changed for a previously trusted device. Pairing blocked.")
            return false
        }
        val all = currentTrusted.filterNot { it.deviceId == pending.deviceId }.toMutableList()
        all += BridgeTrustedDevice(pending.deviceId, pending.name, pending.platform, pending.fingerprint, System.currentTimeMillis())
        if (!saveTrusted(all)) {
            pendingSessions.remove(pending.handle)
            BridgeNative.sessionClose(pending.handle)
            postStatus("Could not persist trusted-device approval.")
            return false
        }
        val sent = JSONObject(BridgeNative.sessionSend(pending.handle, """{"type":"pairing_decision","decision":"accepted"}"""))
        if (!sent.optBoolean("ok")) {
            saveTrusted(currentTrusted)
            pendingSessions.remove(pending.handle)
            BridgeNative.sessionClose(pending.handle)
            postStatus("Could not confirm pairing with the requesting device.")
            return false
        }
        pendingSessions.remove(pending.handle)
        activeSessions.put(pending.deviceId, pending.handle)?.let(BridgeNative::sessionClose)
        postStatus("${pending.name} is now connected and trusted.")
        return true
    }

    fun reject(pending: BridgePendingPairing) {
        pendingSessions.remove(pending.handle)
        BridgeNative.sessionSend(pending.handle, """{"type":"pairing_decision","decision":"rejected"}""")
        BridgeNative.sessionClose(pending.handle)
        postStatus("Pairing request rejected.")
    }

    private fun awaitRemoteApproval(handle: Long, remote: JSONObject, fingerprint: String) {
        try {
            val response = JSONObject(BridgeNative.sessionReceive(handle))
            if (!response.optBoolean("ok")) error(response.optString("error", "The other device did not respond."))
            val message = JSONObject(response.getString("message"))
            check(message.optString("type") == "pairing_decision" && message.optString("decision") == "accepted") {
                "The other device rejected the pairing request."
            }
            val peerId = remote.getString("id")
            val current = trustedDevices()
            val existing = current.firstOrNull { it.deviceId == peerId }
            check(existing == null || existing.fingerprint == fingerprint) { "Trusted device identity changed." }
            val all = current.filterNot { it.deviceId == peerId }.toMutableList()
            all += BridgeTrustedDevice(peerId, remote.getString("display_name"), remote.getString("platform"), fingerprint, System.currentTimeMillis())
            check(saveTrusted(all)) { "Could not save the trusted device." }
            activeSessions.put(peerId, handle)?.let(BridgeNative::sessionClose)
            postStatus("${remote.getString("display_name")} accepted. Device connected.")
        } catch (error: Exception) {
            BridgeNative.sessionClose(handle)
            postStatus("Pairing was not approved: ${error.message}")
        }
    }

    fun revoke(deviceId: String) {
        val remaining = trustedDevices().filterNot { it.deviceId == deviceId }
        if (!saveTrusted(remaining)) {
            postStatus("Could not persist device revocation.")
            return
        }
        activeSessions.remove(deviceId)?.let(BridgeNative::sessionClose)
        postStatus("Device trust revoked and its active session closed.")
    }

    fun trustedDevices(): List<BridgeTrustedDevice> {
        val encoded = preferences.getString("devices_encrypted_v1", null)
        if (encoded == null) {
            val legacy = preferences.getString("devices", null) ?: return emptyList()
            val migrated = parseTrustedDevices(JSONArray(legacy))
            check(saveTrusted(migrated)) { "Could not migrate trusted devices into Android Keystore-backed storage" }
            return migrated
        }
        val plaintext = BridgeIdentityStore.decryptLocalData(
            appContext, TRUST_STORE_PURPOSE, android.util.Base64.decode(encoded, android.util.Base64.NO_WRAP)
        )
        return parseTrustedDevices(JSONArray(String(plaintext, Charsets.UTF_8)))
    }

    private fun parseTrustedDevices(array: JSONArray): List<BridgeTrustedDevice> =
        (0 until array.length()).map { index ->
            val item = requireNotNull(array.optJSONObject(index)) { "Trusted-device record $index is invalid" }
            val deviceId = item.optString("id")
            val name = item.optString("name")
            val platform = item.optString("platform")
            val fingerprint = item.optString("fingerprint")
            require(deviceId.isNotBlank() && name.isNotBlank() && platform.isNotBlank()) {
                "Trusted-device record $index is incomplete"
            }
            require(fingerprint.matches(Regex("[0-9a-fA-F]{64}"))) {
                "Trusted-device record $index has an invalid fingerprint"
            }
            BridgeTrustedDevice(deviceId, name, platform, fingerprint, item.optLong("paired_at"))
        }

    fun stop() {
        stopped = true
        activeSessions.values.forEach(BridgeNative::sessionClose)
        activeSessions.clear()
        pendingSessions.keys.toList().forEach(BridgeNative::sessionClose)
        pendingSessions.clear()
    }

    private fun handleResult(response: JSONObject) {
        if (!response.optBoolean("ok")) {
            val error = response.optString("error", "Unknown pairing error")
            if (!stopped && error != "pairing listener timed out") postStatus(error)
            return
        }
        val remote = response.getJSONObject("remote_device")
        val handle = response.getLong("handle")
        if (stopped) {
            BridgeNative.sessionClose(handle)
            return
        }
        val peerId = remote.getString("id")
        val remoteFingerprint = response.getString("remote_fingerprint")
        val trusted = try { trustedDevices().firstOrNull { it.deviceId == peerId } } catch (error: Exception) {
            BridgeNative.sessionClose(handle)
            postStatus("Could not read trusted-device registry: ${error.message}")
            return
        }
        if (trusted != null) {
            if (trusted.fingerprint == remoteFingerprint) {
                activeSessions.put(peerId, handle)?.let(BridgeNative::sessionClose)
                postStatus("Secure session re-established with ${trusted.name}.")
            } else {
                BridgeNative.sessionClose(handle)
                postStatus("Peer identity mismatch. Existing trust was preserved and connection rejected.")
            }
            return
        }
        if (response.optString("role") == "initiator") {
            postStatus("Request sent to ${remote.getString("display_name")}. Waiting for approval on that device…")
            Thread({ awaitRemoteApproval(handle, remote, remoteFingerprint) }, "BridgePairApproval")
                .apply { isDaemon = true; start() }
            return
        }
        pendingSessions[handle] = true
        val pending = BridgePendingPairing(
            handle = handle,
            fingerprint = remoteFingerprint,
            deviceId = peerId,
            name = remote.getString("display_name"),
            platform = remote.getString("platform")
        )
        mainHandler.post {
            if (!stopped) onPending(pending) else {
                pendingSessions.remove(handle)
                BridgeNative.sessionClose(handle)
            }
        }
    }

    private fun request(
        host: String? = null,
        bindIp: String? = null,
        expectedFingerprint: String? = null,
        expectedDeviceId: String? = null,
        port: Int = 45821
    ): String {
        val json = JSONObject()
            .put("port", port)
            .put("timeout_ms", 1_000)
            .put("private_key_hex", keyHex)
            .put("local_device", localDescriptor)
            .put("expected_fingerprint", expectedFingerprint ?: JSONObject.NULL)
            .put("expected_device_id", expectedDeviceId ?: JSONObject.NULL)
        if (host != null) json.put("host", host)
        if (bindIp != null) json.put("bind_ip", bindIp)
        return json.toString()
    }

    private fun saveTrusted(devices: List<BridgeTrustedDevice>): Boolean {
        val array = JSONArray()
        devices.forEach { device ->
            array.put(JSONObject()
                .put("id", device.deviceId)
                .put("name", device.name)
                .put("platform", device.platform)
                .put("fingerprint", device.fingerprint)
                .put("paired_at", device.pairedAt))
        }
        val encrypted = BridgeIdentityStore.encryptLocalData(
            appContext, TRUST_STORE_PURPOSE, array.toString().toByteArray(Charsets.UTF_8)
        )
        val encoded = android.util.Base64.encodeToString(encrypted, android.util.Base64.NO_WRAP)
        return preferences.edit()
            .putString("devices_encrypted_v1", encoded)
            .remove("devices")
            .commit()
    }

    private fun postStatus(message: String) = mainHandler.post { onStatus(message) }

    private companion object {
        const val TRUST_STORE_PURPOSE = "bridge.trusted.devices.v1"
    }
}
