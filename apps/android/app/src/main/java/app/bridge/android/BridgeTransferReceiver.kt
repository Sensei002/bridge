package app.bridge.android

import android.content.Context
import android.os.Handler
import android.os.Looper
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.nio.ByteBuffer
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.LinkedBlockingQueue

internal data class BridgeIncomingOffer(
    val deviceId: String,
    val handle: Long,
    val transferId: String,
    val fileName: String,
    val fileSize: Long,
    val sha256: String
)

/**
 * One reader owns each authenticated Noise session. All packets are placed on
 * a queue so an accepted transfer never competes with another socket reader.
 */
internal class BridgeTransferReceiver(
    context: Context,
    private val onOffer: (BridgeIncomingOffer) -> Unit,
    private val onStatus: (String) -> Unit
) {
    private val appContext = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    private val queues = ConcurrentHashMap<Long, LinkedBlockingQueue<ByteArray>>()
    private val readers = ConcurrentHashMap<Long, Thread>()
    private val pending = ConcurrentHashMap<String, BridgeIncomingOffer>()

    fun attach(deviceId: String, handle: Long) {
        if (readers.containsKey(handle)) return
        val queue = LinkedBlockingQueue<ByteArray>()
        queues[handle] = queue
        val thread = Thread({
            try {
                while (!Thread.currentThread().isInterrupted) {
                    val packet = BridgeNative.sessionReceiveBytes(handle) ?: continue
                    if (packet.isEmpty()) continue
                    if (packet[0].toInt() == CONTROL_TAG) {
                        val control = JSONObject(String(packet, 1, packet.size - 1, Charsets.UTF_8))
                        if (control.optString("type") == "file_offer") {
                            val value = control.getJSONObject("offer")
                            val offer = BridgeIncomingOffer(
                                deviceId, handle,
                                value.getString("transfer_id"),
                                value.getString("file_name"),
                                value.getLong("file_size"),
                                value.getString("sha256")
                            )
                            validate(offer)
                            pending[offer.transferId] = offer
                            main.post { onOffer(offer) }
                        } else {
                            queue.put(packet)
                        }
                    } else {
                        queue.put(packet)
                    }
                }
            } catch (error: Exception) {
                if (!Thread.currentThread().isInterrupted) postStatus("Transfer connection ended: ${error.message}")
            }
        }, "BridgeTransferReader-$handle").apply { isDaemon = true; start() }
        readers[handle] = thread
    }

    fun accept(offer: BridgeIncomingOffer) {
        pending.remove(offer.transferId)
        Thread({
            val queue = queues[offer.handle] ?: return@Thread
            var temporary: File? = null
            try {
                sendControl(offer.handle, JSONObject()
                    .put("type", "file_accept")
                    .put("transfer_id", offer.transferId))
                val directory = File(appContext.filesDir, "Bridge/Received")
                check(directory.exists() || directory.mkdirs()) { "Could not create the received-files folder." }
                temporary = File(directory, ".bridge-${offer.transferId}.part")
                FileOutputStream(temporary).use { output ->
                    val digest = MessageDigest.getInstance("SHA-256")
                    var received = 0L
                    while (received < offer.fileSize) {
                        val packet = queue.take()
                        check(packet[0].toInt() == CHUNK_TAG) { "Expected a file data packet." }
                        check(packet.size > CHUNK_HEADER_SIZE + 1) { "Invalid empty data chunk." }
                        check(String(packet, 1, 4, Charsets.US_ASCII) == "BRF1") { "Unknown data chunk format." }
                        val buffer = ByteBuffer.wrap(packet, 5, 24)
                        val id = UUID(buffer.long, buffer.long).toString()
                        val offset = buffer.long
                        val dataOffset = CHUNK_HEADER_SIZE + 1
                        val count = packet.size - dataOffset
                        check(id == offer.transferId && offset == received) { "Data chunk order or transfer ID mismatch." }
                        check(count in 1..(48 * 1024) && received + count <= offer.fileSize) { "Invalid data chunk size." }
                        output.write(packet, dataOffset, count)
                        digest.update(packet, dataOffset, count)
                        received += count
                    }
                    output.fd.sync()
                    val actual = digest.digest().joinToString("") { "%02x".format(it) }
                    check(actual.equals(offer.sha256, ignoreCase = true)) { "File integrity check failed." }
                }
                val destination = collisionSafeFile(directory, offer.fileName)
                check(temporary.renameTo(destination)) { "Could not publish the received file." }
                temporary = null
                sendControl(offer.handle, JSONObject()
                    .put("type", "file_complete")
                    .put("transfer_id", offer.transferId))
                postStatus("Received ${offer.fileName}. Saved inside Bridge's app storage.")
            } catch (error: Exception) {
                temporary?.delete()
                try {
                    sendControl(offer.handle, JSONObject()
                        .put("type", "file_cancel")
                        .put("transfer_id", offer.transferId))
                } catch (_: Exception) { }
                postStatus("Could not receive ${offer.fileName}: ${error.message}")
            }
        }, "BridgeFileReceive-${offer.transferId}").apply { isDaemon = true; start() }
    }

    fun reject(offer: BridgeIncomingOffer) {
        pending.remove(offer.transferId)
        try {
            sendControl(offer.handle, JSONObject()
                .put("type", "file_reject")
                .put("transfer_id", offer.transferId)
                .put("reason", "Declined on receiving device"))
            postStatus("Declined ${offer.fileName}.")
        } catch (error: Exception) {
            postStatus("Could not reject transfer: ${error.message}")
        }
    }

    fun stop() {
        readers.values.forEach { it.interrupt() }
        readers.clear()
        queues.clear()
        pending.clear()
    }

    private fun validate(offer: BridgeIncomingOffer) {
        require(offer.fileName.isNotBlank() && offer.fileName.length <= 240)
        require(offer.fileName != "." && offer.fileName != "..")
        require(!offer.fileName.contains('/') && !offer.fileName.contains('\\') && !offer.fileName.contains(':'))
        require(offer.fileName.none { it.isISOControl() })
        require(offer.fileSize in 0..MAX_FILE_BYTES)
        require(offer.sha256.matches(Regex("[0-9a-fA-F]{64}")))
    }

    private fun sendControl(handle: Long, value: JSONObject) {
        val json = value.toString().toByteArray(Charsets.UTF_8)
        val packet = byteArrayOf(CONTROL_TAG) + json
        val result = JSONObject(BridgeNative.sessionSendBytes(handle, packet))
        check(result.optBoolean("ok")) { result.optString("error", "Could not send transfer response.") }
    }

    private fun collisionSafeFile(directory: File, name: String): File {
        val dot = name.lastIndexOf('.').takeIf { it > 0 } ?: name.length
        val stem = name.substring(0, dot)
        val extension = name.substring(dot)
        for (index in 0..9999) {
            val candidate = File(directory, if (index == 0) name else "$stem ($index)$extension")
            if (!candidate.exists()) return candidate
        }
        error("Too many files with the same name.")
    }

    private fun postStatus(message: String) = main.post { onStatus(message) }

    private companion object {
        const val CONTROL_TAG = 0x01
        const val CHUNK_TAG = 0x02
        const val CHUNK_HEADER_SIZE = 28
        const val MAX_FILE_BYTES = 2L * 1024 * 1024 * 1024
    }
}
