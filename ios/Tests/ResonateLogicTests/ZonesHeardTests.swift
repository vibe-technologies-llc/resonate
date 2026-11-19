import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct ZonesHeardTests {
    @Test func aZoneSoundingUnseenHoldsTheSessionWhileTheZoneShownIsStoppedOrEmpty() {
        let shownStopped = ZonesHeard.sounds(.stopped, .playing)
        let shownEmpty = ZonesHeard.sounds(nil, .loading)
        let shownPlaying = ZonesHeard.sounds(.playing, nil)
        let nothingSounds = ZonesHeard.sounds(.paused, nil)

        #expect(shownStopped)
        #expect(shownEmpty)
        #expect(shownPlaying)
        #expect(!nothingSounds)
    }

    @Test func theSessionSpeaksForTheZoneHeardOnlyWhileTheZoneShownIsSilent() {
        let shownSilent = ZonesHeard.speaksFor(shown: .stopped, elsewhere: .playing, elsewhereNamesASong: true)
        let shownPlaying = ZonesHeard.speaksFor(shown: .playing, elsewhere: .playing, elsewhereNamesASong: true)
        let heardNamesNoSong = ZonesHeard.speaksFor(shown: .paused, elsewhere: .loading, elsewhereNamesASong: false)
        let nothingElsewhere = ZonesHeard.speaksFor(shown: .paused, elsewhere: nil, elsewhereNamesASong: false)

        #expect(shownSilent == .elsewhere)
        #expect(shownPlaying == .shown)
        #expect(heardNamesNoSong == .shown)
        #expect(nothingElsewhere == .shown)
    }

    @Test func aSnapshotOfAnEmptyZoneShownSoundsWhileAnotherPlays() {
        let unseen = snapshot(.stopped, heardElsewhere: soundingElsewhere())
        let silent = snapshot(.paused, current: track(1))

        #expect(unseen.anyZoneSounds)
        #expect(unseen.spokenElsewhere?.zone == 2)
        #expect(!silent.anyZoneSounds)
        #expect(silent.spokenElsewhere == nil)
    }

    @Test func theLockScreenShowsTheSongOfTheZoneHeardWhileTheZoneShownIsSilent() {
        let shownStopped = snapshot(
            .stopped,
            current: track(1),
            positionMs: 5_000,
            durationMs: 100_000,
            heardElsewhere: soundingElsewhere(.playing, in: 3, current: track(9), positionMs: 42_000, durationMs: 180_000)
        )

        let spoken = NowPlayingSpoken(shownStopped)

        #expect(spoken?.track.id == 9)
        #expect(spoken?.transport == .playing)
        #expect(spoken?.positionMs == 42_000)
        #expect(spoken?.durationMs == 180_000)
        #expect(spoken?.zoneHeard == 3)
    }

    @Test func theLockScreenShowsTheZoneShownWhileItSoundsOrTheOneHeardNamesNoSong() {
        let shownPlaying = snapshot(.playing, current: track(1), positionMs: 5_000, heardElsewhere: soundingElsewhere())
        let heardNamesNoSong = snapshot(.paused, current: track(1), heardElsewhere: soundingElsewhere(.loading, current: nil))

        let playing = NowPlayingSpoken(shownPlaying)
        let paused = NowPlayingSpoken(heardNamesNoSong)

        #expect(playing?.track.id == 1)
        #expect(playing?.positionMs == 5_000)
        #expect(playing?.zoneHeard == nil)
        #expect(paused?.track.id == 1)
        #expect(paused?.transport == .paused)
        #expect(paused?.zoneHeard == nil)
    }

    @Test func nothingIsShownWhereTheZoneShownIsEmptyAndNoOtherSounds() {
        #expect(NowPlayingSpoken(snapshot(.stopped)) == nil)
        #expect(NowPlayingSpoken(snapshot(.stopped, heardElsewhere: soundingElsewhere(current: nil))) == nil)
    }

    @Test func pausingFromTheLockScreenPausesEveryZoneThatSoundsWhileOneUnseenIsShown() {
        #expect(ZonesHeard.answering(.pause, speaksFor: .elsewhere) == .pauseSounding)
        #expect(ZonesHeard.answering(.toggle, speaksFor: .elsewhere) == .pauseSounding)
        #expect(ZonesHeard.answering(.play, speaksFor: .elsewhere) == .nothing)
    }

    @Test func theLockScreenSteersTheZoneShownWhileItSpeaksForIt() {
        #expect(ZonesHeard.answering(.play, speaksFor: .shown) == .resumeShown)
        #expect(ZonesHeard.answering(.pause, speaksFor: .shown) == .pauseShown)
        #expect(ZonesHeard.answering(.toggle, speaksFor: .shown) == .toggleShown)
    }
}
