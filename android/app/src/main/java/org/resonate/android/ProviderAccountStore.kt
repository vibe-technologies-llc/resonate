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
import org.resonate.android.nativecore.ScrobbleAccount
import org.resonate.android.nativecore.SyncConfig

class ProviderAccountStore(context: Context) {
    private val storage = context.getSharedPreferences("provider-accounts", Context.MODE_PRIVATE)
    private val alias = "resonate-provider-accounts"

    @Synchronized
    fun readAccounts(): ProviderAccountBook {
        if (!ProviderAccountRecords.isCurrent(storage.all)) {
            committed(ProviderAccountRecords.migration(storage.all, ::encrypt), ACCOUNTS_NOT_SAVED)
        }
        return ProviderAccountRecords.decode(storage.all, ::open)
    }

    @Synchronized
    fun writeAccounts(book: ProviderAccountBook) {
        committed(ProviderAccountRecords.encode(book, ::encrypt, storage.all.keys), ACCOUNTS_NOT_SAVED)
    }

    @Synchronized
    fun change(edit: (ProviderAccountBook) -> ProviderAccountBook): ProviderAccountBook {
        val changed = edit(readAccounts())
        writeAccounts(changed)
        return changed
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

    @Synchronized fun readAuddToken() = decrypt("audd-token")

    @Synchronized
    fun writeAuddToken(value: String) {
        check(storage.edit().putString("audd-token", encrypt(value)).commit()) {
            "The AudD token could not be saved"
        }
    }

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
            nearbyOnly = storage.getBoolean("sync-nearby-only", false),
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
                .putBoolean("sync-nearby-only", config.nearbyOnly)
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

    private fun committed(changes: Changes, failure: String) {
        val edit = storage.edit()
        changes.forEach { (name, value) ->
            if (value == null) edit.remove(name) else edit.putString(name, value)
        }
        check(edit.commit()) { failure }
    }

    private fun decrypt(name: String): String = open(storage.getString(name, null).orEmpty())

    private fun open(stored: String): String {
        if (stored.isBlank()) return ""
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

    private companion object {
        const val ACCOUNTS_NOT_SAVED = "Provider accounts could not be saved"
    }
}
