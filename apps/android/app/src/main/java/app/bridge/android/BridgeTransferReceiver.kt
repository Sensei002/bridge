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
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

private const val CLIPBOARD_TAG = 0x03
private const val DISPLAY_FRAME_TAG = 0x04
private const val DISPLAY_FRAME_HEADER_BYTES = 37
private const val DISPLAY_FRAME_FRAGMENT_BYTES = 48 * 1024
private const val MAX_DISPLAY_FRAME_BYTES = 64 * 1024 * 1024
private const val MAX_CLIPBOARD_TEXT_BYTES = 16 * 1024
private const val DISPLAY_INPUT_TAG = 0x05

internal data class BridgeDisplayFrame(
    val deviceId: String,
    val sequence: Long,
    val width: Int,
    val height: Int,
    val stride: Int,
    val timestampNs: Long,
    val pixels: ByteArray,
    val receivedAtElapsedRealtimeNs: Long = android.os.SystemClock.elapsedRealtimeNanos()
)

/** Reassembles one ordered BGRA8 frame and caps its allocation at 64 MiB. */
private class DisplayFrameAssembler {
    private data class Header(
        val sequence: Long, val width: Int, val height: Int, val stride: Int,
        val timestampNs: Long, val totalBytes: Int
    )
    private var active: Header? = null
    private var pixels: ByteArray? = null
    private var received = 0
    private var lastCompletedSequence = 0L

    fun reset() {
        active = null
        pixels = null
        received = 0
    }

    fun push(packet: ByteArray): BridgeDisplayFrame? {
        try {
            require(packet.size > DISPLAY_FRAME_HEADER_BYTES &&
                packet.size <= DISPLAY_FRAME_HEADER_BYTES + DISPLAY_FRAME_FRAGMENT_BYTES &&
                packet[0].toInt() and 0xff == DISPLAY_FRAME_TAG) { "Invalid display-frame packet size or tag." }
            val buffer = ByteBuffer.wrap(packet).order(java.nio.ByteOrder.BIG_ENDIAN)
            buffer.get()
            val header = Header(
                buffer.long, buffer.int, buffer.int, buffer.int, buffer.long, buffer.int
            )
            val offset = buffer.int
            val expectedStride = Math.multiplyExact(header.width, 4)
            val expectedBytes = header.stride.toLong() * header.height.toLong()
            require(header.sequence > 0 && header.width in 1..4096 && header.height in 1..4096 &&
                header.stride == expectedStride && expectedBytes in 1..MAX_DISPLAY_FRAME_BYTES.toLong() &&
                header.totalBytes.toLong() == expectedBytes) { "Invalid display-frame metadata." }
            val count = packet.size - DISPLAY_FRAME_HEADER_BYTES
            require(count in 1..DISPLAY_FRAME_FRAGMENT_BYTES &&
                offset >= 0 && offset.toLong() + count <= header.totalBytes.toLong()) {
                "Invalid display-frame fragment range."
            }
            if (offset == 0) {
                require(active == null) { "A new frame arrived before the previous frame completed." }
                require(header.sequence > lastCompletedSequence) { "Display-frame sequence is stale or duplicated." }
                active = header
                pixels = ByteArray(header.totalBytes)
                received = 0
            }
            require(active == header && offset == received) { "Display-frame fragments are out of order." }
            System.arraycopy(packet, DISPLAY_FRAME_HEADER_BYTES, requireNotNull(pixels), received, count)
            received += count
            if (received == header.totalBytes) {
                val completed = requireNotNull(pixels)
                lastCompletedSequence = header.sequence
                reset()
                return BridgeDisplayFrame(
                    deviceId = "", sequence = header.sequence, width = header.width,
                    height = header.height, stride = header.stride,
                    timestampNs = header.timestampNs, pixels = completed
                )
            }
            return null
        } catch (error: Exception) {
            reset()
            throw error
        }
    }
}


/**
 * Keeps at most one pending UI update per authenticated session. If rendering
 * falls behind the network reader, newer complete frames replace older ones
 * instead of building an unbounded main-thread callback backlog.
 */
internal class LatestDisplayFrameDispatcher(
    private val handler: Handler,
    private val onFrame: (BridgeDisplayFrame) -> Unit
) {
    private val latest = AtomicReference<BridgeDisplayFrame?>(null)
    private val scheduled = AtomicBoolean(false)
    @Volatile private var closed = false

    private val drain = object : Runnable {
        override fun run() {
            try {
                if (closed) {
                    latest.set(null)
                } else {
                    latest.getAndSet(null)?.let(onFrame)
                }
            } finally {
                scheduled.set(false)
                if (!closed && latest.get() != null && scheduled.compareAndSet(false, true)) {
                    if (!handler.post(this)) scheduled.set(false)
                }
            }
        }
    }

    fun offer(frame: BridgeDisplayFrame) {
        if (closed) return
        latest.set(frame)
        if (scheduled.compareAndSet(false, true) && !handler.post(drain)) {
            scheduled.set(false)
        }
    }

    fun close() {
        closed = true
        latest.set(null)
    }
}

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
    private val localNoiseFingerprint: String,
    private val onOffer: (BridgeIncomingOffer) -> Unit,
    private val onClipboardText: (String, String) -> Unit,
    private val onDisplayFrame: (BridgeDisplayFrame) -> Unit,
    private val onStatus: (String) -> Unit,
    private val onSessionEnded: (String, Long) -> Unit
) {
    private val appContext = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    private val queues = ConcurrentHashMap<Long, LinkedBlockingQueue<ByteArray>>()
    private val readers = ConcurrentHashMap<Long, Thread>()
    private val pending = ConcurrentHashMap<String, BridgeIncomingOffer>()
    private val transferLocks = ConcurrentHashMap<Long, ReentrantLock>()
    private val cancellations = ConcurrentHashMap<String, java.util.concurrent.atomic.AtomicBoolean>()
    private val displayInputSequence = java.util.concurrent.atomic.AtomicLong(0)
    private val displayDispatchers = ConcurrentHashMap<Long, LatestDisplayFrameDispatcher>()

    fun attach(deviceId: String, handle: Long, allowDisplayAttestation: Boolean = false) {
        if (readers.containsKey(handle)) return
        val queue = LinkedBlockingQueue<ByteArray>(8)
        val displayAssembler = DisplayFrameAssembler()
        val displayDispatcher = LatestDisplayFrameDispatcher(main, onDisplayFrame)
        displayDispatchers.put(handle, displayDispatcher)?.close()
        queues[handle] = queue
        val thread = Thread({
            try {
                while (!Thread.currentThread().isInterrupted) {
                    val packet = BridgeNative.sessionReceiveBytes(handle)
                        ?: error("The secure device session has ended.")
                    if (packet.size == 1 && packet[0] == 0.toByte()) {
                        error("The secure device session has ended.")
                    }
                    if (packet.isEmpty()) continue
                    if ((packet[0].toInt() and 0xff) == DISPLAY_FRAME_TAG) {
                        try {
                            displayAssembler.push(packet)?.let { frame ->
                                displayDispatcher.offer(frame.copy(deviceId = deviceId))
                            }
                        } catch (error: Exception) {
                            postStatus("Dropped invalid second-screen frame: ${error.message}")
                        }
                        continue
                    }
                    if ((packet[0].toInt() and 0xff) == CLIPBOARD_TAG) {
                        val payloadSize = packet.size - 1
                        if (payloadSize in 0..MAX_CLIPBOARD_TEXT_BYTES) {
                            val text = String(packet, 1, payloadSize, Charsets.UTF_8)
                            main.post { onClipboardText(deviceId, text) }
                        }
                        continue
                    }
                    if (packet[0].toInt() == CONTROL_TAG) {
                        val control = JSONObject(String(packet, 1, packet.size - 1, Charsets.UTF_8))
                        when (control.optString("type")) {
                            "file_offer" -> {
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
                            }
                            "display_challenge_request" -> {
                                require(allowDisplayAttestation) {
                                    "Display attestation is only available to an explicitly trusted Windows peer."
                                }
                                val requestId = control.getString("request_id")
                                val sessionId = control.getString("session_id")
                                val nonce = control.getString("nonce")
                                require(requestId.matches(Regex("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-5][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}"))) {
                                    "Invalid display challenge request ID."
                                }
                                require(sessionId.matches(Regex("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-5][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}"))) {
                                    "Invalid display attestation session ID."
                                }
                                require(nonce.matches(Regex("[0-9a-fA-F]{64}"))) {
                                    "Invalid display challenge nonce."
                                }
                                require(localNoiseFingerprint.matches(Regex("[0-9a-fA-F]{64}"))) {
                                    "Local Noise identity fingerprint is invalid."
                                }
                                val seed = BridgeIdentityStore.loadOrCreateAttestationSeed(appContext)
                                val proof = try {
                                    JSONObject(BridgeNative.attestationSign(
                                        seed, nonce, sessionId, localNoiseFingerprint
                                    ))
                                } finally {
                                    seed.fill(0)
                                }
                                check(proof.optBoolean("ok")) {
                                    proof.optString("error", "Could not sign Android display attestation.")
                                }
                                val signature = proof.getString("signature")
                                check(signature.matches(Regex("[0-9a-fA-F]{128}"))) {
                                    "Native attestation signer returned an invalid signature."
                                }
                                check(proof.getString("device_fingerprint").equals(localNoiseFingerprint, ignoreCase = true)) {
                                    "Native attestation signer returned a mismatched device fingerprint."
                                }
                                sendControl(handle, JSONObject()
                                    .put("type", "display_challenge_response")
                                    .put("request_id", requestId)
                                    .put("session_id", sessionId)
                                    .put("nonce", nonce)
                                    .put("device_fingerprint", proof.getString("device_fingerprint"))
                                    .put("signature", signature))
                            }
                            else -> queue.put(packet)
                        }
                    } else {
                        queue.put(packet)
                    }
                }
            } catch (error: Exception) {
                if (!Thread.currentThread().isInterrupted) postStatus("Transfer connection ended: ${error.message}")
            } finally {
                displayDispatcher.close()
                displayDispatchers.remove(handle, displayDispatcher)
                queues.remove(handle, queue)
                readers.remove(handle, Thread.currentThread())
                onSessionEnded(deviceId, handle)
            }
        }, "BridgeTransferReader-$handle").apply { isDaemon = true; start() }
        readers[handle] = thread
    }

    fun sendFile(uri: Uri, handle: Long) {
        val lock = transferLocks.computeIfAbsent(handle) { ReentrantLock() }
        Thread({
            lock.withLock {
                var staged: File? = null
                var activeTransferId: String? = null
                try {
                    val metadata = appContext.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                    val displayName = metadata?.use { cursor ->
                        if (cursor.moveToFirst()) cursor.getString(0) else null
                    } ?: "shared-file"
                    validateFileName(displayName)
                    val outgoingDir = File(appContext.cacheDir, "Bridge/Outgoing")
                    check(outgoingDir.exists() || outgoingDir.mkdirs()) { "Could not prepare temporary file storage." }
                    val stagingFile = File(outgoingDir, ".bridge-send-${UUID.randomUUID()}.part")
                    staged = stagingFile
                    appContext.contentResolver.openInputStream(uri)?.use { input ->
                        FileOutputStream(stagingFile).use { output ->
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
                    val source = requireNotNull(staged)
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
                    activeTransferId = transferId.toString()
                    val cancelFlag = java.util.concurrent.atomic.AtomicBoolean(false)
                    cancellations[transferId.toString()] = cancelFlag
                    sendControl(handle, JSONObject()
                        .put("type", "file_offer")
                        .put("offer", JSONObject()
                            .put("transfer_id", transferId.toString())
                            .put("file_name", displayName)
                            .put("file_size", size)
                            .put("sha256", sha256)))
                    postStatus("Waiting for the receiving device to approve $displayName…")
                    val queue = queues[handle] ?: error("The secure device session is no longer available.")
                    var acceptedPacket: ByteArray? = null
                    val approvalDeadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(600)
                    while (acceptedPacket == null && System.nanoTime() < approvalDeadline) {
                        check(!cancelFlag.get()) { "Transfer cancelled by you." }
                        acceptedPacket = queue.poll(1, TimeUnit.SECONDS)
                    }
                    val approved = acceptedPacket ?: error("Timed out waiting for the receiving device.")
                    val accepted = parseControl(approved)
                    when (accepted.optString("type")) {
                        "file_accept" -> check(accepted.optString("transfer_id") == transferId.toString()) { "Mismatched transfer approval." }
                        "file_reject" -> error("The receiving device declined the file.")
                        else -> error("Unexpected response while waiting for approval.")
                    }
                    postStatus("Sending $displayName…")
                    val fileDigest = MessageDigest.getInstance("SHA-256")
                    var lastSpeedSample = System.nanoTime()
                    var lastSpeedBytes = 0L
                    FileInputStream(source).use { input ->
                        val buffer = ByteArray(CHUNK_DATA_BYTES)
                        var offset = 0L
                        while (offset < size) {
                            check(!cancelFlag.get()) { "Transfer cancelled by you." }
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
                            val now = System.nanoTime()
                            val elapsed = ((now - lastSpeedSample).coerceAtLeast(1L)) / 1_000_000_000.0
                            val speed = (offset - lastSpeedBytes) / elapsed / 1_048_576.0
                            lastSpeedSample = now
                            lastSpeedBytes = offset
                            postStatus("Sending " + displayName + " · " + (offset * 100 / maxOf(size, 1)) + "% · " + "%.2f MB/s".format(speed))
                        }
                    }
                    val actualDigest = fileDigest.digest().joinToString("") { "%02x".format(it) }
                    check(actualDigest.equals(sha256, ignoreCase = true)) { "The selected file changed while being sent." }
                    var completedPacket: ByteArray? = null
                    val completionDeadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(120)
                    while (completedPacket == null && System.nanoTime() < completionDeadline) {
                        check(!cancelFlag.get()) { "Transfer cancelled by you." }
                        completedPacket = queue.poll(1, TimeUnit.SECONDS)
                    }
                    val completed = parseControl(completedPacket ?: error("Timed out waiting for transfer confirmation."))
                    when (completed.optString("type")) {
                        "file_complete" -> check(completed.optString("transfer_id") == transferId.toString()) { "Mismatched completion confirmation." }
                        "file_cancel" -> error("The receiving device cancelled the transfer.")
                        else -> error("The receiving device did not confirm the completed transfer.")
                    }
                    postStatus("$displayName sent successfully.")
                } catch (error: Exception) {
                    if (activeTransferId != null) {
                        try { sendControl(handle, JSONObject().put("type", "file_cancel").put("transfer_id", activeTransferId)) } catch (_: Exception) { }
                    }
                    postStatus("Could not send file: ${error.message ?: error.javaClass.simpleName}")
                } finally {
                    staged?.delete()
                    activeTransferId?.let(cancellations::remove)
                }
            }
        }, "BridgeFileSend-$handle").apply { isDaemon = true; start() }
    }

    fun accept(offer: BridgeIncomingOffer) {
        pending.remove(offer.transferId)
        val lock = transferLocks.computeIfAbsent(offer.handle) { ReentrantLock() }
        val cancelFlag = java.util.concurrent.atomic.AtomicBoolean(false)
        cancellations[offer.transferId] = cancelFlag
        Thread({
            lock.withLock {
            val queue = queues[offer.handle] ?: return@withLock
            var temporary: File? = null
            var peerCancelled = false
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
                    var lastSpeedSample = System.nanoTime()
                    var lastSpeedBytes = 0L
                    while (received < offer.fileSize) {
                        check(!cancelFlag.get()) { "Transfer cancelled by you." }
                        val packet = queue.poll(1, TimeUnit.SECONDS) ?: continue
                        if (packet.isNotEmpty() && packet[0].toInt() == CONTROL_TAG) {
                            val control = parseControl(packet)
                            if (control.optString("type") == "file_cancel" &&
                                control.optString("transfer_id") == offer.transferId) {
                                peerCancelled = true
                                error("Transfer cancelled by the sending device.")
                            }
                            error("Unexpected transfer control message: ${control.optString("type")}")
                        }
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
                        val now = System.nanoTime()
                        val elapsed = ((now - lastSpeedSample).coerceAtLeast(1L)) / 1_000_000_000.0
                        val speed = (received - lastSpeedBytes) / elapsed / 1_048_576.0
                        lastSpeedSample = now
                        lastSpeedBytes = received
                        postStatus("Receiving " + offer.fileName + " · " + (received * 100 / maxOf(offer.fileSize, 1)) + "% · " + "%.2f MB/s".format(speed))
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
                if (!peerCancelled) {
                    try {
                        sendControl(offer.handle, JSONObject()
                            .put("type", "file_cancel")
                            .put("transfer_id", offer.transferId))
                        // Drain queued chunks until the sender acknowledges our cancellation.
                        // A full queue must never leave the single session reader blocked.
                        awaitPeerCancellation(queue, offer.transferId)
                    } catch (_: Exception) { }
                }
                postStatus("Could not receive ${offer.fileName}: ${error.message}")
            } finally {
                cancellations.remove(offer.transferId)
            }
            }
        }, "BridgeFileReceive-${offer.transferId}").apply { isDaemon = true; start() }
    }

    private fun awaitPeerCancellation(queue: LinkedBlockingQueue<ByteArray>, transferId: String) {
        val deadline = System.nanoTime() + TimeUnit.SECONDS.toNanos(10)
        while (System.nanoTime() < deadline) {
            val packet = queue.poll(100, TimeUnit.MILLISECONDS) ?: continue
            if (packet.isNotEmpty() && packet[0].toInt() == CONTROL_TAG) {
                val control = try { parseControl(packet) } catch (_: Exception) { continue }
                if (control.optString("type") == "file_cancel" &&
                    control.optString("transfer_id") == transferId) return
            }
        }
        // The sender may have disconnected; discard residual chunks before reuse.
        queue.clear()
    }

    fun sendDisplayInput(handle: Long, action: Int, pointerId: Int, x: Int, y: Int) {
        require(action in 1..4) { "Invalid display input action." }
        require(pointerId in 0..9) { "Invalid display pointer ID." }
        require(x in 0..65535 && y in 0..65535) { "Display coordinates must be normalized to 16-bit values." }
        val sequence = displayInputSequence.incrementAndGet()
        require(sequence > 0) { "Display input sequence exhausted." }
        val timestampNs = System.nanoTime()
        val packet = ByteBuffer.allocate(23).order(java.nio.ByteOrder.BIG_ENDIAN)
            .put(DISPLAY_INPUT_TAG.toByte())
            .putLong(sequence)
            .putLong(timestampNs)
            .put(pointerId.toByte())
            .put(action.toByte())
            .putShort(x.toShort())
            .putShort(y.toShort())
            .array()
        sendBytes(handle, packet)
    }

    fun sendClipboard(handle: Long, text: String) {
        val payload = text.toByteArray(Charsets.UTF_8)
        require(payload.size <= MAX_CLIPBOARD_TEXT_BYTES) { "Text exceeds the 16 KiB clipboard limit." }
        sendBytes(handle, byteArrayOf(CLIPBOARD_TAG.toByte()) + payload)
    }

    fun cancelAll() { cancellations.values.forEach { it.set(true) } }

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
        displayDispatchers.values.forEach { it.close() }
        displayDispatchers.clear()
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
        const val CHUNK_DATA_BYTES = 48 * 1024
        const val MAX_FILE_BYTES = 2L * 1024 * 1024 * 1024
    }
}
