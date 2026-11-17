package org.resonate.android

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.resonate.android.nativecore.SyncConfig

class SyncSettingsUnread(message: String, cause: Exception) : Exception(message, cause)

class SyncSettingsReading(
    private val stored: () -> SyncConfig,
    private val unread: (Exception) -> Unit,
    private val sealedForgotten: () -> Unit,
) {
    private var said = false
    private val unreadableNow = MutableStateFlow(false)
    val unreadable: StateFlow<Boolean> = unreadableNow

    @Synchronized
    fun read(): SyncConfig? =
        try {
            stored().also {
                said = false
                unreadableNow.value = false
            }
        } catch (failure: Exception) {
            if (!said) unread(failure)
            said = true
            unreadableNow.value = true
            null
        }

    @Synchronized
    fun startOver(minted: String): SyncConfig? {
        sealedForgotten()
        return read()?.copy(key = minted)
    }
}
