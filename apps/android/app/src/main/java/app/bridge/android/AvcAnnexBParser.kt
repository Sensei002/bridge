package app.bridge.android

internal data class AvcParameterSets(val sps: ByteArray, val pps: ByteArray)

/** Extracts the first SPS and PPS NAL units, retaining their Annex-B start codes. */
internal object AvcAnnexBParser {
    private data class StartCode(val offset: Int, val length: Int)

    fun extract(accessUnit: ByteArray): AvcParameterSets? {
        fun findStartCode(from: Int): StartCode? {
            var index = from
            while (index + 3 <= accessUnit.size) {
                if (accessUnit[index] == 0.toByte() && accessUnit[index + 1] == 0.toByte()) {
                    if (accessUnit[index + 2] == 1.toByte()) return StartCode(index, 3)
                    if (index + 4 <= accessUnit.size &&
                        accessUnit[index + 2] == 0.toByte() &&
                        accessUnit[index + 3] == 1.toByte()
                    ) return StartCode(index, 4)
                }
                index++
            }
            return null
        }

        var start = findStartCode(0) ?: return null
        var sps: ByteArray? = null
        var pps: ByteArray? = null
        while (true) {
            val nalStart = start.offset + start.length
            val next = findStartCode(nalStart)
            var nalEnd = next?.offset ?: accessUnit.size
            while (nalEnd > nalStart && accessUnit[nalEnd - 1] == 0.toByte()) nalEnd--
            if (nalEnd > nalStart) {
                when (accessUnit[nalStart].toInt() and 0x1f) {
                    7 -> if (sps == null) sps = accessUnit.copyOfRange(start.offset, nalEnd)
                    8 -> if (pps == null) pps = accessUnit.copyOfRange(start.offset, nalEnd)
                }
            }
            if (next == null) break
            start = next
        }
        val sequence = sps ?: return null
        val picture = pps ?: return null
        return AvcParameterSets(sequence, picture)
    }
}
