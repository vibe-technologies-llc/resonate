package org.resonate.android

import android.content.SharedPreferences
import android.os.SystemClock
import android.util.Log
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.NewerRelease

object NewerReleases {
    const val LOOKED_AT_AGAIN_EVERY_MS = 6 * 60 * 60 * 1_000L
    const val FOREGROUND_LOOKED_FOR_EVERY_MS = 60_000L

    fun shown(checks: Boolean, heard: NewerRelease?, dismissed: String?): NewerRelease? =
        heard?.takeIf { checks && it.version != dismissed }

    fun due(lookedAtMs: Long?, nowMs: Long): Boolean =
        lookedAtMs == null || nowMs - lookedAtMs >= LOOKED_AT_AGAIN_EVERY_MS
}

class ReleaseWatch(
    private val library: LibraryRepository,
    private val settings: SharedPreferences,
    private val scope: CoroutineScope,
    derived: CoroutineScope,
    private val foreground: () -> Boolean,
) {
    private val mutableChecks = MutableStateFlow(StoredChoices.checkForUpdates(settings::storedFlag))
    private val mutableAsksSomewhere = MutableStateFlow(false)
    private val heard = MutableStateFlow<NewerRelease?>(null)
    private val dismissed = MutableStateFlow(settings.getString(StoredChoices.UPDATE_DISMISSED, null))
    private var told: String? = null
    private var lookedAtMs: Long? = null
    private var looking: Job? = null

    val checks: StateFlow<Boolean> = mutableChecks.asStateFlow()
    val asksSomewhere: StateFlow<Boolean> = mutableAsksSomewhere.asStateFlow()
    val newer: StateFlow<NewerRelease?> =
        combine(mutableChecks, heard, dismissed) { checks, heard, dismissed ->
                NewerReleases.shown(checks, heard, dismissed)
            }
            .stateIn(derived, SharingStarted.Eagerly, null)

    fun begin(native: MobilePlayer) {
        mutableAsksSomewhere.value = native.namesWhereItWasReleased()
        look()
    }

    fun check(on: Boolean) {
        mutableChecks.value = on
        settings.edit().putBoolean(StoredChoices.CHECK_FOR_UPDATES, on).apply()
        look()
    }

    fun dismiss(version: String) {
        dismissed.value = version
        settings.edit().putString(StoredChoices.UPDATE_DISMISSED, version).apply()
    }

    private fun look() {
        looking?.cancel()
        lookedAtMs = null
        if (!mutableChecks.value) return
        looking =
            scope.launch {
                while (isActive) {
                    val now = SystemClock.elapsedRealtime()
                    if (foreground() && NewerReleases.due(lookedAtMs, now)) {
                        lookedAtMs = now
                        hear()
                    }
                    delay(NewerReleases.FOREGROUND_LOOKED_FOR_EVERY_MS)
                }
            }
    }

    private suspend fun hear() {
        val newer =
            try {
                library.holding { it.newerRelease() }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                Log.w("Resonate", "Newer releases could not be looked for", failure)
                return
            }
        heard.value = newer
        val shown = NewerReleases.shown(mutableChecks.value, newer, dismissed.value)
        if (shown != null && shown.version != told) {
            told = shown.version
            library.notice(R.string.updates_newer_is_out_notice, shown.version)
        }
    }
}
