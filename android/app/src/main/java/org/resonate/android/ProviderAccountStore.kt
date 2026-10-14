package org.resonate.android

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.nativecore.ScrobbleAccount
import org.resonate.android.nativecore.SyncConfig

class ProviderAccountStore(context: Context) {
    private val storage = context.getSharedPreferences("provider-accounts", Context.MODE_PRIVATE)
    private val alias = "resonate-provider-accounts"

    @Synchronized
    fun read(): DownloadAccount =
        DownloadAccount(
            provider =
                MusicProvider.entries.find { it.name == storage.getString("provider", null) }
                    ?: MusicProvider.MONOCHROME,
            server = storage.getString("server", "").orEmpty(),
            user = decrypt("user"),
            password = decrypt("password"),
            clientId = decrypt("client-id"),
            clientSecret = decrypt("client-secret"),
            refreshToken = decrypt("refresh-token"),
        )

    @Synchronized
    fun write(account: DownloadAccount) {
        val edit =
            storage
                .edit()
                .putString("provider", account.provider.name)
                .putString("server", account.server)
        mapOf(
                "user" to account.user,
                "password" to account.password,
                "client-id" to account.clientId,
                "client-secret" to account.clientSecret,
                "refresh-token" to account.refreshToken,
            )
            .forEach { (name, value) -> edit.putString(name, encrypt(value)) }
        check(edit.commit()) { "Provider account could not be saved" }
    }

    @Synchronized
    fun readScrobbling() =
        ScrobbleAccount(
            decrypt("listenbrainz"),
            decrypt("lastfm-key"),
            decrypt("lastfm-secret"),
            decrypt("lastfm-session"),
        )

    @Synchronized
    fun writeScrobbling(account: ScrobbleAccount) {
        val edit = storage.edit()
        mapOf(
                "listenbrainz" to account.listenbrainzToken,
                "lastfm-key" to account.lastfmKey,
                "lastfm-secret" to account.lastfmSecret,
                "lastfm-session" to account.lastfmSession,
            )
            .forEach { (key, value) -> edit.putString(key, encrypt(value)) }
        check(edit.commit()) { "Scrobbling accounts could not be saved" }
    }

    @Synchronized fun readRecognitionKey() = decrypt("acoustid-key")

    @Synchronized
    fun writeRecognitionKey(value: String) {
        check(storage.edit().putString("acoustid-key", encrypt(value)).commit()) {
            "Recognition key could not be saved"
        }
    }

    @Synchronized
    fun readSync() =
        SyncConfig(
            enabled = storage.getBoolean("sync-enabled", true),
            server = storage.getString("sync-server", "").orEmpty(),
            key = decrypt("sync-key"),
            device = storage.getString("sync-device", "").orEmpty(),
            favourites = storage.getBoolean("sync-favourites", false),
            playlists = storage.getBoolean("sync-playlists", false),
            everything = storage.getBoolean("sync-everything", false),
            meteredAllowed = storage.getBoolean("sync-metered-allowed", false),
        )

    @Synchronized
    fun writeSync(config: SyncConfig) {
        val edit =
            storage
                .edit()
                .putBoolean("sync-enabled", config.enabled)
                .putString("sync-server", config.server)
                .putString("sync-key", encrypt(config.key))
                .putString("sync-device", config.device)
                .putBoolean("sync-favourites", config.favourites)
                .putBoolean("sync-playlists", config.playlists)
                .putBoolean("sync-everything", config.everything)
                .putBoolean("sync-metered-allowed", config.meteredAllowed)
        check(edit.commit()) { "Device sync settings could not be saved" }
    }

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(alias, null) as? SecretKey)?.let {
            return it
        }
        return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
            .apply {
                init(
                    KeyGenParameterSpec.Builder(
                            alias,
                            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                        )
                        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                        .build()
                )
            }
            .generateKey()
    }

    private fun encrypt(value: String): String {
        if (value.isEmpty()) return ""
        val cipher =
            Cipher.getInstance("AES/GCM/NoPadding").apply { init(Cipher.ENCRYPT_MODE, key()) }
        return Base64.encodeToString(cipher.iv, Base64.NO_WRAP) +
            ":" +
            Base64.encodeToString(cipher.doFinal(value.toByteArray(Charsets.UTF_8)), Base64.NO_WRAP)
    }

    private fun decrypt(name: String): String {
        val stored = storage.getString(name, null)?.takeIf { it.isNotBlank() } ?: return ""
        val parts = stored.split(':', limit = 2)
        val cipher =
            Cipher.getInstance("AES/GCM/NoPadding").apply {
                init(
                    Cipher.DECRYPT_MODE,
                    key(),
                    GCMParameterSpec(128, Base64.decode(parts[0], Base64.NO_WRAP)),
                )
            }
        return String(cipher.doFinal(Base64.decode(parts[1], Base64.NO_WRAP)), Charsets.UTF_8)
    }
}
