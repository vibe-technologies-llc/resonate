package org.resonate.android.ui.home

import java.time.LocalDate
import java.time.ZoneId
import java.time.ZoneOffset
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.English
import org.resonate.android.nativecore.CollectionOrder
import org.resonate.android.nativecore.ListenedOnDay
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.TrackFilter
import org.resonate.android.nativecore.TrackOrder
import org.resonate.android.ui.albumOwner
import org.resonate.android.ui.library.LibraryOpening
import org.resonate.android.ui.navigation.Screen

class JumpBackInTest {
    private fun track(
        id: ULong,
        title: String = "Song $id",
        artist: String = "Artist",
        album: String = "Album",
        albumArtist: String = "",
    ) =
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
            albumArtist = albumArtist,
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

    private val keyOf: (Track) -> String? = { song ->
        if (song.album.isEmpty()) null else "${song.album.lowercase()}|${song.albumOwner.lowercase()}"
    }

    private fun albums(heard: List<Track>, keeping: Int = JumpBackIn.ALBUMS_KEPT) =
        JumpBackIn.albums(heard, keeping, keyOf)

    private val anyFilter =
        TrackFilter(
            text = "",
            genre = "",
            artist = "",
            album = "",
            minimumRate = 0u,
            minimumBits = 0u,
            favourites = false,
            unplayed = false,
            played = false,
            order = TrackOrder.TITLE,
            descending = false,
        )

    @Test
    fun theSongsReadAreThoseHeardLastHeardFirst() {
        val filter = JumpBackIn.heardLately(anyFilter)

        assertTrue(filter.played)
        assertEquals(TrackOrder.PLAYED, filter.order)
        assertTrue(filter.descending)
        assertFalse(filter.favourites)
    }

    @Test
    fun eachAlbumStandsOnceAtTheSongHeardLast() {
        val heard =
            listOf(
                track(1u, title = "One", artist = "Low", album = "Things We Lost"),
                track(2u, title = "Two", artist = "Björk", album = "Vespertine"),
                track(3u, title = "Three", artist = "Low", album = "Things We Lost"),
            )

        val tiles = albums(heard)

        assertEquals(listOf("Things We Lost", "Vespertine"), tiles.map { it.title })
        assertEquals(listOf("Low", "Björk"), tiles.map { it.artist })
        assertEquals(1uL, tiles.first().cover)
    }

    @Test
    fun aGuestOnOneSongDoesNotSplitItsAlbum() {
        val heard =
            listOf(
                track(1u, artist = "Host feat. Guest", album = "Together", albumArtist = "Host"),
                track(2u, artist = "Host", album = "Together", albumArtist = "Host"),
            )

        val tiles = albums(heard)

        assertEquals(1, tiles.size)
        assertEquals("Host", tiles.first().artist)
    }

    @Test
    fun aSongWithNoAlbumIsPassedOver() {
        val tiles = albums(listOf(track(1u, album = ""), track(2u, album = "Kept")))

        assertEquals(listOf("Kept"), tiles.map { it.title })
    }

    @Test
    fun aSongTheKeyRefusesIsPassedOver() {
        val tiles = JumpBackIn.albums(listOf(track(1u, album = "Kept")), keyOf = { null })

        assertTrue(tiles.isEmpty())
    }

    @Test
    fun noMoreAlbumsAreKeptThanAsked() {
        val heard = (1..20).map { track(it.toULong(), album = "Album $it") }

        assertEquals(JumpBackIn.ALBUMS_KEPT, albums(heard).size)
        assertEquals(listOf("Album 1", "Album 2", "Album 3"), albums(heard, keeping = 3).map { it.title })
    }

    @Test
    fun favouritesTakeTheLastPlaceOnlyWhereThereAreSome() {
        val shelf = albums((1..8).map { track(it.toULong(), album = "Album $it") })

        val with = JumpBackIn.shown(shelf, favourites = true, columns = 2)
        val without = JumpBackIn.shown(shelf, favourites = false, columns = 2)

        assertEquals(6, with.size)
        assertEquals(JumpBackTile.Favourites, with.last())
        assertEquals(
            listOf("Album 1", "Album 2", "Album 3", "Album 4", "Album 5"),
            with.dropLast(1).map { it.title(English) },
        )
        assertEquals(6, without.size)
        assertFalse(JumpBackTile.Favourites in without)
    }

    @Test
    fun fewerThanTwoTilesShowNothing() {
        val one = albums(listOf(track(1u, album = "Only")))

        assertTrue(JumpBackIn.shown(emptyList(), favourites = true, columns = 2).isEmpty())
        assertTrue(JumpBackIn.shown(one, favourites = false, columns = 2).isEmpty())
        assertEquals(2, JumpBackIn.shown(one, favourites = true, columns = 2).size)
    }

    @Test
    fun theGridIsTwoAcrossOnAPhoneFourWhenWideAndOneAtLargeText() {
        assertEquals(2, JumpBackGrid.columns(wide = false, largeText = false))
        assertEquals(4, JumpBackGrid.columns(wide = true, largeText = false))
        assertEquals(1, JumpBackGrid.columns(wide = true, largeText = true))
        assertEquals(4, JumpBackGrid.tiles(1))
        assertEquals(6, JumpBackGrid.tiles(2))
        assertEquals(8, JumpBackGrid.tiles(4))
    }

    @Test
    fun aTileOpensItsAlbumOrTheFavouritesAndSpeaksItsAlbumAndArtist() {
        val album = albums(listOf(track(1u, artist = "Low", album = "Trust"))).first()

        assertEquals(Screen.Album("trust|low", "Trust", "Low", 1uL), album.screen)
        assertEquals("Trust, Low", album.spoken(English))
        assertEquals(Screen.Favourites, JumpBackTile.Favourites.screen)
        assertEquals("Favourites", JumpBackTile.Favourites.spoken(English))
    }
}

class HomeMomentTest {
    private fun moment(revision: ULong, day: Int, hour: Int): HomeMoment =
        HomeMoment.of(revision, ZonedDateTime.of(2026, 11, day, hour, 0, 0, 0, ZoneOffset.UTC))

    @Test
    fun theSameRevisionOnTheSameDayIsTheSameMomentWhateverTheHour() {
        assertEquals(moment(4u, day = 5, hour = 9), moment(4u, day = 5, hour = 23))
    }

    @Test
    fun anotherDayOrAnotherRevisionIsAnotherMoment() {
        assertNotEquals(moment(4u, day = 5, hour = 23), moment(4u, day = 6, hour = 0))
        assertNotEquals(moment(4u, day = 5, hour = 9), moment(5u, day = 5, hour = 9))
    }
}

class ListeningGlanceTest {
    private val hourMs = 3_600_000uL
    private val minuteMs = 60_000uL
    private val zone: ZoneId = ZoneOffset.UTC
    private val today: LocalDate = LocalDate.of(2026, 11, 5)
    private val dated = Dated { day, withYear ->
        DateTimeFormatter.ofPattern(if (withYear) "MMM d, yyyy" else "MMM d", Locale.US).format(day)
    }

    private fun startOf(day: LocalDate): ULong = day.atStartOfDay(zone).toEpochSecond().toULong()

    private fun day(back: Long, listenedMs: ULong, plays: ULong = 1u) =
        ListenedOnDay(startOf(today.minusDays(back)), plays, listenedMs)

    private fun glance(
        days: List<ListenedOnDay>,
        firstHeardBack: Long = 400,
        plays: ULong = 120u,
        listenedMs: ULong = 300uL * hourMs,
        songsHeard: ULong = 40u,
        songs: ULong = 100u,
    ): ListeningGlance =
        ListeningGlance.of(
            ListeningGlance.Heard(
                days = days,
                firstHeard = if (days.isEmpty()) null else startOf(today.minusDays(firstHeardBack)),
                plays = plays,
                listenedMs = listenedMs,
                songsHeard = songsHeard,
                songs = songs,
            ),
            English,
            today,
            zone,
            dated,
        )

    private fun ListeningGlance.fact(kind: ListeningGlance.FactKind) = facts.firstOrNull { it.kind == kind }

    @Test
    fun timeListenedReadsBrieflyAndAloud() {
        assertEquals("5 min", ListenedTime.brief(English, 5uL * minuteMs))
        assertEquals("6 h 12 min", ListenedTime.brief(English, 6uL * hourMs + 12uL * minuteMs))
        assertEquals("312 h", ListenedTime.brief(English, 312uL * hourMs))
        assertEquals("1 minute", ListenedTime.spoken(English, minuteMs))
        assertEquals("2 hours", ListenedTime.spoken(English, 2uL * hourMs))
        assertEquals("1 hour 3 minutes", ListenedTime.spoken(English, hourMs + 3uL * minuteMs))
    }

    @Test
    fun aTrendIsTheWeekAgainstTheOneBeforeWithNothingToCompareSaidAsNothing() {
        assertNull(ListeningTrend.of(100u, 0u))
        assertEquals(ListeningTrend.Steady, ListeningTrend.of(103u, 100u))
        assertEquals(ListeningTrend.Up(20), ListeningTrend.of(120u, 100u))
        assertEquals(ListeningTrend.Down(15), ListeningTrend.of(85u, 100u))
        assertEquals(ListeningTrend.Up(100), ListeningTrend.of(200u, 100u))
        assertEquals(ListeningTrend.MoreThanDouble, ListeningTrend.of(201u, 100u))
        assertEquals("up 20% on the week before", ListeningTrend.Up(20).words(English))
    }

    @Test
    fun someoneWhoHasPlayedNothingIsToldHowAPlayCountsOverSevenEmptyDays() {
        val empty = glance(emptyList(), plays = 0u, listenedMs = 0u, songsHeard = 0u)

        assertEquals(ListeningGlance.Standing.NOTHING_HEARD, empty.standing)
        assertEquals("No plays yet", empty.headline)
        assertTrue(empty.detail.contains("half its length"))
        assertEquals(ListeningGlance.DAYS_SHOWN, empty.week.size)
        assertEquals(0uL, empty.tallestMs)
        assertEquals(0uL, empty.averageMs)
        assertTrue(empty.facts.isEmpty())
        assertTrue(empty.spoken.startsWith("No plays yet."))
    }

    @Test
    fun sevenDaysEndingTodayAreDrawnWithGapsFilledAndTodayMarked() {
        val week =
            glance(listOf(day(0, hourMs), day(2, 2uL * hourMs), day(7, hourMs))).week

        assertEquals(7, week.size)
        assertEquals(today, week.last().day)
        assertTrue(week.last().isToday)
        assertEquals(1, week.count { it.isToday })
        assertEquals(listOf(0uL, 0uL, 0uL, 0uL, 2uL * hourMs, 0uL, hourMs), week.map { it.listenedMs })
    }

    @Test
    fun theHeadlineIsTheWeeksTimeWithItsPaceAndTrend() {
        val heard =
            glance(listOf(day(0, 4uL * hourMs), day(3, 3uL * hourMs), day(8, 3uL * hourMs + 30uL * minuteMs)))

        assertEquals(ListeningGlance.Standing.HEARD_THIS_WEEK, heard.standing)
        assertEquals("Last 7 days", heard.eyebrow)
        assertEquals("7 h 0 min", heard.headline)
        assertEquals(hourMs, heard.averageMs)
        assertEquals("1 h 0 min a day · up 100% on the week before", heard.detail)
    }

    @Test
    fun aNewListenersPaceIsOverTheDaysSinceTheFirstPlay() {
        val new = glance(listOf(day(0, 2uL * hourMs), day(1, 2uL * hourMs)), firstHeardBack = 1)

        assertEquals(2uL * hourMs, new.averageMs)
        assertEquals("2 h 0 min a day", new.detail)
    }

    @Test
    fun aRunEndsTodayOrYesterdayAndOnlyTwoOrMoreDaysAreShown() {
        val kept = glance(listOf(day(0, 1u), day(1, 1u), day(2, 1u), day(4, 1u)))
        val yesterdays = glance(listOf(day(1, hourMs), day(2, hourMs)))
        val broken = glance(listOf(day(0, hourMs), day(2, hourMs)))

        assertEquals("3 days", kept.fact(ListeningGlance.FactKind.RUN)?.value)
        assertEquals("2 days in a row", yesterdays.fact(ListeningGlance.FactKind.RUN)?.spoken)
        assertNull(broken.fact(ListeningGlance.FactKind.RUN))
    }

    @Test
    fun theWeekdayHeardMostWaitsForAWeekOfHeardDays() {
        val few = glance(listOf(day(0, hourMs)))
        val mondays = listOf(3L, 10L).map { day(it, 5uL * hourMs) }
        val rest = listOf(0L, 1L, 2L, 4L, 5L).map { day(it, hourMs) }

        val habit = glance(mondays + rest)

        assertNull(few.fact(ListeningGlance.FactKind.WEEKDAY))
        assertEquals("Mondays", habit.fact(ListeningGlance.FactKind.WEEKDAY)?.value)
        assertEquals("Heard most on Mondays", habit.fact(ListeningGlance.FactKind.WEEKDAY)?.spoken)
    }

    @Test
    fun aListenerAwayThisWeekReadsAllTimeAndWhenLastHeardWithNoChart() {
        val away = glance(listOf(day(30, hourMs)))

        assertEquals(ListeningGlance.Standing.RESTING_THIS_WEEK, away.standing)
        assertEquals("All time", away.eyebrow)
        assertEquals("300 h", away.headline)
        assertEquals("Nothing this week · last heard Oct 6", away.detail)
        assertEquals(0uL, away.averageMs)
        assertEquals(listOf(ListeningGlance.FactKind.SONGS_HEARD), away.facts.map { it.kind })
        assertTrue(away.spoken.contains("last heard Oct 6"))
    }

    @Test
    fun aLastHeardDayOfAnotherYearSaysTheYear() {
        val away = glance(listOf(day(320, hourMs)))

        assertEquals("Nothing this week · last heard Dec 20, 2025", away.detail)
    }

    @Test
    fun songsHeardSayTheLibraryUnlessServedSongsPassIt() {
        val held = glance(listOf(day(0, hourMs)))
        val served = glance(listOf(day(0, hourMs)), songsHeard = 150u, songs = 100u)

        assertEquals("40 of 100", held.fact(ListeningGlance.FactKind.SONGS_HEARD)?.value)
        assertEquals("150", served.fact(ListeningGlance.FactKind.SONGS_HEARD)?.value)
    }

    @Test
    fun everyFigureIsSpokenInSentences() {
        val heard = glance(listOf(day(0, 4uL * hourMs), day(1, 3uL * hourMs)))

        assertTrue(heard.spoken, heard.spoken.startsWith("Last 7 days, 7 hours listened. 1 hour a day."))
        assertTrue(heard.spoken.contains("The most was Thursday, 4 hours."))
        assertTrue(heard.spoken.contains("2 days in a row"))
        assertTrue(heard.spoken.contains("300 hours in all, 120 plays"))
        assertTrue(heard.spoken.contains("40 of 100 songs heard"))
    }

    @Test
    fun theWordsSitBesideTheChartOnlyWhenWideAndNotAtLargeText() {
        assertEquals(GlanceArrangement.BESIDE, GlanceArrangement.of(wide = true, largeText = false))
        assertEquals(GlanceArrangement.STACKED, GlanceArrangement.of(wide = true, largeText = true))
        assertEquals(GlanceArrangement.STACKED, GlanceArrangement.of(wide = false, largeText = false))
    }
}

class LibraryOpeningTest {
    @Test
    fun anOpeningSetsOnlyTheOrderOfTheShelfItNames() {
        val albums = LibraryOpening.recentlyAdded
        val artists = LibraryOpening.mostPlayedArtists
        val playlists = LibraryOpening.playlists

        assertEquals(CollectionOrder.ADDED, albums.albumOrder)
        assertNull(albums.artistOrder)
        assertEquals(CollectionOrder.PLAYS, artists.artistOrder)
        assertNull(artists.albumOrder)
        assertNull(playlists.albumOrder)
        assertNull(playlists.artistOrder)
    }
}
