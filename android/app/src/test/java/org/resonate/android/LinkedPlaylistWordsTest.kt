package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.PlaylistLinkInfo
import org.resonate.android.nativecore.RefreshedPlaylist

class LinkedPlaylistWordsTest {
    private fun refreshed(added: UInt = 0u, removed: UInt = 0u, missing: UInt = 0u, wanted: UInt = 0u) =
        RefreshedPlaylist(id = 7uL, added = added, removed = removed, missing = missing, wanted = wanted)

    @Test
    fun anImportOnceSaysWhatItMatchedAndOneKeptInStepWhatItSavedForLater() {
        assertEquals(
            "Imported 3 local songs; 2 still missing",
            LinkedPlaylistWords.imported(English, "Night drive", "Spotify", false, 3u, 2u),
        )
        assertEquals(
            "Keeping “Night drive” in step with Spotify · 3 songs here, 2 saved for later",
            LinkedPlaylistWords.imported(English, "Night drive", "Spotify", true, 3u, 2u),
        )
        assertEquals(
            "Keeping “Night drive” in step with Deezer · 1 song here",
            LinkedPlaylistWords.imported(English, "Night drive", "Deezer", true, 1u, 0u),
        )
    }

    @Test
    fun theLineUnderTheHeaderSaysHowThePlaylistFollowsItsLink() {
        val link = PlaylistLinkInfo(page = "https://x", service = "Spotify", keptInStep = false, refreshed = 0uL)

        assertEquals("Imported from Spotify", LinkedPlaylistWords.line(English, link))
        assertEquals("Kept in step with Spotify", LinkedPlaylistWords.line(English, link.copy(keptInStep = true)))
    }

    @Test
    fun aRefreshSaysWhatMovedAndOnlyAMoveCountsAsOne() {
        assertFalse(LinkedPlaylistWords.moved(refreshed(missing = 4u)))
        assertTrue(LinkedPlaylistWords.moved(refreshed(wanted = 1u)))
        assertEquals(
            "Nothing new for “Night drive” on Spotify",
            LinkedPlaylistWords.refreshed(English, "Night drive", "Spotify", refreshed(missing = 4u)),
        )
        assertEquals(
            "“Night drive”: 1 song added · 2 taken out · 3 saved for later",
            LinkedPlaylistWords.refreshed(
                English,
                "Night drive",
                "Spotify",
                refreshed(added = 1u, removed = 2u, wanted = 3u),
            ),
        )
    }
}
