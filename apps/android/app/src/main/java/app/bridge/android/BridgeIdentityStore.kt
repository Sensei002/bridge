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
 * Stores Bridge's long-lived identity seeds encrypted with an AES-GCM key
 * protected by Android Keystore. The wrapping key is non-exportable.
 *
 * The Ed25519 seed is encrypted at rest in app-private storage and is only
 * returned to the caller when needed for signing. Android Keystore protects
 * the wrapping key; it does not make the Ed25519 seed itself non-exportable.
 */
internal object BridgeIdentityStore {
    private const val KEY_ALIAS = "bridge.identity.wrap.v1"
    private const val PREFS = "bridge_secure_identity"
    private const val PRIVATE_KEY = "noise_static_private_key"
    private const val ATTESTATION_SEED = "android_attestation_seed_v1"
    private const val ATTESTATION_SEED_INITIALIZED = "android_attestation_seed_initialized_v1"
    private const val ATTESTATION_PURPOSE = "bridge.android.peer-attestation.seed.v1"
    private const val KEYSTORE = "AndroidKeyStore"
    private const val AES_GCM_IV_BYTES = 12
    private const val GCM_TAG_BITS = 128
    private const val ED25519_SEED_BYTES = 32

    @Synchronized
    fun loadOrCreate(context: Context): ByteArray {
        val preferences = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val encoded = preferences.getString(PRIVATE_KEY, null)
        if (encoded != null) {
            val blob = Base64.decode(encoded, Base64.NO_WRAP)
            require(blob.size > AES_GCM_IV_BYTES + 16) { "Stored Bridge identity is corrupted" }
            val iv = blob.copyOfRange(0, AES_GCM_IV_BYTES)
            val ciphertext = blob.copyOfRange(AES_GCM_IV_BYTES, blob.size)
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, wrappingKey(), GCMParameterSpec(GCM_TAG_BITS, iv))
            return cipher.doFinal(ciphertext)
        }

        val privateKey = ByteArray(ED25519_SEED_BYTES).also(SecureRandom()::nextBytes)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, wrappingKey())
        val ciphertext = cipher.doFinal(privateKey)
        val blob = cipher.iv + ciphertext
        check(preferences.edit().putString(PRIVATE_KEY, Base64.encodeToString(blob, Base64.NO_WRAP)).commit()) {
            "Could not persist Bridge identity"
        }
        return privateKey
    }

    /**
     * Load or create the per-install Ed25519 signing seed.
     *
     * The seed is protected by AES-256-GCM with a non-exportable Android
     * Keystore wrapping key and purpose-bound authenticated data. A marker is
     * committed with the ciphertext so accidental record loss is detected
     * instead of silently replacing an identity that may already be paired.
     */
    @Synchronized
    fun loadOrCreateAttestationSeed(context: Context): ByteArray {
        val preferences = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val encoded = preferences.getString(ATTESTATION_SEED, null)
        if (encoded != null) {
            val encrypted = Base64.decode(encoded, Base64.NO_WRAP)
            val seed = decryptLocalData(context, ATTESTATION_PURPOSE, encrypted)
            if (seed.size != ED25519_SEED_BYTES) {
                seed.fill(0)
                throw IllegalStateException("Stored Android attestation identity has an invalid length")
            }
            check(preferences.getBoolean(ATTESTATION_SEED_INITIALIZED, false)) {
                seed.fill(0)
                "Android attestation identity metadata is inconsistent"
            }
            return seed
        }

        check(!preferences.getBoolean(ATTESTATION_SEED_INITIALIZED, false)) {
            "Android attestation identity is missing; refusing to silently replace a paired identity"
        }

        val seed = ByteArray(ED25519_SEED_BYTES).also(SecureRandom()::nextBytes)
        try {
            val encrypted = encryptLocalData(context, ATTESTATION_PURPOSE, seed)
            val committed = preferences.edit()
                .putString(ATTESTATION_SEED, Base64.encodeToString(encrypted, Base64.NO_WRAP))
                .putBoolean(ATTESTATION_SEED_INITIALIZED, true)
                .commit()
            check(committed) { "Could not persist Android attestation identity" }
            return seed
        } catch (error: Throwable) {
            seed.fill(0)
            throw error
        }
    }

    /** Encrypt local records with purpose-bound authenticated data. */
    fun encryptLocalData(context: Context, purpose: String, plaintext: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, wrappingKey())
        cipher.updateAAD(purpose.toByteArray(Charsets.UTF_8))
        return cipher.iv + cipher.doFinal(plaintext)
    }

    /** Decrypt a purpose-bound local record; authentication failures are fatal. */
    fun decryptLocalData(context: Context, purpose: String, blob: ByteArray): ByteArray {
        require(blob.size > AES_GCM_IV_BYTES + 16) { "Encrypted Bridge data is truncated" }
        val iv = blob.copyOfRange(0, AES_GCM_IV_BYTES)
        val ciphertext = blob.copyOfRange(AES_GCM_IV_BYTES, blob.size)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, wrappingKey(), GCMParameterSpec(GCM_TAG_BITS, iv))
        cipher.updateAAD(purpose.toByteArray(Charsets.UTF_8))
        return cipher.doFinal(ciphertext)
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
