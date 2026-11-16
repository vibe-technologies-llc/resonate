import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct LossySongsTests {
    private func lossy(_ id: UInt64, lossless: Track? = nil, wantable: Bool = false, pending: Bool = false) -> LossySong {
        LossySong(song: track(id, codec: "MP3"), lossless: lossless, wantable: wantable, pending: pending)
    }

    @Test func aSongIsTickedWhereItsCopyIsHeldOrCanBeWanted() {
        let songs = [
            lossy(1, lossless: track(9), wantable: true, pending: true),
            lossy(2, wantable: true),
            lossy(3, wantable: true, pending: true),
            lossy(4),
        ]

        #expect(songs.map(LossySongs.standing) == [.held, .wantable, .waiting, .unmatched])
        #expect(LossySongs.ticked(songs) == [1, 2])
        #expect(LossySongs.stillTicked(songs, [2, 3, 4]) == [2])
        #expect(LossySongs.deletes(songs, [1, 2]) == 1)
    }

    @Test func eachRowSaysWhatReplacingItWouldDo() {
        #expect(LossySongs.line(lossy(1, lossless: track(9))) == "MP3; a lossless FLAC copy is held")
        #expect(LossySongs.line(lossy(2, wantable: true)) == "MP3; a lossless copy is downloaded first")
        #expect(LossySongs.line(lossy(3, wantable: true, pending: true)) == "MP3; waiting for a lossless copy to download")
        #expect(LossySongs.line(lossy(4)) == "MP3; not matched on MusicBrainz, so no lossless copy can be asked for")
    }

    @Test func whatWasDoneIsCountedPartByPart() {
        #expect(
            LossySongs.told(LossyReplaced(replaced: 2, wanted: 1, unmatched: 0, kept: 1))
                == "Replaced 2 songs with their lossless copies; 1 lossless copy wanted; 1 file could not be deleted; its folder would not let go of it")
        #expect(LossySongs.told(LossyReplaced(replaced: 0, wanted: 0, unmatched: 3, kept: 0)) == "3 songs not matched, so nothing could be asked for")
        #expect(LossySongs.told(LossyReplaced(replaced: 0, wanted: 0, unmatched: 0, kept: 0)) == "Nothing was replaced")
    }
}
