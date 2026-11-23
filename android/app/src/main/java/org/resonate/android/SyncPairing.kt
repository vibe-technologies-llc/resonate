package org.resonate.android

import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.SyncStanding

sealed interface SyncJoining {
    data class Joined(val config: SyncConfig) : SyncJoining

    data class Waiting(val code: String) : SyncJoining

    data object NotACode : SyncJoining
}

object SyncPairing {
    fun joining(config: SyncConfig?, typed: String, spelled: (String) -> String?): SyncJoining {
        val code = spelled(typed.trim()) ?: return SyncJoining.NotACode
        return if (config == null) SyncJoining.Waiting(code)
        else SyncJoining.Joined(config.copy(key = code, enabled = true))
    }
}

val SyncStanding.runs: Boolean
    get() = this != SyncStanding.OFF && this != SyncStanding.FAILED
