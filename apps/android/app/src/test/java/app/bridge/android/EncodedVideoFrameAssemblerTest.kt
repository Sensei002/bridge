package app.bridge.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder

class EncodedVideoFrameAssemblerTest {
    private fun packet(sequence: Long): ByteArray =
        ByteBuffer.allocate(35).order(ByteOrder.BIG_ENDIAN)
            .put(0x06.toByte())
            .putLong(sequence)
            .putInt(2)
            .putInt(2)
            .putLong(sequence * 1_000_000L)
            .putInt(1)
            .putInt(0)
            .put(0)
            .put(0x00)
            .array()

    @Test
    fun firstAccessUnitDoesNotReportGap() {
        val assembler = EncodedVideoFrameAssembler()
        val frame = requireNotNull(assembler.push(packet(1), receivedAtElapsedRealtimeNs = 0L))
        assertFalse(frame.sequenceGap)
    }

    @Test
    fun contiguousAccessUnitsDoNotReportGap() {
        val assembler = EncodedVideoFrameAssembler()
        assembler.push(packet(1), receivedAtElapsedRealtimeNs = 0L)
        val frame = requireNotNull(assembler.push(packet(2), receivedAtElapsedRealtimeNs = 0L))
        assertFalse(frame.sequenceGap)
    }

    @Test
    fun skippedAccessUnitRequestsRecoveryThroughGapFlag() {
        val assembler = EncodedVideoFrameAssembler()
        assembler.push(packet(1), receivedAtElapsedRealtimeNs = 0L)
        val frame = requireNotNull(assembler.push(packet(3), receivedAtElapsedRealtimeNs = 0L))
        assertTrue(frame.sequenceGap)
    }
}
