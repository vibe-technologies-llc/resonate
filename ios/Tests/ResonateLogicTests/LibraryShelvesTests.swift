import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct LibraryShelvesTests {
    @Test func theShelvesAreAndroidsFourInItsOrder() {
        let every = LibraryShelf.allCases

        #expect(every == [.songs, .albums, .artists, .playlists])
        #expect(every.map(\.title) == ["Songs", "Albums", "Artists", "Playlists"])
        #expect(Set(every.map(\.symbol)).count == every.count)
    }

    @Test func albumsSortFiveWaysAndArtistsThree() {
        #expect(LibraryShelf.albums.orders == [.alphabetical, .artist, .year, .added, .plays])
        #expect(LibraryShelf.artists.orders == [.alphabetical, .added, .plays])
        #expect(LibraryShelf.songs.orders.isEmpty)
        #expect(LibraryShelf.playlists.orders.isEmpty)
    }

    @Test func anOpeningNamesItsShelfAndTheOrderItAsksFor() {
        #expect(LibraryOpening.songs.shelf == .songs)
        #expect(LibraryOpening.playlists.shelf == .playlists)
        #expect(LibraryOpening.albums(by: .added).shelf == .albums)
        #expect(LibraryOpening.albums(by: .added).albumOrder == .added)
        #expect(LibraryOpening.albums(by: .added).artistOrder == nil)
        #expect(LibraryOpening.artists(by: .plays).artistOrder == .plays)
        #expect(LibraryOpening.artists(by: .plays).albumOrder == nil)
        #expect(LibraryOpening.albums(by: nil).albumOrder == nil)
    }

    @Test func anOrderAShelfDoesNotOfferIsNotAsked() {
        #expect(LibraryOpening.artists(by: .year).artistOrder == nil)
        #expect(LibraryOpening.artists(by: .artist).artistOrder == nil)
    }

    @Test func onlyOpeningTheSongsClearsTheirFilters() {
        #expect(LibraryOpening.songs.clearsFilters)
        #expect(!LibraryOpening.albums(by: nil).clearsFilters)
        #expect(!LibraryOpening.artists(by: nil).clearsFilters)
        #expect(!LibraryOpening.playlists.clearsFilters)
    }

    @Test func albumsAreAGridUnlessRowsWereChosenOrTheTextIsLarge() {
        #expect(AlbumsLayout.shown(asRows: false, accessibilityText: false) == .grid)
        #expect(AlbumsLayout.shown(asRows: true, accessibilityText: false) == .rows)
        #expect(AlbumsLayout.shown(asRows: false, accessibilityText: true) == .rows)
    }

    @Test func anAlbumsBylineIsItsArtistAndYearOrVariousArtists() {
        let dated = AlbumHit(key: "k", title: "Low Tide", artist: "Example Band", year: 1971, songs: 6, durationMs: 0, artworkTrack: 0)
        let undated = AlbumHit(key: "k", title: "Low Tide", artist: "", year: nil, songs: 6, durationMs: 0, artworkTrack: 0)
        let gathered = AlbumHit(
            key: "k", title: "Hits", artist: "Someone", year: 1999, songs: 6, durationMs: 0, artworkTrack: 0, variousArtists: true
        )

        #expect(dated.byline == "Example Band · 1971")
        #expect(undated.byline == "Unknown artist")
        #expect(gathered.byline == "Various artists · 1999")
    }

    @Test func aDraggedCapsuleStaysOnTheRowAndLandsOnTheNearestTab() {
        let row = ShelfRowDrag(width: 400, count: 4, between: 4)

        #expect(row.segment == 97)
        #expect((0..<4).map(row.centre(of:)) == [48.5, 149.5, 250.5, 351.5])
        #expect(row.clamped(10) == 48.5)
        #expect(row.clamped(390) == 351.5)
        #expect(row.clamped(200) == 200)
        #expect(row.nearest(to: -50) == 0)
        #expect(row.nearest(to: 98) == 0)
        #expect(row.nearest(to: 100) == 1)
        #expect(row.nearest(to: 260) == 2)
        #expect(row.nearest(to: 1000) == 3)
    }

    @Test func aRowNotYetMeasuredDragsNowhere() {
        let row = ShelfRowDrag(width: 0, count: 4, between: 4)

        #expect(row.segment == 0)
        #expect(row.nearest(to: 120) == 3)
        #expect(ShelfRowDrag(width: 300, count: 0, between: 4).nearest(to: 120) == 0)
    }

    @Test func theSearchFieldIsHiddenUntilTheListScrollsUp() {
        var reveal = SearchReveal()

        reveal.scrolled(to: 0, within: 2000)
        #expect(!reveal.shown)

        reveal.scrolled(to: 500, within: 2000)
        reveal.scrolled(to: 480, within: 2000)
        #expect(!reveal.shown)

        reveal.scrolled(to: 470, within: 2000)
        #expect(reveal.shown)

        reveal.scrolled(to: 490, within: 2000)
        #expect(reveal.shown)

        reveal.scrolled(to: 500, within: 2000)
        #expect(!reveal.shown)
    }

    @Test func aPullAtTheTopShowsItAndTheBounceBackKeepsIt() {
        var reveal = SearchReveal()

        reveal.scrolled(to: 0, within: 2000)
        reveal.scrolled(to: -30, within: 2000)
        #expect(reveal.shown)

        reveal.scrolled(to: -10, within: 2000)
        reveal.scrolled(to: 0, within: 2000)
        #expect(reveal.shown)
    }

    @Test func aTurnStartsTheCountAgainAndABounceAtTheFootCountsNothing() {
        var reveal = SearchReveal()

        reveal.scrolled(to: 100, within: 1000)
        reveal.scrolled(to: 70, within: 1000)
        #expect(reveal.shown)

        reveal.scrolled(to: 85, within: 1000)
        reveal.scrolled(to: 80, within: 1000)
        reveal.scrolled(to: 95, within: 1000)
        #expect(reveal.shown)

        reveal.scrolled(to: 1000, within: 1000)
        #expect(!reveal.shown)

        reveal.scrolled(to: 1060, within: 1000)
        reveal.scrolled(to: 1000, within: 1000)
        #expect(!reveal.shown)

        reveal.scrolled(to: 900, within: 1000)
        #expect(reveal.shown)
        reveal.startOver()
        #expect(!reveal.shown)
    }

    @Test func atTheTopTheFieldStandsInItsOwnRoomAboveTheRowsAndScrollsAwayWithThem() {
        var reveal = SearchReveal()
        let band = 60.0

        #expect(reveal.raised(within: band) == 0)

        reveal.scrolled(to: 0, within: 2000)
        #expect(reveal.raised(within: band) == 0)

        reveal.scrolled(to: 20, within: 2000)
        #expect(reveal.raised(within: band) == 20)

        reveal.scrolled(to: 45, within: 2000)
        #expect(!reveal.shown)
        #expect(reveal.raised(within: band) == 45)

        reveal.scrolled(to: 400, within: 2000)
        #expect(reveal.raised(within: band) == band)

        reveal.scrolled(to: 0, within: 2000)
        #expect(reveal.raised(within: band) == 0)
    }

    @Test func aFieldShownByScrollingUpStandsWhollyOverTheRows() {
        var reveal = SearchReveal()
        let band = 60.0

        reveal.scrolled(to: 500, within: 2000)
        reveal.scrolled(to: 470, within: 2000)
        #expect(reveal.shown)
        #expect(reveal.raised(within: band) == 0)

        reveal.scrolled(to: 0, within: 2000)
        reveal.scrolled(to: -30, within: 2000)
        #expect(reveal.raised(within: band) == 0)
    }
}
