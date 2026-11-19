package org.resonate.android

import org.resonate.android.nativecore.HeardElsewhere
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.Transport

enum class SessionSpeaksFor {
    SHOWN,
    ELSEWHERE,
}

object ZonesHeard {
    fun sounds(shown: Transport?, elsewhere: Transport?): Boolean =
        shown.sounds() || elsewhere.sounds()

    fun speaksFor(shown: Transport?, elsewhere: Transport?, elsewhereNamesASong: Boolean): SessionSpeaksFor =
        if (!shown.sounds() && elsewhere.sounds() && elsewhereNamesASong) SessionSpeaksFor.ELSEWHERE
        else SessionSpeaksFor.SHOWN

    fun soundBegan(before: Boolean, now: Boolean): Boolean = !before && now

    fun fellSilentOfItself(before: Boolean, now: Boolean, awaitingFocus: Boolean): Boolean =
        before && !now && !awaitingFocus
}

internal fun Transport?.sounds() = this == Transport.PLAYING || this == Transport.LOADING

fun Snapshot?.anyZoneSounds(): Boolean = ZonesHeard.sounds(this?.transport, this?.heardElsewhere?.transport)

fun Snapshot?.spokenElsewhere(): HeardElsewhere? {
    val heard = this?.heardElsewhere ?: return null
    val speaksFor = ZonesHeard.speaksFor(transport, heard.transport, heard.current != null)
    return heard.takeIf { speaksFor == SessionSpeaksFor.ELSEWHERE }
}
