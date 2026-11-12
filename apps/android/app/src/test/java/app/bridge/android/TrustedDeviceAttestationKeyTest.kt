package app.bridge.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class TrustedDeviceAttestationKeyTest {
    @Test
    fun acceptsNullAndLegacyNullStrings() {
        assertNull(parseOptionalAttestationPublicKey(null, 0))
        assertNull(parseOptionalAttestationPublicKey("", 0))
        assertNull(parseOptionalAttestationPublicKey("null", 0))
        assertNull(parseOptionalAttestationPublicKey(" NULL ", 0))
    }

    @Test
    fun acceptsValidEd25519PublicKeyHex() {
        val key = "a".repeat(64)
        assertEquals(key, parseOptionalAttestationPublicKey(key, 0))
    }

    @Test
    fun rejectsMalformedOptionalKeyWithoutThrowing() {
        assertNull(parseOptionalAttestationPublicKey("not-a-public-key", 0))
        assertNull(parseOptionalAttestationPublicKey("a".repeat(63), 0))
        assertNull(parseOptionalAttestationPublicKey("a".repeat(66), 0))
    }
}
