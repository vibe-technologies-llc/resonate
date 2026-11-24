package org.resonate.android

import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.SyncStanding

sealed interface SyncJoining {
    data class Joined(val code: String) : SyncJoining {
        fun into(config: SyncConfig): SyncConfig = config.copy(key = code, enabled = true)
    }

    data class Waiting(val code: String) : SyncJoining

    data object NotACode : SyncJoining
}

object SyncPairing {
    fun joining(config: SyncConfig?, typed: String, spelled: (String) -> String?): SyncJoining {
        val code = spelled(typed.trim()) ?: return SyncJoining.NotACode
        return if (config == null) SyncJoining.Waiting(code)
        else SyncJoining.Joined(code)
    }
}

val SyncStanding.runs: Boolean
    get() = this != SyncStanding.OFF && this != SyncStanding.FAILED
