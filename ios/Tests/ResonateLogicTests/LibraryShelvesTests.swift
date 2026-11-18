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
        #expect(Set(every.map(\.searchPrompt)).count == every.count)
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
        let dated = AlbumHit(key: "k", title: "Meddle", artist: "Pink Floyd", year: 1971, songs: 6, durationMs: 0, artworkTrack: 0)
        let undated = AlbumHit(key: "k", title: "Meddle", artist: "", year: nil, songs: 6, durationMs: 0, artworkTrack: 0)
        let gathered = AlbumHit(
            key: "k", title: "Hits", artist: "Someone", year: 1999, songs: 6, durationMs: 0, artworkTrack: 0, variousArtists: true
        )

        #expect(dated.byline == "Pink Floyd · 1971")
        #expect(undated.byline == "Unknown artist")
        #expect(gathered.byline == "Various artists · 1999")
    }
}
