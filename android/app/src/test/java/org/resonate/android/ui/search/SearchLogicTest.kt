package org.resonate.android.ui.search

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.AlbumHit
import org.resonate.android.nativecore.ArtistHit
import org.resonate.android.nativecore.LibrarySearch
import org.resonate.android.nativecore.SearchColumn
import org.resonate.android.nativecore.TextRange
import org.resonate.android.nativecore.Track

class SearchLogicTest {
    private fun track(id: ULong, title: String, artist: String, album: String) =
        Track(
            id = id,
            uri = "content://song/$id",
            name = "$id.flac",
            title = title,
            artist = artist,
            album = album,
            durationMs = 180_000u,
            rate = 44_100u,
            codec = "FLAC",
            favourite = false,
            albumArtist = "",
            genre = "",
            date = "",
            trackNumber = null,
            discNumber = null,
            bits = 16u,
            channels = 2u,
            recording = null,
            release = null,
            artistMbid = null,
            artRevision = 0u,
            queueId = null,
        )

    private fun found(
        tracks: List<Track> = emptyList(),
        trackCount: UInt = tracks.size.toUInt(),
        albums: List<AlbumHit> = emptyList(),
        artists: List<ArtistHit> = emptyList(),
    ) = LibrarySearch(tracks, trackCount, albums, artists, emptyList(), null, emptyList())

    @Test
    fun aKeptSearchGoesFirstOnceWhateverItsCase() {
        val recent = listOf("night", "Aurora", "copper sun")

        assertEquals(listOf("aurora", "night", "copper sun"), keptSearch(recent, "  aurora "))
    }

    @Test
    fun onlyTenSearchesAreKept() {
        val recent = (1..10).map { "search $it" }

        val kept = keptSearch(recent, "eleventh")

        assertEquals(RECENT_SEARCHES_KEPT, kept.size)
        assertEquals("eleventh", kept.first())
        assertFalse("search 10" in kept)
    }

    @Test
    fun blankTextAndLinksAreNotKept() {
        val recent = listOf("night")

        assertEquals(recent, keptSearch(recent, "   "))
        assertEquals(recent, keptSearch(recent, "https://tidal.com/browse/track/1"))
    }

    @Test
    fun oneSearchIsForgottenAlone() {
        assertEquals(listOf("a", "c"), forgottenSearch(listOf("a", "b", "c"), "b"))
    }

    @Test
    fun linksAreToldFromWords() {
        assertTrue(isLink("https://open.spotify.com/album/1"))
        assertTrue(isLink(" HTTP://musicbrainz.org/release/1"))
        assertTrue(isLink("spotify:track:1"))
        assertFalse(isLink("http"))
        assertFalse(isLink("copper sun"))
    }

    @Test
    fun completionsOfferOnlyLongerSpellingsOfWhatWasTyped() {
        val offered =
            offeredCompletions(listOf("aurora", "Aurora fields", "aurora fields", "night"), "aurora") {
                it.lowercase()
            }

        assertEquals(listOf("Aurora fields", "aurora fields"), offered)
    }

    @Test
    fun rangesPastTheTextAreClampedAndEmptyOnesDropped() {
        val ranges = litRanges("Aurora", listOf(TextRange(0u, 3u), TextRange(4u, 99u), TextRange(9u, 12u)))

        assertEquals(listOf(0..2, 4..5), ranges)
    }

    @Test
    fun rangesNeverSplitASurrogatePair() {
        val text = "a🎵b"

        assertEquals(listOf(1..2), litRanges(text, listOf(TextRange(2u, 3u))))
        assertEquals(listOf(0..2), litRanges(text, listOf(TextRange(0u, 2u))))
    }

    @Test
    fun touchingRangesMerge() {
        assertEquals(listOf(0..5), litRanges("Copper", listOf(TextRange(3u, 6u), TextRange(0u, 3u))))
    }

    @Test
    fun everyShownNameIsAskedOnceAndBlankOnesNever() {
        val result =
            found(
                tracks = listOf(track(1u, "Night Drive", "Aurora Fields", ""), track(2u, "Dawn", "Aurora Fields", "Lumen")),
                albums = listOf(AlbumHit("k", "Lumen", "Aurora Fields", null, 2u, 0u, 1u)),
                artists = listOf(ArtistHit("Aurora Fields", 2u, 1u, 1u)),
            )

        assertEquals(
            setOf(
                SearchColumn.TITLE to "Night Drive",
                SearchColumn.TITLE to "Dawn",
                SearchColumn.ARTIST to "Aurora Fields",
                SearchColumn.ALBUM to "Lumen",
            ),
            asked(result),
        )
    }

    @Test
    fun litKeepsOnlyNamesWithRanges() {
        val result = found(tracks = listOf(track(1u, "Night Drive", "Aurora Fields", "Lumen")))

        val lit =
            lit("night", result) { _, text, _ ->
                if (text.startsWith("Night")) listOf(TextRange(0u, 5u)) else emptyList()
            }

        assertEquals(listOf(0..4), lit.of(SearchColumn.TITLE, "Night Drive"))
        assertEquals(emptyList<IntRange>(), lit.of(SearchColumn.ARTIST, "Aurora Fields"))
    }

    @Test
    fun songsSayHowManyOfTheMatchesAreListed() {
        val tracks = listOf(track(1u, "a", "b", "c"))

        assertEquals("1 of 812", songsCounted(found(tracks, trackCount = 812u)))
        assertEquals("1", songsCounted(found(tracks)))
        assertEquals("Songs · 1 of 812", scopeLabel(LibraryScope.SONGS, found(tracks, 812u)))
    }

    @Test
    fun aScopeWithNothingInItFallsBackToTop() {
        val result = found(tracks = listOf(track(1u, "a", "b", "c")))

        assertEquals(LibraryScope.TOP, shownScope(LibraryScope.ALBUMS, result))
        assertEquals(LibraryScope.SONGS, shownScope(LibraryScope.SONGS, result))
        assertEquals(listOf(LibraryScope.TOP, LibraryScope.SONGS), scopes(result))
        assertTrue(matchedNothing(found()))
    }
}
