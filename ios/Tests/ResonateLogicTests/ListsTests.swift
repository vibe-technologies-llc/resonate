import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct ShownNamesTests {
    @Test func aSongWithoutATitleIsShownByItsFileName() {
        #expect(track(1, title: "", name: "01 untitled.flac").shownTitle == "01 untitled.flac")
        #expect(track(1, title: "Echoes").shownTitle == "Echoes")
    }

    @Test func aSongWithoutAnArtistIsShownByTheWordsUnknownArtist() {
        #expect(track(1, artist: "").shownArtist == "Unknown artist")
        #expect(track(1, artist: "Björk").shownArtist == "Björk")
    }

    @Test func aSongIsGroupedUnderItsAlbumArtistWhereItHasOne() {
        #expect(track(1, artist: "Guest", albumArtist: "Host").groupingArtist == "Host")
        #expect(track(1, artist: "Guest", albumArtist: "").groupingArtist == "Guest")
    }

    @Test func aSongsQualityListsItsCodecDepthAndRateInKilohertz() {
        #expect(track(1).quality == "FLAC · 16-bit · 44.1 kHz")
        #expect(track(1, codec: "MP3", rate: 48_000, bits: nil).quality == "MP3 · 48 kHz")
        #expect(track(1, codec: "", rate: 0, bits: nil).quality == "")
        #expect(track(1, rate: 96_000, bits: 24).quality == "FLAC · 24-bit · 96 kHz")
    }

    @Test func anAlbumOrArtistWithNoNameIsShownByItsUnknownWords() {
        let album = AlbumHit(key: "k", title: "", artist: "", year: nil, songs: 1, durationMs: 0, artworkTrack: 0)
        let artist = ArtistHit(name: "", songs: 1, albums: 0, artworkTrack: 0)

        #expect(album.shownTitle == "Unknown album")
        #expect(album.shownArtist == "Unknown artist")
        #expect(artist.shownName == "Unknown artist")
    }

    @Test func anAlbumsDetailJoinsItsArtistYearAndSongCountSkippingAnEmptyYear() {
        let dated = AlbumHit(key: "k", title: "Meddle", artist: "Pink Floyd", year: 1971, songs: 6, durationMs: 0, artworkTrack: 0)
        let undated = AlbumHit(key: "k", title: "Meddle", artist: "Pink Floyd", year: nil, songs: 1, durationMs: 0, artworkTrack: 0)

        #expect(dated.detail == "Pink Floyd · 1971 · 6 songs")
        #expect(undated.detail == "Pink Floyd · 1 song")
    }

    @Test func anArtistsDetailLeavesOutAlbumsItHasNone() {
        #expect(ArtistHit(name: "A", songs: 12, albums: 3, artworkTrack: 0).detail == "3 albums · 12 songs")
        #expect(ArtistHit(name: "A", songs: 1, albums: 0, artworkTrack: 0).detail == "1 song")
    }
}

@Suite struct PlaylistEntryTests {
    @Test func aSongRepeatedInAPlaylistIsNumberedByOccurrenceSoEveryRowHasItsOwnKey() {
        let entries = PlaylistEntry.of([track(1), track(2), track(1), track(1)])

        #expect(entries.map(\.position) == [0, 1, 2, 3])
        #expect(entries.map(\.id.occurrence) == [0, 0, 1, 2])
        #expect(Set(entries.map(\.id)).count == 4)
    }

    @Test func aKeyStaysWithItsSongWhenOthersMoveAroundIt() {
        let before = PlaylistEntry.of([track(1), track(2), track(1)])
        let after = PlaylistEntry.of([track(2), track(1), track(1)])

        #expect(before[2].id == after[2].id)
        #expect(before[0].id == after[1].id)
    }

    @Test func nothingMakesNoEntries() {
        #expect(PlaylistEntry.of([]).isEmpty)
    }
}

@Suite struct SongKeysTests {
    let songs = [
        track(1, title: "Hunter", artist: "Björk", album: "Homogenic"),
        track(2, title: "Echoes", artist: "Pink Floyd", album: "Meddle"),
        track(3, title: "Hey You", artist: "Pink Floyd", album: "The Wall"),
    ]

    @Test func aBlankSearchMatchesEverySong() {
        #expect(SongKeys.of(songs).matching("").map(\.id) == [1, 2, 3])
    }

    @Test func aSearchMatchesTitleArtistOrAlbumFoldedOfAccentsAndCase() {
        let keys = SongKeys.of(songs)

        #expect(keys.matching("bjork".searchFolded).map(\.id) == [1])
        #expect(keys.matching("pink".searchFolded).map(\.id) == [2, 3])
        #expect(keys.matching("the wall".searchFolded).map(\.id) == [3])
        #expect(keys.matching("zzz").isEmpty)
    }

    @Test func aSearchDoesNotRunAcrossTheFieldsItJoins() {
        let keys = SongKeys.of([track(1, title: "Hunter", artist: "Bjork", album: "Homogenic")])

        #expect(keys.matching("hunterbjork").isEmpty)
    }

    @Test func nothingKeyedMatchesNothing() {
        #expect(SongKeys.empty.matching("anything").isEmpty)
        #expect(SongKeys.empty.matching("").isEmpty)
    }
}

@Suite struct AlbumDiscTests {
    @Test func tracksAreGroupedByDiscInTheOrderTheyComeAndAMissingDiscIsTheFirst() {
        let discs = AlbumDisc.of([track(1, disc: 1), track(2), track(3, disc: 2), track(4, disc: 2)])

        #expect(discs.map(\.number) == [1, 2])
        #expect(discs[0].tracks.map(\.id) == [1, 2])
        #expect(discs[1].tracks.map(\.id) == [3, 4])
    }

    @Test func aDiscComingBackAfterAnotherStartsAGroupOfItsOwn() {
        let discs = AlbumDisc.of([track(1, disc: 1), track(2, disc: 2), track(3, disc: 1)])

        #expect(discs.map(\.number) == [1, 2, 1])
        #expect(Set(discs.map(\.id)).count == discs.count)
    }

    @Test func nothingMakesNoDiscs() {
        #expect(AlbumDisc.of([]).isEmpty)
    }

    @Test func anAlbumSharesAQualityOnlyWhereEverySongHasOneQuality() {
        #expect(sharedQuality([track(1), track(2)]) == "FLAC · 16-bit · 44.1 kHz")
        #expect(sharedQuality([track(1), track(2, rate: 48_000)]) == nil)
        #expect(sharedQuality([track(1), track(2, codec: "MP3")]) == nil)
        #expect(sharedQuality([track(1), track(2, bits: 24)]) == nil)
        #expect(sharedQuality([]) == nil)
        #expect(sharedQuality([track(1, codec: "", rate: 0, bits: nil)]) == nil)
    }
}

@Suite struct QueuePartsTests {
    @Test func aQueueWithNothingAskedIsHistoryThePlayingRowAndTheRest() {
        let parts = QueueParts(rows: 10, playing: 3, next: nil)

        #expect(parts.history == 0..<3)
        #expect(parts.next == 4..<4)
        #expect(parts.rest == 4..<10)
        #expect(parts.beyond == 0)
    }

    @Test func rowsAskedToPlayNextSitBetweenThePlayingRowAndTheRest() {
        let parts = QueueParts(rows: 10, playing: 3, next: 4..<6)

        #expect(parts.next == 4..<6)
        #expect(parts.rest == 6..<10)
    }

    @Test func aRangeAskedBeforeThePlayingRowOrPastTheEndIsHeldToWhatExists() {
        #expect(QueueParts(rows: 10, playing: 5, next: 0..<2).next == 6..<6)
        #expect(QueueParts(rows: 10, playing: 5, next: 6..<40).next == 6..<10)
        #expect(QueueParts(rows: 10, playing: 5, next: 6..<40).rest == 10..<10)
    }

    @Test func aPlayingRowPastTheEndLeavesNothingAfterIt() {
        let parts = QueueParts(rows: 3, playing: 9, next: nil)

        #expect(parts.history == 0..<3)
        #expect(parts.next == 3..<3)
        #expect(parts.rest == 3..<3)
    }

    @Test func aWindowShowsOnlyHowMuchHistoryAndHowFarAheadItWasGivenAndCountsTheRest() {
        let parts = QueueParts(rows: 1_000, playing: 500, next: nil, heardAtMost: 50, aheadAtMost: 200)

        #expect(parts.history == 450..<500)
        #expect(parts.rest == 501..<701)
        #expect(parts.beyond == 299)
    }

    @Test func aWindowKeepsTheRowsAskedToPlayNextWithinWhatItShows() {
        let parts = QueueParts(rows: 1_000, playing: 10, next: 11..<20, heardAtMost: 50, aheadAtMost: 200)

        #expect(parts.history == 0..<10)
        #expect(parts.next == 11..<20)
        #expect(parts.rest == 20..<211)
        #expect(parts.beyond == 789)
    }

    @Test func aWindowNeverShowsFewerRowsThanWereAskedToPlayNext() {
        let parts = QueueParts(rows: 1_000, playing: 0, next: 1..<400, heardAtMost: 50, aheadAtMost: 200)

        #expect(parts.next == 1..<201)
        #expect(parts.rest == 201..<201)
    }

    @Test func everyRowOfAnyQueueIsInExactlyOnePartOrBeyond() {
        for rows in [0, 1, 7, 100] {
            for playing in [0, 3, 99, 200] {
                for window in [nil, 5] {
                    let parts = QueueParts(rows: rows, playing: playing, next: nil, heardAtMost: window, aheadAtMost: window)
                    let heard = min(playing, rows)
                    let playingRows = min(playing + 1, rows) - heard
                    let hiddenHistory = heard - parts.history.count
                    let accounted = hiddenHistory + parts.history.count + playingRows + parts.next.count + parts.rest.count + parts.beyond
                    #expect(accounted == rows, "\(rows) \(playing) \(String(describing: window))")
                }
            }
        }
    }
}

@Suite struct FilterChipTests {
    @Test func theDefaultFilterHasNoChips() {
        #expect(FilterChip.of(defaultTrackFilter()).isEmpty)
    }

    @Test func eachPartOfAFilterGetsItsChipInOrder() {
        var filter = defaultTrackFilter()
        filter.favourites = true
        filter.unplayed = true
        filter.played = true
        filter.minimumRate = 88_200
        filter.minimumBits = 24
        filter.genre = "Rock"
        filter.artist = "Pink Floyd"
        filter.album = "Meddle"
        filter.order = .plays
        filter.descending = true

        #expect(
            FilterChip.of(filter).map(\.label) == [
                "Favourites", "Unplayed", "Played", "Hi-res", "24-bit or more", "Genre: Rock", "Artist: Pink Floyd",
                "Album: Meddle", "Sorted by plays, descending",
            ]
        )
    }

    @Test func aTitleOrderAscendingIsNoChipButDescendingOrAnotherOrderIs() {
        var filter = defaultTrackFilter()
        filter.descending = true
        #expect(FilterChip.of(filter).map(\.label) == ["Sorted by title, descending"])

        filter.descending = false
        filter.order = .played
        #expect(FilterChip.of(filter).map(\.label) == ["Sorted by last played"])
    }

    @Test func clearingAChipUndoesJustItsPartOfTheFilter() {
        var filter = defaultTrackFilter()
        filter.favourites = true
        filter.genre = "Rock"
        filter.minimumRate = 96_000
        filter.order = .artist
        filter.descending = true

        let cleared = FilterChip.genre("Rock").cleared(from: filter)
        #expect(cleared.genre == "")
        #expect(cleared.favourites && cleared.minimumRate == 96_000)

        let reordered = FilterChip.order(.artist, descending: true).cleared(from: filter)
        #expect(reordered.order == .title)
        #expect(!reordered.descending)
        #expect(reordered.genre == "Rock")
    }

    @Test func clearingEveryChipOfAFilterLeavesTheDefault() {
        var filter = defaultTrackFilter()
        filter.favourites = true
        filter.unplayed = true
        filter.played = true
        filter.minimumRate = 1
        filter.minimumBits = 1
        filter.genre = "g"
        filter.artist = "a"
        filter.album = "b"
        filter.order = .added

        let cleared = FilterChip.of(filter).reduce(filter) { $1.cleared(from: $0) }

        #expect(cleared == defaultTrackFilter())
    }

    @Test func everyOrderHasAName() {
        for order in [TrackOrder.title, .album, .artist, .duration, .added, .plays, .played] {
            #expect(FilterChip.orderNames.contains { $0.0 == order })
        }
    }
}
