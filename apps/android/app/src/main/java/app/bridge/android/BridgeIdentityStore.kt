package app.bridge.android

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Stores the Bridge Noise static private key encrypted with an AES-GCM key
 * protected by Android Keystore. The wrapping key is non-exportable.
 *
 * This stores key material only; it does not itself establish or authenticate
 * a network session.
 */
internal object BridgeIdentityStore {
    private const val KEY_ALIAS = "bridge.identity.wrap.v1"
    private const val PREFS = "bridge_secure_identity"
    private const val PRIVATE_KEY = "noise_static_private_key"
    private const val KEYSTORE = "AndroidKeyStore"

    @Synchronized
    fun loadOrCreate(context: Context): ByteArray {
        val preferences = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val encoded = preferences.getString(PRIVATE_KEY, null)
        if (encoded != null) {
            val blob = Base64.decode(encoded, Base64.NO_WRAP)
            require(blob.size > 12 + 16) { "Stored Bridge identity is corrupted" }
            val iv = blob.copyOfRange(0, 12)
            val ciphertext = blob.copyOfRange(12, blob.size)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, wrappingKey(), GCMParameterSpec(128, iv))
            return cipher.doFinal(ciphertext)
        }

        val privateKey = ByteArray(32).also(SecureRandom()::nextBytes)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, wrappingKey())
        val ciphertext = cipher.doFinal(privateKey)
        val blob = cipher.iv + ciphertext
        check(preferences.edit().putString(PRIVATE_KEY, Base64.encodeToString(blob, Base64.NO_WRAP)).commit()) {
            "Could not persist Bridge identity"
        }
        return privateKey
    }

    private fun wrappingKey(): SecretKey {
        val keyStore = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }

        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return generator.generateKey()
    }
}
