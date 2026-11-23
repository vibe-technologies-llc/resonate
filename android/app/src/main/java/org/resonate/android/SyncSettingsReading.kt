package org.resonate.android

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.resonate.android.nativecore.SyncConfig

class SyncSettingsUnread(message: String, cause: Exception) : Exception(message, cause)

enum class SyncSettingsTrouble {
    UNREAD,
    CODE_UNKEPT,
}

class SyncSettingsReading(
    private val stored: () -> SyncConfig,
    private val kept: (SyncConfig) -> Unit,
    private val minted: () -> String,
    private val unread: (SyncSettingsTrouble, Exception) -> Unit,
    private val sealedForgotten: () -> Unit,
) {
    private var said: SyncSettingsTrouble? = null
    private val troubleNow = MutableStateFlow<SyncSettingsTrouble?>(null)
    val trouble: StateFlow<SyncSettingsTrouble?> = troubleNow

    @Synchronized
    fun read(): SyncConfig? {
        val config =
            try {
                stored()
            } catch (failure: Exception) {
                return troubled(SyncSettingsTrouble.UNREAD, failure)
            }
        val keyed =
            if (config.key.isNotBlank()) config
            else
                try {
                    config.copy(key = minted()).also(kept)
                } catch (failure: Exception) {
                    return troubled(SyncSettingsTrouble.CODE_UNKEPT, failure)
                }
        said = null
        troubleNow.value = null
        return keyed
    }

    @Synchronized
    fun startOver(): SyncConfig? {
        sealedForgotten()
        return read()
    }

    private fun troubled(trouble: SyncSettingsTrouble, failure: Exception): SyncConfig? {
        if (said != trouble) unread(trouble, failure)
        said = trouble
        troubleNow.value = trouble
        return null
    }
}
