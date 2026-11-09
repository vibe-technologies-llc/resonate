package org.resonate.android.ui.library

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.English
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.TrackFilter
import org.resonate.android.nativecore.TrackOrder

class LibraryLogicTest {
    private fun song(id: ULong, disc: UInt? = null, number: UInt? = null, durationMs: ULong = 60_000uL) =
        Track(
            id = id,
            uri = "content://test/$id",
            name = "$id.flac",
            title = "Song $id",
            artist = "Artist",
            album = "Album",
            durationMs = durationMs,
            rate = 44_100u,
            codec = "FLAC",
            favourite = false,
            albumArtist = "",
            genre = "",
            date = "",
            trackNumber = number,
            discNumber = disc,
            bits = 16u,
            channels = 2u,
            recording = null,
            release = null,
            artistMbid = null,
            artRevision = 0uL,
            queueId = null,
        )

    private val plain =
        TrackFilter(
            text = "",
            genre = "",
            artist = "",
            album = "",
            minimumRate = 0u,
            minimumBits = 0u,
            favourites = false,
            unplayed = false,
            order = TrackOrder.TITLE,
            descending = false,
        )

    @Test
    fun aPlainFilterHasNoPartsAndNoOrder() {
        assertEquals(emptyList<FilterPart>(), plain.parts())
        assertFalse(plain.ordered)
    }

    @Test
    fun eachChosenNarrowingIsAPartThatCanBeTakenAway() {
        val narrowed =
            plain.copy(genre = "Jazz", minimumRate = 96_000u, minimumBits = 24u, favourites = true, unplayed = true)

        assertEquals(
            listOf(FilterPart.GENRE, FilterPart.RATE, FilterPart.DEPTH, FilterPart.FAVOURITES, FilterPart.UNPLAYED),
            narrowed.parts(),
        )
        assertEquals("96 kHz or more", narrowed.label(English, FilterPart.RATE))
        assertEquals("24 bit or more", narrowed.label(English, FilterPart.DEPTH))
        assertEquals(
            listOf(FilterPart.GENRE, FilterPart.DEPTH, FilterPart.FAVOURITES, FilterPart.UNPLAYED),
            narrowed.without(FilterPart.RATE).parts(),
        )
    }

    @Test
    fun blankTextIsNoPartAndTypedTextIsTrimmed() {
        val typed = plain.copy(artist = "  Air ", album = "   ")

        assertEquals(listOf(FilterPart.ARTIST), typed.parts())
        assertEquals("Air", typed.trimmed().artist)
        assertEquals("", typed.trimmed().album)
    }

    @Test
    fun anOrderOtherThanTitleOrDescendingCounts() {
        assertTrue(plain.copy(order = TrackOrder.ADDED).ordered)
        assertTrue(plain.copy(descending = true).ordered)
        assertEquals(
            "Most played, descending",
            plain.copy(order = TrackOrder.PLAYS, descending = true).orderLabel(English),
        )
    }

    @Test
    fun suggestionsPutNamesBeginningWithTheTextFirstAndSkipAnExactMatch() {
        val names = sequenceOf("The Air Show", "Air", "Airiel", "airiel", "Bob", "")

        assertEquals(listOf("Airiel", "The Air Show"), suggestions(names, "Air"))
        assertEquals(listOf("Air", "Airiel", "Bob", "The Air Show"), suggestions(names, ""))
    }

    @Test
    fun repeatedSongsInAPlaylistGetKeysOfTheirOwn() {
        val rows = playlistEntries(listOf(song(1uL), song(2uL), song(1uL)))

        assertEquals(listOf("1#1", "2#1", "1#2"), rows.map { it.key })
        assertEquals(listOf(0, 1, 2), rows.map { it.position })
    }

    @Test
    fun oneDiscIsOneGroupWithoutAHeading() {
        val discs = discsOf(listOf(song(1uL, disc = 1u), song(2uL, disc = 1u)))

        assertEquals(1, discs.size)
        assertEquals(null, discs.single().number)
    }

    @Test
    fun twoDiscsKeepEachRowsIndexInTheWholeAlbum() {
        val discs = discsOf(listOf(song(1uL, disc = 1u), song(2uL, disc = 2u), song(3uL, disc = 2u)))

        assertEquals(listOf(1u, 2u), discs.map { it.number })
        assertEquals(listOf(1, 2), discs[1].held.map { it.index })
    }

    private fun lacking(title: String, disc: UInt? = 1u, position: UInt? = null) =
        SearchSong(
            recording = "rec-$title",
            title = title,
            artist = "Artist",
            artistId = "",
            album = "Album",
            release = "release",
            releaseGroup = "group",
            disc = disc,
            position = position,
            durationMs = 60_000uL,
            isrcs = emptyList(),
            held = false,
        )

    private fun names(disc: Disc) =
        disc.rows.map { row ->
            when (row) {
                is AlbumRow.Held -> "${row.track.id}"
                is AlbumRow.NotHeld -> row.song.title
            }
        }

    @Test
    fun aSongNotHeldStandsAtItsPlaceAmongTheSongsHeld() {
        val discs =
            discsOf(
                listOf(song(1uL, number = 2u), song(2uL, number = 4u)),
                listOf(lacking("first", position = 1u), lacking("third", position = 3u), lacking("fifth", position = 5u)),
            )

        assertEquals(1, discs.size)
        assertEquals(listOf("first", "1", "third", "2", "fifth"), names(discs.single()))
        assertEquals(listOf(0, 1), discs.single().held.map { it.index })
    }

    @Test
    fun aDiscHoldingNoSongYetStandsInItsOrderWithItsNumber() {
        val discs =
            discsOf(
                listOf(song(1uL, disc = 1u, number = 1u), song(2uL, disc = 3u, number = 1u)),
                listOf(lacking("second", disc = 2u, position = 1u), lacking("second too", disc = 2u, position = 2u)),
            )

        assertEquals(listOf(1u, 2u, 3u), discs.map { it.number })
        assertEquals(listOf("second", "second too"), names(discs[1]))
    }

    @Test
    fun anAlbumHoldingOneDiscWhoseReleaseHasTwoShowsBothHeadings() {
        val discs =
            discsOf(
                listOf(song(1uL, disc = 1u, number = 1u)),
                listOf(lacking("on two", disc = 2u, position = 1u)),
            )

        assertEquals(listOf(1u, 2u), discs.map { it.number })
        assertEquals(listOf("on two"), names(discs[1]))
    }

    @Test
    fun songsNotHeldWithNoDiscLieOnTheFirst() {
        val discs = discsOf(listOf(song(1uL, number = 1u)), listOf(lacking("loose", disc = null, position = 2u)))

        assertEquals(listOf("1", "loose"), names(discs.single()))
    }

    @Test
    fun aTrackWithoutANumberShowsItsPlace() {
        assertEquals("7", trackNumber(song(1uL, number = 7u), 0))
        assertEquals("3", trackNumber(song(1uL), 2))
    }

    @Test
    fun theSummaryCountsSongsAndLength() {
        val tracks = listOf(song(1uL, durationMs = 300_000uL), song(2uL, durationMs = 300_000uL))

        assertEquals("2019 · 2 songs · 10 min", collectionSummary(English, "2019", tracks = tracks))
        assertEquals("0 songs", collectionSummary(English, null, tracks = emptyList()))
    }

    @Test
    fun theGreetingFollowsTheHour() {
        assertEquals("Good morning", greeting(English, 8))
        assertEquals("Good afternoon", greeting(English, 13))
        assertEquals("Good evening", greeting(English, 21))
        assertEquals("Good evening", greeting(English, 2))
    }

    @Test
    fun everyWordMustAppearSomewhere() {
        assertTrue(matchesWords("Amber The Meridian Copper Sun", "meridian amb"))
        assertFalse(matchesWords("Amber The Meridian Copper Sun", "meridian fjord"))
        assertTrue(matchesWords("Anything", "  "))
    }

    @Test
    fun theScrollerMapsSharesToRowsBothWays() {
        assertEquals(0f, scrolledShare(0, 100, 10), 0f)
        assertEquals(1f, scrolledShare(90, 100, 10), 0f)
        assertEquals(0f, scrolledShare(0, 5, 10), 0f)
        assertEquals(99, itemAtShare(1f, 100))
        assertEquals(0, itemAtShare(0f, 0))
    }

    @Test
    fun aLinkIsNamedByItsSite() {
        assertEquals("musicbrainz.org", siteOf("https://musicbrainz.org/artist/1"))
        assertEquals("en.wikipedia.org", siteOf("https://www.en.wikipedia.org/wiki/X"))
    }
}
