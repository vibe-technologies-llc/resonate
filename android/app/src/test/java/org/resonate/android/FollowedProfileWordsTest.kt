package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.FollowedProfile
import org.resonate.android.nativecore.ImportedPlaylist
import org.resonate.android.nativecore.MusicPlaylist

class FollowedProfileWordsTest {
    private fun imported(id: ULong) = ImportedPlaylist(id = id, matched = 3u, missing = 1u)

    private fun followed(
        playlists: List<ImportedPlaylist>,
        letGo: UInt = 0u,
        gone: UInt = 0u,
        unread: UInt = 0u,
        name: String = "Ana",
    ) =
        FollowedProfile(
            name = name,
            service = "Deezer",
            playlists = playlists,
            letGo = letGo,
            gone = gone,
            unread = unread,
        )

    private fun playlist(id: ULong, revision: ULong) =
        MusicPlaylist(
            id = id,
            name = "List $id",
            songs = 3uL,
            revision = revision,
            covers = emptyList(),
            search = null,
            link = null,
        )

    @Test
    fun aProfileFollowedSaysItsPlaylistsAndOnlyTheCountsLeftOutThatAreNotNought() {
        assertEquals(
            "Following Ana on Deezer · 2 playlists",
            FollowedProfileWords.followed(followed(listOf(imported(1uL), imported(2uL)))),
        )
        assertEquals(
            "Following Ana on Deezer · 1 playlist, 1 let go, 2 deleted on Deezer, 3 unreadable",
            FollowedProfileWords.followed(
                followed(listOf(imported(1uL)), letGo = 1u, gone = 2u, unread = 3u)
            ),
        )
        assertEquals(
            "Following this profile on Deezer · 0 playlists",
            FollowedProfileWords.followed(followed(emptyList(), name = "")),
        )
    }

    @Test
    fun aProfileMovedOnceCountsItsPlaylistsAndWhatCouldNotBeRead() {
        assertEquals(
            "Moved 2 playlists of Ana to Resonate",
            FollowedProfileWords.moved(followed(listOf(imported(1uL), imported(2uL)))),
        )
        assertEquals(
            "Moved 1 playlist of Ana to Resonate, 1 unreadable",
            FollowedProfileWords.moved(followed(listOf(imported(1uL)), unread = 1u)),
        )
    }

    @Test
    fun aTimedFollowIsQuietUnlessAPlaylistArrivedChangedOrWentAway() {
        val before = listOf(playlist(1uL, 4uL), playlist(2uL, 7uL), playlist(9uL, 1uL))

        val same = FollowedProfileWords.changes(before, before, followed(listOf(imported(1uL), imported(2uL))))
        val moved =
            FollowedProfileWords.changes(
                before,
                listOf(playlist(1uL, 5uL), playlist(2uL, 7uL), playlist(3uL, 1uL), playlist(9uL, 2uL)),
                followed(listOf(imported(1uL), imported(2uL), imported(3uL)), gone = 1u),
            )

        assertTrue(same.quiet)
        assertEquals(ProfileChanges(added = 1, changed = 1, gone = 1u), moved)
        assertFalse(moved.quiet)
        assertEquals(
            "Ana on Deezer: 1 new playlist · 1 changed · 1 deleted on Deezer",
            FollowedProfileWords.refreshed(followed(emptyList(), gone = 1u), moved),
        )
    }

    @Test
    fun aFollowedProfileIsListedByItsService() {
        assertEquals("Deezer profile · its playlists are kept in step", FollowedProfileWords.line("Deezer"))
    }
}
