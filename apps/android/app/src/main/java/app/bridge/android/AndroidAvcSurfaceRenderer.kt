package app.bridge.android

import android.media.MediaCodec
import android.media.MediaFormat
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.viewinterop.AndroidView
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.nio.ByteBuffer
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

/**
 * Diagnostic AVC renderer. Raw BGRA remains visible until MediaCodec has actually
 * released at least one decoded output buffer to the Surface.
 */
@Composable
internal fun AvcSurfaceRenderer(
    frame: BridgeEncodedVideoFrame?,
    modifier: Modifier = Modifier,
    onStatus: (String) -> Unit
) {
    var rendered by remember { mutableStateOf(false) }
    val decoder = remember {
        AvcSurfaceDecoder(
            onStatus = { message -> onStatus(message) },
            onRendered = { ready -> rendered = ready }
        )
    }

    AndroidView(
        factory = { context ->
            SurfaceView(context).apply {
                holder.addCallback(decoder)
                setZOrderMediaOverlay(true)
            }
        },
        modifier = modifier.graphicsLayer { alpha = if (rendered) 1f else 0f }
    )

    LaunchedEffect(frame?.sequence) {
        if (frame != null) {
            withContext(Dispatchers.IO) { decoder.submit(frame) }
        }
    }

    DisposableEffect(decoder) {
        onDispose { decoder.close() }
    }
}


private class AvcSurfaceDecoder(
    private val onStatus: (String) -> Unit,
    private val onRendered: (Boolean) -> Unit
) : SurfaceHolder.Callback, AutoCloseable {
    private val executor = ThreadPoolExecutor(
        1, 1, 0L, TimeUnit.MILLISECONDS,
        ArrayBlockingQueue(2),
        { task -> Thread(task, "BridgeAvcDecoder").apply { isDaemon = true } },
        ThreadPoolExecutor.DiscardOldestPolicy()
    )

    @Volatile private var closed = false
    private var surface: Surface? = null
    private var codec: MediaCodec? = null
    private var configuredWidth = 0
    private var configuredHeight = 0
    private var waitingForKeyframe = true
    private val recoveryState = AvcDecoderRecoveryState()

    override fun surfaceCreated(holder: SurfaceHolder) {
        val newSurface = holder.surface
        if (closed) return
        runCatching { executor.execute {
            if (!closed && newSurface.isValid) {
                surface = newSurface
                releaseCodec()
                postStatus("H.264 output Surface is ready; waiting for an IDR keyframe.")
            }
        } }
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) = Unit

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        if (closed) return
        runCatching { executor.execute {
            surface = null
            releaseCodec()
            postStatus("H.264 output Surface was destroyed; BGRA fallback remains available.")
        } }
    }

    fun submit(frame: BridgeEncodedVideoFrame) {
        if (closed) return
        try {
            executor.execute { decode(frame) }
        } catch (_: java.util.concurrent.RejectedExecutionException) {
            // A full bounded queue intentionally drops an older video frame.
        }
    }

    private fun decode(frame: BridgeEncodedVideoFrame) {
        if (closed) return
        val outputSurface = surface
        if (outputSurface == null || !outputSurface.isValid) return

        val recovery = recoveryState.observe(frame)
        if (recovery.sequenceGap) {
            releaseCodec()
            postStatus("H.264 frame sequence gap detected; decoder reset and waiting for the next IDR keyframe.")
        }

        if (codec != null && (configuredWidth != frame.width || configuredHeight != frame.height)) {
            releaseCodec()
        }

        if (codec == null) {
            if (!frame.keyframe) return
            val parameterSets = recovery.parameterSets
            if (parameterSets == null) {
                postStatus("Received an H.264 IDR frame but no cached SPS/PPS is available; waiting for a configured keyframe.")
                return
            }
            try {
                val format = MediaFormat.createVideoFormat(MediaFormat.MIMETYPE_VIDEO_AVC, frame.width, frame.height).apply {
                    setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, frame.bitstream.size.coerceAtLeast(64 * 1024))
                    setInteger(MediaFormat.KEY_FRAME_RATE, 60)
                    setByteBuffer("csd-0", ByteBuffer.wrap(parameterSets.sps))
                    setByteBuffer("csd-1", ByteBuffer.wrap(parameterSets.pps))
                }
                codec = MediaCodec.createDecoderByType(MediaFormat.MIMETYPE_VIDEO_AVC).also {
                    it.configure(format, outputSurface, null, 0)
                    it.start()
                }
                configuredWidth = frame.width
                configuredHeight = frame.height
                waitingForKeyframe = true
                postStatus("MediaCodec configured with SPS/PPS for ${frame.width}×${frame.height}; starting at IDR.")
            } catch (error: Exception) {
                releaseCodec()
                postRendered(false)
                postStatus("MediaCodec initialization failed: ${error.message}")
                return
            }
        }

        if (waitingForKeyframe && !frame.keyframe) return
        val activeCodec = codec ?: return
        try {
            val inputIndex = activeCodec.dequeueInputBuffer(5_000)
            if (inputIndex < 0) return
            val input = activeCodec.getInputBuffer(inputIndex) ?: run {
                activeCodec.queueInputBuffer(inputIndex, 0, 0, frame.timestampNs / 1_000L, 0)
                return
            }
            input.clear()
            if (frame.bitstream.size > input.remaining()) {
                activeCodec.queueInputBuffer(inputIndex, 0, 0, frame.timestampNs / 1_000L, 0)
                throw IllegalArgumentException("H.264 access unit exceeds the decoder input buffer capacity.")
            }
            input.put(frame.bitstream)
            activeCodec.queueInputBuffer(inputIndex, 0, frame.bitstream.size, frame.timestampNs / 1_000L, 0)
            waitingForKeyframe = false

            val info = MediaCodec.BufferInfo()
            var outputIndex = activeCodec.dequeueOutputBuffer(info, 0)
            var renderedAny = false
            var drained = 0
            while (outputIndex >= 0 && drained < 8) {
                activeCodec.releaseOutputBuffer(outputIndex, true)
                renderedAny = true
                drained++
                outputIndex = activeCodec.dequeueOutputBuffer(info, 0)
            }
            if (renderedAny) {
                postRendered(true)
                postStatus("Android MediaCodec decoded H.264 output on the Surface.")
            }
        } catch (error: Exception) {
            releaseCodec()
            postRendered(false)
            postStatus("H.264 decoding stopped; BGRA fallback remains active: ${error.message}")
        }
    }

    private fun releaseCodec() {
        val active = codec
        codec = null
        configuredWidth = 0
        configuredHeight = 0
        waitingForKeyframe = true
        if (active != null) {
            runCatching { active.stop() }
            runCatching { active.release() }
        }
        postRendered(false)
    }

    private fun postStatus(message: String) {
        android.os.Handler(android.os.Looper.getMainLooper()).post {
            if (!closed) onStatus(message)
        }
    }

    private fun postRendered(ready: Boolean) {
        android.os.Handler(android.os.Looper.getMainLooper()).post {
            if (!closed) onRendered(ready)
        }
    }

    override fun close() {
        if (closed) return
        closed = true
        executor.queue.clear()
        runCatching {
            executor.execute {
                releaseCodec()
                surface = null
            }
        }
        executor.shutdown()
        recoveryState.reset()
    }
}
