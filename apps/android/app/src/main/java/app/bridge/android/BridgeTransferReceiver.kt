package app.bridge.android

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import android.os.Handler
import android.os.Looper
import org.json.JSONObject
import java.io.File
import java.io.FileOutputStream
import java.io.FileInputStream
import java.nio.ByteBuffer
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

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
    private val transferLocks = ConcurrentHashMap<Long, ReentrantLock>()

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

    fun sendFile(uri: Uri, handle: Long) {
        val lock = transferLocks.computeIfAbsent(handle) { ReentrantLock() }
        Thread({
            lock.withLock {
                var staged: File? = null
                try {
                    val metadata = appContext.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                    val displayName = metadata?.use { cursor ->
                        if (cursor.moveToFirst()) cursor.getString(0) else null
                    } ?: "shared-file"
                    validateFileName(displayName)
                    val outgoingDir = File(appContext.cacheDir, "Bridge/Outgoing")
                    check(outgoingDir.exists() || outgoingDir.mkdirs()) { "Could not prepare temporary file storage." }
                    staged = File(outgoingDir, ".bridge-send-${UUID.randomUUID()}.part")
                    appContext.contentResolver.openInputStream(uri)?.use { input ->
                        FileOutputStream(staged).use { output ->
                            val buffer = ByteArray(64 * 1024)
                            var total = 0L
                            while (true) {
                                val count = input.read(buffer)
                                if (count < 0) break
                                total += count
                                check(total <= MAX_FILE_BYTES) { "File exceeds the 2 GiB transfer limit." }
                                output.write(buffer, 0, count)
                            }
                            output.fd.sync()
                        }
                    } ?: error("Android could not open the selected file.")
                    val source = staged
                    val size = source.length()
                    val digest = MessageDigest.getInstance("SHA-256")
                    FileInputStream(source).use { input ->
                        val buffer = ByteArray(64 * 1024)
                        while (true) {
                            val count = input.read(buffer)
                            if (count < 0) break
                            digest.update(buffer, 0, count)
                        }
                    }
                    val sha256 = digest.digest().joinToString("") { "%02x".format(it) }
                    val transferId = UUID.randomUUID()
                    sendControl(handle, JSONObject()
                        .put("type", "file_offer")
                        .put("offer", JSONObject()
                            .put("transfer_id", transferId.toString())
                            .put("file_name", displayName)
                            .put("file_size", size)
                            .put("sha256", sha256)))
                    postStatus("Waiting for the receiving device to approve $displayName…")
                    val queue = queues[handle] ?: error("The secure device session is no longer available.")
                    val acceptedPacket = queue.poll(120, TimeUnit.SECONDS)
                        ?: error("Timed out waiting for the receiving device.")
                    val accepted = parseControl(acceptedPacket)
                    when (accepted.optString("type")) {
                        "file_accept" -> check(accepted.optString("transfer_id") == transferId.toString()) { "Mismatched transfer approval." }
                        "file_reject" -> error("The receiving device declined the file.")
                        else -> error("Unexpected response while waiting for approval.")
                    }
                    postStatus("Sending $displayName…")
                    val fileDigest = MessageDigest.getInstance("SHA-256")
                    FileInputStream(source).use { input ->
                        val buffer = ByteArray(CHUNK_DATA_BYTES)
                        var offset = 0L
                        while (offset < size) {
                            val expected = minOf(buffer.size.toLong(), size - offset).toInt()
                            var read = 0
                            while (read < expected) {
                                val count = input.read(buffer, read, expected - read)
                                if (count < 0) error("The selected file changed while being sent.")
                                read += count
                            }
                            fileDigest.update(buffer, 0, read)
                            val chunk = ByteBuffer.allocate(CHUNK_HEADER_SIZE + 1 + read)
                                .put(CHUNK_TAG.toByte())
                                .put("BRF1".toByteArray(Charsets.US_ASCII))
                                .putLong(transferId.mostSignificantBits)
                                .putLong(transferId.leastSignificantBits)
                                .putLong(offset)
                                .put(buffer, 0, read)
                                .array()
                            sendBytes(handle, chunk)
                            offset += read
                            postStatus("Sending $displayName · ${(offset * 100 / maxOf(size, 1))}%")
                        }
                    }
                    val actualDigest = fileDigest.digest().joinToString("") { "%02x".format(it) }
                    check(actualDigest.equals(sha256, ignoreCase = true)) { "The selected file changed while being sent." }
                    val completedPacket = queue.poll(120, TimeUnit.SECONDS)
                        ?: error("Timed out waiting for transfer confirmation.")
                    val completed = parseControl(completedPacket)
                    when (completed.optString("type")) {
                        "file_complete" -> check(completed.optString("transfer_id") == transferId.toString()) { "Mismatched completion confirmation." }
                        "file_cancel" -> error("The receiving device cancelled the transfer.")
                        else -> error("The receiving device did not confirm the completed transfer.")
                    }
                    postStatus("$displayName sent successfully.")
                } catch (error: Exception) {
                    postStatus("Could not send file: ${error.message ?: error.javaClass.simpleName}")
                } finally {
                    staged?.delete()
                }
            }
        }, "BridgeFileSend-$handle").apply { isDaemon = true; start() }
    }

    fun accept(offer: BridgeIncomingOffer) {
        pending.remove(offer.transferId)
        val lock = transferLocks.computeIfAbsent(offer.handle) { ReentrantLock() }
        Thread({
            lock.withLock {
            val queue = queues[offer.handle] ?: return@withLock
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
            }
        }, "BridgeFileReceive-${offer.transferId}").apply { isDaemon = true; start() }
    }

    fun reject(offer: BridgeIncomingOffer) {
        pending.remove(offer.transferId)
        val lock = transferLocks.computeIfAbsent(offer.handle) { ReentrantLock() }
        lock.withLock {
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
    }

    fun stop() {
        readers.values.forEach { it.interrupt() }
        readers.clear()
        queues.clear()
        pending.clear()
        transferLocks.clear()
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
        sendBytes(handle, byteArrayOf(CONTROL_TAG.toByte()) + value.toString().toByteArray(Charsets.UTF_8))
    }

    private fun sendBytes(handle: Long, packet: ByteArray) {
        val result = JSONObject(BridgeNative.sessionSendBytes(handle, packet))
        check(result.optBoolean("ok")) { result.optString("error", "Could not send transfer packet.") }
    }

    private fun parseControl(packet: ByteArray): JSONObject {
        check(packet.isNotEmpty() && packet[0].toInt() == CONTROL_TAG) { "Expected a transfer control response." }
        return JSONObject(String(packet, 1, packet.size - 1, Charsets.UTF_8))
    }

    private fun validateFileName(name: String) {
        require(name.isNotBlank() && name.length <= 240 && name != "." && name != "..")
        require(!name.contains('/') && !name.contains('\\') && !name.contains(':'))
        require(name.none { it.isISOControl() })
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
