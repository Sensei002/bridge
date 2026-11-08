package app.bridge.android

/**
 * JNI entry points backed by the exact same Rust Noise XX implementation as Windows.
 * Never pass private key material to logs or UI.
 */
internal object BridgeNative {
    init {
        System.loadLibrary("bridge_mobile")
    }

    external fun identityFingerprint(privateKeyHex: String): String
    external fun pairConnect(requestJson: String): String
    external fun pairAccept(requestJson: String): String
    external fun qrInvitationCreate(requestJson: String): String
    external fun qrInvitationConsume(uri: String, nowMs: Long): String
    external fun sessionSend(handle: Long, message: String): String
    external fun sessionReceive(handle: Long): String
    external fun sessionClose(handle: Long)
}
