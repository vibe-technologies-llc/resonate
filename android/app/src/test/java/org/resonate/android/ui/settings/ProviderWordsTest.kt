package org.resonate.android.ui.settings

import org.junit.Assert.assertEquals
import org.junit.Test
import org.resonate.android.emptyAccount
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider

class ProviderWordsTest {
    @Test
    fun anAccountIsSummarisedByWhereItIsAskedAndWhetherItIsReady() {
        val subsonic =
            DownloadAccount(MusicProvider.SUBSONIC, " https://music.example/ ", "me", "pw", "", "", "")
        val hifi = emptyAccount(MusicProvider.HIFI_API).copy(password = "key")

        assertEquals("music.example · Ready", accountSummary(subsonic, true))
        assertEquals("music.example · Off", accountSummary(subsonic, false))
        assertEquals(
            "Hosted service · Ready",
            accountSummary(emptyAccount(MusicProvider.MONOCHROME), true),
        )
        assertEquals(
            "No server yet · Needs a server address",
            accountSummary(emptyAccount(MusicProvider.SUBSONIC), true),
        )
        assertEquals("Your own server · Ready", accountSummary(hifi, true))
        assertEquals(
            "Your subscription · Needs a TIDAL client ID",
            accountSummary(emptyAccount(MusicProvider.TIDAL), true),
        )
    }

    @Test
    fun theLeadSaysHowManySourcesAreAsked() {
        assertEquals("No source is set up yet.", accountsLead(0))
        assertEquals("One source is asked for each song.", accountsLead(1))
        assertEquals(
            "All 3 sources are asked at once for each song, and the first to deliver wins.",
            accountsLead(3),
        )
    }
}
