package org.resonate.android.ui.settings

import org.junit.Assert.assertEquals
import org.junit.Test
import org.resonate.android.English
import org.resonate.android.emptyAccount
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider

class ProviderWordsTest {
    @Test
    fun anAccountIsSummarisedByWhereItIsAskedAndWhetherItIsReady() {
        val subsonic =
            DownloadAccount(MusicProvider.SUBSONIC, " https://music.example/ ", "me", "pw", "", "", "")
        val hifi = emptyAccount(MusicProvider.HIFI_API).copy(password = "key")

        assertEquals("music.example · Ready", accountSummary(English, subsonic, true))
        assertEquals("music.example · Off", accountSummary(English, subsonic, false))
        assertEquals(
            "Hosted service · Ready",
            accountSummary(English, emptyAccount(MusicProvider.MONOCHROME), true),
        )
        assertEquals(
            "No server yet · Needs a server address",
            accountSummary(English, emptyAccount(MusicProvider.SUBSONIC), true),
        )
        assertEquals("Your own server · Ready", accountSummary(English, hifi, true))
        assertEquals(
            "jelly.example · Ready",
            accountSummary(
                English,
                DownloadAccount(
                    MusicProvider.JELLYFIN,
                    "https://jelly.example/",
                    "me",
                    "pw",
                    "",
                    "",
                    "",
                ),
                true,
            ),
        )
        assertEquals(
            "No server yet · Needs a server address",
            accountSummary(English, emptyAccount(MusicProvider.JELLYFIN), true),
        )
        assertEquals(
            "Your subscription · Needs a TIDAL client ID",
            accountSummary(English, emptyAccount(MusicProvider.TIDAL), true),
        )
    }

    @Test
    fun theLeadSaysHowManySourcesAreAsked() {
        assertEquals("No source is set up yet.", accountsLead(English, 0))
        assertEquals("One source is asked for each song.", accountsLead(English, 1))
        assertEquals(
            "All 3 sources are asked at once for each song, and the first to deliver wins.",
            accountsLead(English, 3),
        )
    }
}
