import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct DrivingTests {
    @Test func aSettingNeverStoredOrUnreadableIsEverythingAndEachReadsBackAsSpelt() {
        #expect(SyncControls.of(nil) == .everything)
        #expect(SyncControls.of("a later word") == .everything)
        for control in [SyncControl.everything, .transport, .nothing] {
            #expect(SyncControls.of(SyncControls.spelt(control)) == control)
        }
        #expect(SyncControls.spelt(.transport) == "transport")
    }

    @Test func aRowIsCountedFromTheFirstMirroredAndMovesOnlyWithinItsQueue() {
        #expect(DrivenRows.row(3, firstRow: 40) == 43)
        #expect(DrivenRows.moveUp(0) == nil)
        #expect(DrivenRows.moveUp(4) == .move(from: 4, to: 3))
        #expect(DrivenRows.moveDown(4, queueLength: 5) == nil)
        #expect(DrivenRows.moveDown(3, queueLength: 5) == .move(from: 3, to: 4))
    }

    @Test func aFirstRowNearTheTopOfTheRangeSaturatesInsteadOfOverflowing() {
        #expect(DrivenRows.row(3, firstRow: .max - 1) == .max)
        #expect(DrivenRows.row(0, firstRow: .max) == .max)
        #expect(DrivenRows.row(-1, firstRow: 7) == 7)
        #expect(DrivenRows.moveDown(.max, queueLength: .max) == nil)
        #expect(DrivenRows.moveDown(.max - 1, queueLength: .max) == nil)
        #expect(DrivenRows.moveDown(.max - 2, queueLength: .max) == .move(from: .max - 2, to: .max - 1))
        #expect(DrivenRows.moveDown(9, queueLength: 3) == nil)
    }

    @Test func aSeekIsHeldWithinTheSong() {
        #expect(DrivenRows.seek(to: 12.5, lengthMillis: 60_000) == .seek(millis: 12_500))
        #expect(DrivenRows.seek(to: 90, lengthMillis: 60_000) == .seek(millis: 60_000))
        #expect(DrivenRows.seek(to: -1, lengthMillis: 60_000) == .seek(millis: 0))
    }

    @Test func sessionIsAskedOnceWhereSoundBeganWithNoAskOfItsOwn() {
        var session = SessionAsked()

        let paused = session.owed(by: snapshot(.paused))
        let began = session.owed(by: snapshot(.playing))
        let playingOn = session.owed(by: snapshot(.playing))
        let stopped = session.owed(by: snapshot(.stopped))

        #expect(!paused)
        #expect(began)
        #expect(!playingOn)
        #expect(!stopped)

        session.ask()

        let loadingAsked = session.owed(by: snapshot(.loading))
        let playingAsked = session.owed(by: snapshot(.playing))
        let pausedAgain = session.owed(by: snapshot(.paused))
        let beganAgain = session.owed(by: snapshot(.loading))

        #expect(!loadingAsked)
        #expect(!playingAsked)
        #expect(!pausedAgain)
        #expect(beganAgain)
    }

    @Test func soundBegunInAZoneNotShownAsksTheSessionAndShowingAnotherKeepsIt() {
        var session = SessionAsked()

        let beganUnseen = session.owed(by: snapshot(.stopped, heardElsewhere: soundingElsewhere()))
        let showingIt = session.owed(by: snapshot(.playing, zone: 2))
        let showingTheEmptyOne = session.owed(by: snapshot(.stopped, heardElsewhere: soundingElsewhere()))
        let everyZoneSilent = session.owed(by: snapshot(.stopped))
        let beganAgainUnseen = session.owed(by: snapshot(.stopped, heardElsewhere: soundingElsewhere(.loading)))

        #expect(beganUnseen)
        #expect(!showingIt)
        #expect(!showingTheEmptyOne)
        #expect(!everyZoneSilent)
        #expect(beganAgainUnseen)
    }

    @Test func aReadSetsThePlacesApartFromWhatElseItShows() {
        let zone = { (position: UInt64) in
            RemoteZoneState(
                zone: 1, name: "Kitchen", playing: .playing, playingRow: 0, positionMillis: position,
                lengthMillis: 200_000, volumePermille: 500, queueLength: 1, firstRow: 0, queueRevision: 3, rows: []
            )
        }
        let early = DrivenRead(RemoteDevice(standing: .mirrored, zones: [zone(1_000)]))
        let later = DrivenRead(RemoteDevice(standing: .mirrored, zones: [zone(1_500)]))

        #expect(early.still == later.still)
        #expect(early.places == [1: 1_000])
        #expect(later.places == [1: 1_500])
    }
}
