package app.bridge.android

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertNotNull
import org.junit.Test

class AvcAnnexBParserTest {
    @Test
    fun extractsSpsAndPpsWithMixedAnnexBStartCodeLengths() {
        val accessUnit = byteArrayOf(
            0, 0, 0, 1, 0x67, 0x42, 0, 0x1f,
            0, 0, 1, 0x68, 0xce.toByte(), 0x06, 0xe2.toByte(),
            0, 0, 1, 0x65, 0x11
        )

        val result = AvcAnnexBParser.extract(accessUnit)

        assertNotNull(result)
        assertArrayEquals(
            byteArrayOf(0, 0, 0, 1, 0x67, 0x42, 0, 0x1f),
            result!!.sps
        )
        assertArrayEquals(
            byteArrayOf(0, 0, 1, 0x68, 0xce.toByte(), 0x06, 0xe2.toByte()),
            result.pps
        )
    }

    @Test
    fun returnsNullWhenSpsOrPpsIsMissing() {
        val spsOnly = byteArrayOf(0, 0, 1, 0x67, 0x42, 0, 0x1f)
        val ppsOnly = byteArrayOf(0, 0, 0, 1, 0x68, 0xce.toByte())

        assertNull(AvcAnnexBParser.extract(spsOnly))
        assertNull(AvcAnnexBParser.extract(ppsOnly))
    }

    @Test
    fun returnsNullForNonAnnexBInput() {
        assertNull(AvcAnnexBParser.extract(byteArrayOf(0x67, 0x42, 0x68)))
        assertNull(AvcAnnexBParser.extract(byteArrayOf()))
    }

    @Test
    fun ignoresTrailingZeroBytesAfterParameterSets() {
        val accessUnit = byteArrayOf(
            0, 0, 1, 0x67, 0x42,
            0, 0, 1, 0x68, 0xce.toByte(), 0, 0
        )

        val result = AvcAnnexBParser.extract(accessUnit)

        assertNotNull(result)
        assertArrayEquals(byteArrayOf(0, 0, 1, 0x67, 0x42), result!!.sps)
        assertArrayEquals(byteArrayOf(0, 0, 1, 0x68, 0xce.toByte()), result.pps)
    }
}
