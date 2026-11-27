import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct StorageTests {
    private func sized(_ id: UInt64, _ bytes: UInt64, codec: String = "FLAC") -> SizedSong {
        SizedSong(track: track(id, codec: codec), bytes: bytes)
    }

    private func page(_ songs: [SizedSong], full: Bool) -> [SizedSong] {
        let filler = (0..<(full ? Int(StoragePages.pageRows) - songs.count : 0)).map { offset in
            sized(10_000 + UInt64(offset), 1)
        }
        return songs + filler
    }

    private func sizes(_ bytes: UInt64) -> String {
        "\(bytes) B"
    }

    @Test func aPageIsKeptLargestFirstWithTiesByIdAndNamesWhereTheNextBegins() {
        var pages = StoragePages()

        pages.took(page([sized(3, 500), sized(1, 900), sized(2, 500)], full: true))

        #expect(pages.songs.prefix(3).map(\.track.id) == [1, 2, 3])
        #expect(!pages.ended)
        #expect(pages.after == SizedSongAfter(bytes: 1, id: 10_000 + UInt64(StoragePages.pageRows) - 4))
    }

    @Test func aShortPageEndsTheListAndASongAskedAgainIsListedOnce() {
        var pages = StoragePages()

        pages.took([sized(1, 900), sized(2, 800)])
        pages.took([sized(2, 800), sized(3, 100)])

        #expect(pages.ended)
        #expect(pages.songs.map(\.track.id) == [1, 2, 3])
        #expect(pages.listedBytes == 1_800)
    }

    @Test func anEmptyFirstPageEndsTheList() {
        var pages = StoragePages()

        pages.took([])

        #expect(pages.ended)
        #expect(pages.songs.isEmpty)
        #expect(pages.after == nil)
    }

    @Test func aSongRecompressedMovesDownAmongTheSongsListed() {
        var pages = StoragePages()
        pages.took([sized(1, 900), sized(2, 800), sized(3, 100)])

        pages.resized(track(1, title: "Smaller"), bytes: 500)

        #expect(pages.songs.map(\.track.id) == [2, 1, 3])
        #expect(pages.songs[1].track.title == "Smaller")
        #expect(pages.songs[1].bytes == 500)
    }

    @Test func aSongRecompressedPastThePagesReadWaitsForTheirPage() {
        var pages = StoragePages()
        pages.took(page([sized(1, 900), sized(2, 800)], full: true))

        pages.resized(track(1), bytes: 0)

        #expect(!pages.songs.contains { $0.track.id == 1 })
        #expect(pages.songs.first?.track.id == 2)
    }

    @Test func aSongNotListedIsLeftAloneAndASongGoneIsDropped() {
        var pages = StoragePages()
        pages.took([sized(1, 900), sized(2, 800)])

        pages.resized(track(7), bytes: 50)
        pages.retain { $0 != 1 }

        #expect(pages.songs.map(\.track.id) == [2])
    }

    @Test func onlyAFlacIsOfferedToBeRecompressed() {
        #expect(StorageRows.recompressible(track(1, codec: "FLAC")))
        #expect(StorageRows.recompressible(track(1, codec: "flac")))
        #expect(!StorageRows.recompressible(track(1, codec: "MP3")))
        #expect(!StorageRows.recompressible(track(1, codec: "ALAC")))
    }

    @Test func aRowSaysItsArtistSizeAndFormat() {
        let song = SizedSong(track: track(1, artist: "Kestrel", codec: "FLAC", rate: 96_000, bits: 24), bytes: 42)

        #expect(StorageRows.line(song, size: sizes) == "Kestrel · 42 B · FLAC · 24-bit · 96 kHz")
    }

    @Test func theCeilingIsKeptByNameAndAnUnknownOneIsLossless() {
        #expect(RecompressTo.key == "recompress-to")
        #expect(RecompressTo.kept(nil) == .lossless)
        #expect(RecompressTo.kept("nonsense") == .lossless)
        #expect(RecompressTo.kept(RecompressTo.cd.rawValue) == .cd)
        #expect(RecompressTo.allCases.map(\.ceiling) == [.kept, .studio, .high, .cd])
        #expect(RecompressTo.allCases.filter(\.lowersTheResolution) == [.studio, .high, .cd])
    }

    @Test func theCeilingKeptInTheSettingsIsReadBack() throws {
        let suite = "storage-tests-\(UUID().uuidString)"
        let settings = try #require(UserDefaults(suiteName: suite))
        defer { settings.removePersistentDomain(forName: suite) }

        settings.set(RecompressTo.high.rawValue, forKey: RecompressTo.key)

        #expect(RecompressTo.kept(settings.string(forKey: RecompressTo.key)) == .high)
    }

    @Test func aRunIsToldPartByPart() {
        var tally = RecompressTally()

        tally.add(.shrunk(before: 1_000, after: 600))
        tally.add(.shrunk(before: 500, after: 400))
        tally.add(.kept("Kept as it was, since it is no smaller written again"))
        tally.add(.failed("The file is gone"))
        tally.add(.failed("The folder refused"))

        #expect(tally.done == 5)
        #expect(tally.saved == 500)
        #expect(tally.firstFailure == "The file is gone")
        #expect(
            tally.told(stopped: false, size: sizes)
                == "2 songs re-compressed, 500 B saved; 1 song kept as it was; 2 songs could not be re-compressed")
    }

    @Test func aRunStoppedOrDoingNothingSaysSo() {
        var tally = RecompressTally()

        #expect(tally.told(stopped: false, size: sizes) == "Nothing was re-compressed")

        tally.add(.shrunk(before: 10, after: 4))

        #expect(tally.told(stopped: true, size: sizes) == "Stopped: 1 song re-compressed, 6 B saved")
    }

    @Test func oneSongIsToldByWhatBecameOfIt() {
        #expect(RecompressOutcome.shrunk(before: 9, after: 5).told(title: "Lantern", size: sizes) == "“Lantern” re-compressed from 9 B to 5 B")
        #expect(RecompressOutcome.kept("Kept as it was").told(title: "Lantern", size: sizes) == "“Lantern”: Kept as it was")
        #expect(RecompressOutcome.failed("Gone").told(title: "Lantern", size: sizes) == "“Lantern” was not re-compressed: Gone")
    }

    @Test func storageIsAPageOfTheLibraryGroupFoundByItsWords() {
        let found = SettingsSearch.matching("largest").first

        #expect(SettingsPlace.storage.group == .library)
        #expect(SettingsPlace.storage.opening == .page(.storage))
        #expect(SettingsPlace.storage.title == "Manage storage")
        #expect(found?.place == .storage)
        #expect(SettingsSearch.matching("re-compress").first?.place == .storage)
        #expect(Destination.storage.belongsToSettings)
    }
}
