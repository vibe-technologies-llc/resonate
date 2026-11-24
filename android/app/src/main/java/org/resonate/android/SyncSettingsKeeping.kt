package org.resonate.android

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.resonate.android.nativecore.SyncConfig

class SyncSettingsUnread(message: String, cause: Exception) : Exception(message, cause)

class SyncSettingsUnsaved(message: String, cause: Exception) : Exception(message, cause)

enum class SyncSettingsTrouble {
    UNREAD,
    CODE_UNKEPT,
}

class SyncSettingsKeeping(
    private val stored: () -> SyncConfig,
    private val kept: (SyncConfig) -> Unit,
    private val minted: () -> String,
    private val unread: (SyncSettingsTrouble, Exception) -> Unit,
    private val unsaved: (Exception) -> Unit,
    private val sealedForgotten: () -> Unit,
) {
    private var said: SyncSettingsTrouble? = null
    private val troubleNow = MutableStateFlow<SyncSettingsTrouble?>(null)
    val trouble: StateFlow<SyncSettingsTrouble?> = troubleNow
    private val held = MutableStateFlow<SyncConfig?>(null)
    val config: StateFlow<SyncConfig?> = held
    private var generation = 0L
    private val asked = ArrayDeque<(SyncConfig) -> SyncConfig>()
    private val service = Mutex()
    private var applied = 0L

    @Synchronized fun read(): SyncConfig? = held.value ?: readStored()

    @Synchronized
    fun startOver(): SyncConfig? {
        if (held.value != null) return null
        sealedForgotten()
        return readStored()
    }

    @Synchronized
    fun ask(edit: (SyncConfig) -> SyncConfig) {
        asked.addLast(edit)
    }

    @Synchronized
    fun keepAsked(): Long? {
        val before = generation
        while (true) {
            val edit = asked.removeFirstOrNull() ?: break
            val base = held.value ?: continue
            val changed =
                try {
                    edit(base).also(kept)
                } catch (failure: Exception) {
                    unsaved(failure)
                    continue
                }
            generation += 1
            held.value = changed
        }
        return generation.takeIf { it != before }
    }

    suspend fun applying(upTo: Long?, apply: suspend (SyncConfig) -> Unit) {
        service.withLock {
            val (config, latest) = synchronized(this) { held.value to generation }
            if (config == null || upTo != null && applied >= upTo) return
            apply(config)
            applied = latest
        }
    }

    suspend fun <T> serving(action: suspend (SyncConfig?) -> T): T =
        service.withLock { action(held.value) }

    private fun readStored(): SyncConfig? {
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
        generation += 1
        held.value = keyed
        return keyed
    }

    private fun troubled(trouble: SyncSettingsTrouble, failure: Exception): SyncConfig? {
        if (said != trouble) unread(trouble, failure)
        said = trouble
        troubleNow.value = trouble
        return null
    }
}
