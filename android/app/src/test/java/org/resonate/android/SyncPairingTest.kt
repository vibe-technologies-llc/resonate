package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.SyncControl
import org.resonate.android.nativecore.SyncStanding

class SyncPairingTest {
    private val kept =
        SyncConfig(false, false, "", "KEPT-CODE", "", false, false, false, false, SyncControl.EVERYTHING, "", "", true)

    private val spelled: (String) -> String? = { typed ->
        typed.uppercase().takeIf { it.startsWith("GOOD") }
    }

    @Test
    fun aTypedCodeThatReadsAsNoneLeavesTheKeptGroupAlone() {
        assertEquals(SyncJoining.NotACode, SyncPairing.joining(kept, "a typo", spelled))
        assertEquals(SyncJoining.NotACode, SyncPairing.joining(null, "  ", spelled))
    }

    @Test
    fun aCodeThatReadsIsJoinedInItsOwnSpellingWithSyncTurnedOn() {
        val joining = SyncPairing.joining(kept, "  good-code \n", spelled)
        val joined = (joining as SyncJoining.Joined).into(kept.copy(favourites = true))

        assertEquals(SyncJoining.Joined("GOOD-CODE"), joining)
        assertEquals(kept.copy(key = "GOOD-CODE", enabled = true, favourites = true), joined)
    }

    @Test
    fun aCodeThatReadsBeforeTheSettingsWaitsInItsOwnSpelling() {
        assertEquals(SyncJoining.Waiting("GOOD-CODE"), SyncPairing.joining(null, "good-code", spelled))
    }

    @Test
    fun onlyAStartedSyncRuns() {
        assertFalse(SyncStanding.OFF.runs)
        assertFalse(SyncStanding.FAILED.runs)
        assertTrue(SyncStanding.CONNECTING.runs)
        assertTrue(SyncStanding.NEARBY.runs)
    }
}
