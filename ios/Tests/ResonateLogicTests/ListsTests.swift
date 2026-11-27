import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct ShownNamesTests {
    @Test func aSongWithoutATitleIsShownByItsFileName() {
        #expect(track(1, title: "", name: "01 untitled.flac").shownTitle == "01 untitled.flac")
        #expect(track(1, title: "Slow Water").shownTitle == "Slow Water")
    }

    @Test func aSongWithoutAnArtistIsShownByTheWordsUnknownArtist() {
        #expect(track(1, artist: "").shownArtist == "Unknown artist")
        #expect(track(1, artist: "Sölvi").shownArtist == "Sölvi")
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
        let dated = AlbumHit(key: "k", title: "Low Tide", artist: "Example Band", year: 1971, songs: 6, durationMs: 0, artworkTrack: 0)
        let undated = AlbumHit(key: "k", title: "Low Tide", artist: "Example Band", year: nil, songs: 1, durationMs: 0, artworkTrack: 0)

        #expect(dated.detail == "Example Band · 1971 · 6 songs")
        #expect(undated.detail == "Example Band · 1 song")
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
        track(1, title: "Wavebreak", artist: "Sölvi", album: "Seabrook"),
        track(2, title: "Slow Water", artist: "Example Band", album: "Low Tide"),
        track(3, title: "Small Mercies", artist: "Example Band", album: "The Pier"),
    ]

    @Test func aBlankSearchMatchesEverySong() {
        #expect(SongKeys.of(songs).matching("").map(\.id) == [1, 2, 3])
    }

    @Test func aSearchMatchesTitleArtistOrAlbumFoldedOfAccentsAndCase() {
        let keys = SongKeys.of(songs)

        #expect(keys.matching("solvi".searchFolded).map(\.id) == [1])
        #expect(keys.matching("example".searchFolded).map(\.id) == [2, 3])
        #expect(keys.matching("the pier".searchFolded).map(\.id) == [3])
        #expect(keys.matching("zzz").isEmpty)
    }

    @Test func aSearchDoesNotRunAcrossTheFieldsItJoins() {
        let keys = SongKeys.of([track(1, title: "Wavebreak", artist: "Solvi", album: "Seabrook")])

        #expect(keys.matching("wavebreaksolvi").isEmpty)
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

    private func numbered(_ id: UInt64, _ number: UInt32, disc: UInt32? = nil) -> Track {
        var song = track(id, disc: disc)
        song.trackNumber = number
        return song
    }

    private func notHeld(_ title: String, disc: UInt32 = 1, at position: UInt32) -> SearchSong {
        var song = searchSong(title)
        song.disc = disc
        song.position = position
        return song
    }

    private func rowIds(_ disc: AlbumDisc) -> [String] {
        disc.rows.map { row in
            switch row {
            case .held(let track): "\(track.id)"
            case .notHeld(let song): song.title
            }
        }
    }

    @Test func aSongNotHeldStandsAtItsPlaceAmongTheSongsHeld() {
        let discs = AlbumDisc.of(
            [numbered(1, 2), numbered(2, 4)],
            notHeld: [notHeld("first", at: 1), notHeld("third", at: 3), notHeld("fifth", at: 5)]
        )

        #expect(discs.count == 1)
        #expect(rowIds(discs[0]) == ["first", "1", "third", "2", "fifth"])
        #expect(discs[0].tracks.map(\.id) == [1, 2])
    }

    @Test func aStaleMissingSongDoesNotDuplicateALocalPosition() {
        let discs = AlbumDisc.of([numbered(1, 2)], notHeld: [notHeld("stale", at: 2), notHeld("missing", at: 3), notHeld("missing", at: 3)])
        #expect(rowIds(discs[0]) == ["1", "missing"])
    }

    @Test func aDiscHoldingNoSongYetStandsInItsPlaceWithItsOwnNumber() {
        let discs = AlbumDisc.of(
            [numbered(1, 1, disc: 1), numbered(2, 1, disc: 3)],
            notHeld: [notHeld("second", disc: 2, at: 1), notHeld("second too", disc: 2, at: 2)]
        )

        #expect(discs.map(\.number) == [1, 2, 3])
        #expect(discs.map(\.id) == [0, 1, 2])
        #expect(rowIds(discs[1]) == ["second", "second too"])
    }

    @Test func aSongHeldWithNoNumberStaysFirstAndRowsKeepIdsOfTheirOwn() {
        let discs = AlbumDisc.of([track(1)], notHeld: [notHeld("one", at: 1), notHeld("two", at: 2)])

        #expect(rowIds(discs[0]) == ["1", "one", "two"])
        #expect(Set(discs[0].rows.map(\.id)).count == 3)
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

@Suite struct QueueShapeTests {
    @Test func aQueueIsEmptyOneRowOrSeveral() {
        #expect(QueueShape(count: 0) == .empty)
        #expect(QueueShape(count: 1) == .one)
        #expect(QueueShape(count: 2) == .several)
        #expect(QueueShape(count: 5_000) == .several)
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

@Suite struct RowWindowTests {
    @Test func aWindowOpensOnItsFirstRowsAndWidensByAPage() {
        var window = RowWindow()

        #expect(window.shown(of: 10) == 10)
        #expect(!window.holdsMore(than: 10))
        #expect(window.shown(of: 10_000) == RowWindow.opening)
        #expect(window.holdsMore(than: 10_000))

        window.widen()

        #expect(window.shown(of: 10_000) == RowWindow.opening + RowWindow.widenedBy)
        #expect(!window.holdsMore(than: RowWindow.opening + RowWindow.widenedBy))
    }

    @Test func anUnanchoredWindowStartsAtTheFirstRowAndHoldsNothingEarlier() {
        let window = RowWindow()

        #expect(window.rows(of: 10_000) == 0..<RowWindow.opening)
        #expect(window.rows(of: 10) == 0..<10)
        #expect(window.rows(of: 0) == 0..<0)
        #expect(!window.holdsEarlier)
    }

    @Test func anchoringAtARowItHoldsMovesNothing() {
        var window = RowWindow()

        window.anchor(at: 120, of: 10_000)

        #expect(window == RowWindow())
    }

    @Test func anchoringFarAwayOpensAWindowLeadingIntoThatRow() {
        var window = RowWindow()

        window.widen()
        window.anchor(at: 5_000, of: 10_000)

        #expect(window.rows(of: 10_000) == (5_000 - RowWindow.leadIn)..<(5_000 - RowWindow.leadIn + RowWindow.opening))
        #expect(window.holdsEarlier)
        #expect(window.holdsMore(than: 10_000))
    }

    @Test func anchoringNearTheStartOrPastTheEndIsHeldWithinTheList() {
        var early = RowWindow()
        var late = RowWindow()
        var beyond = RowWindow()

        early.anchor(at: 5_000, of: 10_000)
        early.anchor(at: 40, of: 10_000)
        late.anchor(at: 9_999, of: 10_000)
        beyond.anchor(at: 10_000, of: 10_000)

        #expect(early.rows(of: 10_000) == 0..<RowWindow.opening)
        #expect(late.rows(of: 10_000) == (9_999 - RowWindow.leadIn)..<10_000)
        #expect(!late.holdsMore(than: 10_000))
        #expect(beyond == RowWindow())
    }

    @Test func wideningEarlierGrowsBackTowardsTheFirstRow() {
        var window = RowWindow()

        window.anchor(at: 500, of: 10_000)
        window.widenEarlier()

        #expect(window.rows(of: 10_000) == 100..<(400 + RowWindow.opening))

        window.widenEarlier()

        #expect(window.rows(of: 10_000) == 0..<(400 + RowWindow.opening))
        #expect(!window.holdsEarlier)
    }

    @Test func aListThatShrankBelowTheWindowShowsItsLastRows() {
        var window = RowWindow()

        window.anchor(at: 5_000, of: 10_000)

        #expect(window.rows(of: 1_000) == (1_000 - RowWindow.opening)..<1_000)
        #expect(window.rows(of: 50) == 0..<50)
    }
}

@Suite struct SongOrderingTests {
    @Test func favouritesOpenNewestFirstAsTheSongsPageDoes() {
        #expect(SongOrdering.newestFirst.order == SongBrowsing.defaultFilter.order)
        #expect(SongOrdering.newestFirst.descending == SongBrowsing.defaultFilter.descending)
    }

    @Test func anOrderingAsksForTheFavouritesAloneInThatOrder() {
        let filter = SongOrdering(order: .plays, descending: true).favourites()

        #expect(filter.favourites)
        #expect(filter.order == .plays)
        #expect(filter.descending)
        #expect(filter.text.isEmpty)
        #expect(filter.genre.isEmpty)
    }
}

@Suite struct FilterChipTests {
    @Test func thePartsOfAFilterLeaveItsOrderToTheSortChip() {
        var filter = SongBrowsing.defaultFilter
        filter.favourites = true
        filter.genre = "Rock"
        filter.order = .title
        filter.descending = false

        #expect(FilterChip.parts(of: filter).map(\.label) == FilterChip.of(filter).dropLast().map(\.label))
        #expect(FilterChip.parts(of: filter).count == 2)
        #expect(FilterChip.parts(of: SongBrowsing.defaultFilter).isEmpty)
    }

    @Test func theDefaultFilterHasNoChips() {
        #expect(FilterChip.of(SongBrowsing.defaultFilter).isEmpty)
    }

    @Test func eachPartOfAFilterGetsItsChipInOrder() {
        var filter = SongBrowsing.defaultFilter
        filter.favourites = true
        filter.unplayed = true
        filter.played = true
        filter.minimumRate = 88_200
        filter.minimumBits = 24
        filter.genre = "Rock"
        filter.artist = "Example Band"
        filter.album = "Low Tide"
        filter.order = .plays
        filter.descending = true

        #expect(
            FilterChip.of(filter).map(\.label) == [
                "Favourites", "Unplayed", "Played", "Hi-res", "24-bit or more", "Genre: Rock", "Artist: Example Band",
                "Album: Low Tide", "Sorted by plays, descending",
            ]
        )
    }

    @Test func anOrderOrDirectionUnlikeNewestAddedGetsAChip() {
        var filter = SongBrowsing.defaultFilter
        filter.descending = false
        #expect(FilterChip.of(filter).map(\.label) == ["Sorted by added"])

        filter.order = .title
        #expect(FilterChip.of(filter).map(\.label) == ["Sorted by title"])

        filter.order = .played
        #expect(FilterChip.of(filter).map(\.label) == ["Sorted by last played"])
    }

    @Test func clearingAChipUndoesJustItsPartOfTheFilter() {
        var filter = SongBrowsing.defaultFilter
        filter.favourites = true
        filter.genre = "Rock"
        filter.minimumRate = 96_000
        filter.order = .artist
        filter.descending = true

        let cleared = FilterChip.genre("Rock").cleared(from: filter)
        #expect(cleared.genre == "")
        #expect(cleared.favourites && cleared.minimumRate == 96_000)

        let reordered = FilterChip.order(.artist, descending: true).cleared(from: filter)
        #expect(reordered.order == .added)
        #expect(reordered.descending)
        #expect(reordered.genre == "Rock")
    }

    @Test func clearingEveryChipOfAFilterLeavesTheDefault() {
        var filter = SongBrowsing.defaultFilter
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

        #expect(cleared == SongBrowsing.defaultFilter)
    }

    @Test func everyOrderHasAName() {
        for order in [TrackOrder.title, .album, .artist, .duration, .added, .plays, .played] {
            #expect(FilterChip.orderNames.contains { $0.0 == order })
        }
    }
}

@Suite struct NextPlacementTests {
    @Test func aSongAfterTheAskedRowsMovesToJustPastThem() {
        let placing = NextPlacement(playing: 3, next: 4..<6)

        #expect(placing.takes(9) == 6)
        #expect(placing.takes(1) == 6)
    }

    @Test func withNothingAskedASongMovesToJustAfterThePlayingRow() {
        let placing = NextPlacement(playing: 3, next: 4..<4)

        #expect(placing.takes(8) == 4)
        #expect(placing.takes(0) == 4)
    }

    @Test func theSongsAlreadyThereAndTheOneAtTheEdgeAreNotMoved() {
        let placing = NextPlacement(playing: 3, next: 4..<6)

        #expect(placing.takes(3) == nil)
        #expect(placing.takes(4) == nil)
        #expect(placing.takes(5) == nil)
        #expect(placing.takes(6) == nil)
    }
}

@Suite struct NotInYourLibraryTests {
    private func release(kind: String = "Album", year: UInt32? = 2010) -> ReleaseNotHeld {
        ReleaseNotHeld(group: "g", title: "Lacked", kind: kind, year: year, songs: 0, songsRead: false, covered: false)
    }

    @Test func theHeadingCountsWhatTheLibraryLacks() {
        #expect(NotInYourLibrary.heading(1, .release) == "Not in your library · 1 release")
        #expect(NotInYourLibrary.heading(3, .release) == "Not in your library · 3 releases")
    }

    @Test func onlyTheFirstSongsLackedAreShownUntilAllAreAsked() {
        let songs = (0..<30).map { searchSong("Song \($0)") }

        #expect(NotInYourLibrary.shown(songs, all: false).count == NotInYourLibrary.songsShownFirst)
        #expect(NotInYourLibrary.shown(songs, all: false).first?.title == "Song 0")
        #expect(NotInYourLibrary.shown(songs, all: true).count == 30)
        #expect(NotInYourLibrary.shown([], all: false).isEmpty)
    }

    @Test func aReleaseLackedIsToldByItsKindAndYearWhereItHasThem() {
        #expect(NotInYourLibrary.line(release()) == "Album · 2010")
        #expect(NotInYourLibrary.line(release(year: nil)) == "Album")
        #expect(NotInYourLibrary.line(release(kind: "", year: 1999)) == "1999")
    }
}

@Suite struct PlaylistOrderTests {
    private func playlist(_ id: UInt64, _ name: String, songs: UInt64) -> MusicPlaylist {
        MusicPlaylist(id: id, name: name, songs: songs, revision: 0)
    }

    private var playlists: [MusicPlaylist] {
        [playlist(3, "Alpha", songs: 5), playlist(1, "Beta", songs: 20), playlist(2, "Gamma", songs: 5)]
    }

    @Test func byNameKeepsTheOrderTheCatalogGives() {
        #expect(PlaylistOrder.name.sorted(playlists).map(\.name) == ["Alpha", "Beta", "Gamma"])
    }

    @Test func byAddedPutsTheNewestFirst() {
        #expect(PlaylistOrder.added.sorted(playlists).map(\.name) == ["Alpha", "Gamma", "Beta"])
    }

    @Test func bySongsPutsTheLongestFirstAndKeepsTheOrderOfATie() {
        #expect(PlaylistOrder.songs.sorted(playlists).map(\.name) == ["Beta", "Alpha", "Gamma"])
    }

    @Test func everyOrderNamesItselfAndPicksASymbol() {
        for order in PlaylistOrder.allCases {
            #expect(!order.title.isEmpty)
            #expect(!order.symbol.isEmpty)
        }
    }
}
