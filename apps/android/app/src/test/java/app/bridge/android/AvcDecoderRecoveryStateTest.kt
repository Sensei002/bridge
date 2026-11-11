package app.bridge.android

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AvcDecoderRecoveryStateTest {
    private fun frame(sequence: Long, keyframe: Boolean, bitstream: ByteArray) =
        BridgeEncodedVideoFrame(
            deviceId = "test",
            sequence = sequence,
            width = 1920,
            height = 1080,
            timestampNs = sequence * 16_666_667L,
            keyframe = keyframe,
            bitstream = bitstream,
            receivedAtElapsedRealtimeNs = 0L
        )

    private val configuredIdr = byteArrayOf(
        0, 0, 0, 1, 0x67, 0x42, 0, 0x1f,
        0, 0, 1, 0x68, 0xce.toByte(), 0x06, 0xe2.toByte(),
        0, 0, 1, 0x65, 0x11
    )

    @Test
    fun firstFrameDoesNotReportSequenceGap() {
        val state = AvcDecoderRecoveryState()

        val decision = state.observe(frame(12, true, configuredIdr))

        assertFalse(decision.sequenceGap)
        assertNotNull(decision.parameterSets)
    }

    @Test
    fun contiguousFramesDoNotTriggerRecovery() {
        val state = AvcDecoderRecoveryState()
        state.observe(frame(12, true, configuredIdr))

        val decision = state.observe(frame(13, false, byteArrayOf(0, 0, 1, 0x41, 0x22)))

        assertFalse(decision.sequenceGap)
        assertNull(decision.parameterSets)
    }

    @Test
    fun skippedAccessUnitTriggersRecovery() {
        val state = AvcDecoderRecoveryState()
        state.observe(frame(12, true, configuredIdr))

        val decision = state.observe(frame(14, false, byteArrayOf(0, 0, 1, 0x41, 0x22)))

        assertTrue(decision.sequenceGap)
        assertNull(decision.parameterSets)
    }

    @Test
    fun reusesCachedParameterSetsWhenRecoveryIdrOmitsThem() {
        val state = AvcDecoderRecoveryState()
        state.observe(frame(12, true, configuredIdr))

        val recoveryIdr = state.observe(frame(14, true, byteArrayOf(0, 0, 1, 0x65, 0x33)))

        assertTrue(recoveryIdr.sequenceGap)
        assertNotNull(recoveryIdr.parameterSets)
        assertArrayEquals(
            byteArrayOf(0, 0, 0, 1, 0x67, 0x42, 0, 0x1f),
            recoveryIdr.parameterSets!!.sps
        )
        assertArrayEquals(
            byteArrayOf(0, 0, 1, 0x68, 0xce.toByte(), 0x06, 0xe2.toByte()),
            recoveryIdr.parameterSets.pps
        )
    }

    @Test
    fun resetClearsSequenceAndCachedConfiguration() {
        val state = AvcDecoderRecoveryState()
        state.observe(frame(12, true, configuredIdr))
        state.reset()

        val decision = state.observe(frame(20, true, byteArrayOf(0, 0, 1, 0x65, 0x33)))

        assertFalse(decision.sequenceGap)
        assertNull(decision.parameterSets)
    }
}
