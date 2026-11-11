package app.bridge.android

internal data class AvcRecoveryDecision(
    val sequenceGap: Boolean,
    val parameterSets: AvcParameterSets?
)

/**
 * Tracks the encoded access-unit sequence and retains the latest SPS/PPS so a
 * decoder can be recreated after a dropped frame even when later IDRs omit CSD.
 */
internal class AvcDecoderRecoveryState {
    private var lastSequence = 0L
    private var cachedParameterSets: AvcParameterSets? = null

    fun observe(frame: BridgeEncodedVideoFrame): AvcRecoveryDecision {
        val sequenceGap = lastSequence != 0L && frame.sequence != lastSequence + 1L
        lastSequence = frame.sequence

        if (frame.keyframe) {
            AvcAnnexBParser.extract(frame.bitstream)?.let { extracted ->
                cachedParameterSets = AvcParameterSets(
                    sps = extracted.sps.copyOf(),
                    pps = extracted.pps.copyOf()
                )
            }
        }

        return AvcRecoveryDecision(
            sequenceGap = sequenceGap,
            parameterSets = if (frame.keyframe) cachedParameterSets else null
        )
    }

    fun reset() {
        lastSequence = 0L
        cachedParameterSets = null
    }
}
