package app.bridge.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
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
    val sas: String,
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

    fun pair(peer: BridgePeer) {
        Thread({
            try {
                val trusted = trustedDevices().firstOrNull { it.deviceId == peer.deviceId }
                val expected = trusted?.fingerprint
                val response = JSONObject(
                    BridgeNative.pairConnect(request(host = peer.host, expectedFingerprint = expected))
                )
                handleResult(response)
            } catch (error: Exception) {
                postStatus("Secure pairing failed: ${error.message}")
            }
        }, "BridgeNoiseConnect").apply { isDaemon = true; start() }
    }

    fun approve(pending: BridgePendingPairing, otherScreenCode: String): Boolean {
        if (otherScreenCode.length != 6 || otherScreenCode.any { !it.isDigit() } || otherScreenCode != pending.sas) {
            postStatus("Verification code mismatch. Device was not trusted.")
            return false
        }
        val existing = trustedDevices().firstOrNull { it.deviceId == pending.deviceId }
        if (existing != null && existing.fingerprint != pending.fingerprint) {
            BridgeNative.sessionClose(pending.handle)
            postStatus("Identity changed for a previously trusted device. Pairing blocked.")
            return false
        }
        val all = trustedDevices().filterNot { it.deviceId == pending.deviceId }.toMutableList()
        all += BridgeTrustedDevice(pending.deviceId, pending.name, pending.platform, pending.fingerprint, System.currentTimeMillis())
        if (!saveTrusted(all)) {
            BridgeNative.sessionClose(pending.handle)
            postStatus("Could not persist trusted-device approval.")
            return false
        }
        activeSessions[pending.deviceId] = pending.handle
        postStatus("${pending.name} is trusted. Encrypted Noise session established.")
        return true
    }

    fun reject(pending: BridgePendingPairing) {
        BridgeNative.sessionClose(pending.handle)
        postStatus("Pairing rejected. No trust record was created.")
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
        val array = try { JSONArray(preferences.getString("devices", "[]")) } catch (_: Exception) { JSONArray() }
        return (0 until array.length()).mapNotNull { index ->
            val item = array.optJSONObject(index) ?: return@mapNotNull null
            BridgeTrustedDevice(
                item.optString("id"), item.optString("name"), item.optString("platform"),
                item.optString("fingerprint"), item.optLong("paired_at")
            )
        }
    }

    fun stop() { stopped = true }

    private fun handleResult(response: JSONObject) {
        if (!response.optBoolean("ok")) {
            val error = response.optString("error", "Unknown pairing error")
            if (!stopped) postStatus(error)
            return
        }
        val remote = response.getJSONObject("remote_device")
        val handle = response.getLong("handle")
        val peerId = remote.getString("id")
        val remoteFingerprint = response.getString("remote_fingerprint")
        val trusted = trustedDevices().firstOrNull { it.deviceId == peerId }
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
        val pending = BridgePendingPairing(
            handle = handle,
            sas = response.getString("sas"),
            fingerprint = remoteFingerprint,
            deviceId = peerId,
            name = remote.getString("display_name"),
            platform = remote.getString("platform")
        )
        mainHandler.post { if (!stopped) onPending(pending) else BridgeNative.sessionClose(handle) }
    }

    private fun request(
        host: String? = null,
        bindIp: String? = null,
        expectedFingerprint: String? = null
    ): String {
        val json = JSONObject()
            .put("port", 45821)
            .put("timeout_ms", 1_000)
            .put("private_key_hex", keyHex)
            .put("local_device", localDescriptor)
            .put("expected_fingerprint", expectedFingerprint ?: JSONObject.NULL)
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
        return preferences.edit().putString("devices", array.toString()).commit()
    }

    private fun postStatus(message: String) = mainHandler.post { onStatus(message) }
}
