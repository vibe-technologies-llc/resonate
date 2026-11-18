import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct HigherVersionRowsTests {
    private let held = FoundVersion(version: .held(id: 9), label: "Play the FLAC 24-bit / 96 kHz copy")

    private let served = FoundVersion(version: .served(uri: "tidal:track/7"), label: "Download the FLAC version from TIDAL")

    private func higher(_ id: UInt64, _ found: FoundVersion? = nil, chosen: Bool = false, pending: Bool = false) -> HigherVersion {
        HigherVersion(song: track(id, codec: "MP3"), found: found, chosen: chosen, pending: pending)
    }

    @Test func onlyAVersionFoundAndNotYetChosenIsTickedAndPicked() {
        let songs = [
            higher(1, held),
            higher(2, served),
            higher(3, held, chosen: true),
            higher(4, pending: true),
            higher(5),
        ]

        #expect(songs.map(HigherVersionRows.standing) == [.found, .found, .chosen, .waiting, .none])
        #expect(HigherVersionRows.ticked(songs) == [1, 2])
        #expect(HigherVersionRows.stillTicked(songs, [2, 3, 4]) == [2])
        #expect(HigherVersionRows.picked(songs, [2, 3, 5]) == [HigherVersionPicked(song: 2, version: served.version)])
    }

    @Test func eachRowSaysWhatWasFound() {
        #expect(HigherVersionRows.line(higher(1, held)) == "MP3; Play the FLAC 24-bit / 96 kHz copy")
        #expect(HigherVersionRows.line(higher(1, held, chosen: true)) == "MP3; already plays Play the FLAC 24-bit / 96 kHz copy")
        #expect(HigherVersionRows.line(higher(3, pending: true)) == "MP3; waiting for its version to download")
        #expect(HigherVersionRows.line(higher(4)) == "MP3; no higher quality version held or on your services")
    }

    @Test func whatWasKeptIsCountedPartByPart() {
        #expect(
            HigherVersionRows.told(HigherKept(chosen: 2, downloading: 1, passedOver: 0))
                == "2 songs play their higher quality versions from now on; 1 version is downloaded first")
        #expect(HigherVersionRows.told(HigherKept(chosen: 0, downloading: 0, passedOver: 3)) == "3 versions could no longer be kept")
        #expect(HigherVersionRows.told(HigherKept(chosen: 0, downloading: 0, passedOver: 0)) == "Nothing was kept")
    }
}
