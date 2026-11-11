package app.bridge.android

import android.media.MediaCodecList
import android.os.Build

internal data class AvcDecoderCapability(
    val name: String,
    val hardwareAccelerated: Boolean?,
    val maxWidth: Int,
    val maxHeight: Int,
    val maxFrameRate: Double,
    val maxBitrate: Int,
    val supportsRedmiTargetMode: Boolean
)

/**
 * Inspects platform H.264 decoder declarations without creating or starting a codec.
 * This is safe to run during UI setup and does not claim that a decoder has been
 * successfully initialized for a particular Surface or stream configuration.
 */
internal fun inspectAvcDecoders(): List<AvcDecoderCapability> {
    return MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos
        .asSequence()
        .filterNot { it.isEncoder }
        .filter { info -> info.supportedTypes.any { it.equals("video/avc", ignoreCase = true) } }
        .mapNotNull { info ->
            runCatching {
                val capabilities = info.getCapabilitiesForType(
                    info.supportedTypes.first { it.equals("video/avc", ignoreCase = true) }
                )
                val video = capabilities.videoCapabilities ?: return@runCatching null
                val targetModeSupported = runCatching {
                    video.areSizeAndRateSupported(1080, 2340, 30.0)
                }.getOrDefault(false)
                AvcDecoderCapability(
                    name = info.name,
                    hardwareAccelerated = if (Build.VERSION.SDK_INT >= 29) info.isHardwareAccelerated else null,
                    maxWidth = video.supportedWidths.upper,
                    maxHeight = video.supportedHeights.upper,
                    maxFrameRate = video.supportedFrameRates.upper.toDouble(),
                    maxBitrate = video.bitrateRange.upper,
                    supportsRedmiTargetMode = targetModeSupported
                )
            }.getOrNull()
        }
        .sortedWith(
            compareByDescending<AvcDecoderCapability> { it.hardwareAccelerated == true }
                .thenByDescending { it.supportsRedmiTargetMode }
                .thenByDescending { it.maxBitrate }
        )
        .toList()
}
