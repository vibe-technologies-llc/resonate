package org.resonate.android

import org.resonate.android.nativecore.SyncConfig

class SyncSettingsUnread(message: String, cause: Exception) : Exception(message, cause)

class SyncSettingsReading(
    private val stored: () -> SyncConfig,
    private val unread: (Exception) -> Unit,
) {
    private var said = false

    @Synchronized
    fun read(): SyncConfig? =
        try {
            stored().also { said = false }
        } catch (failure: Exception) {
            if (!said) unread(failure)
            said = true
            null
        }
}
