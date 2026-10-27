import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

func device(
    _ name: String,
    online: Bool = false,
    nearby: Bool = false,
    thisDevice: Bool = false
) -> SyncedDevice {
    SyncedDevice(
        id: name.lowercased(),
        name: name,
        platform: .linux,
        online: online,
        nearby: nearby,
        thisDevice: thisDevice
    )
}

func transfer(
    _ title: String,
    from: String = "Desktop",
    artist: String = "Pink Floyd",
    received: UInt64 = 0,
    size: UInt64 = 0,
    _ state: SyncTransferState = .waiting
) -> SyncTransfer {
    SyncTransfer(title: title, artist: artist, from: from, received: received, size: size, state: state)
}

func status(
    _ standing: SyncStanding = .connected,
    devices: [SyncedDevice] = [],
    transfers: [SyncTransfer] = [],
    moving: UInt32 = 0,
    settled: Bool = true
) -> SyncStatus {
    SyncStatus(
        standing: standing,
        code: "",
        devices: devices,
        moving: moving,
        downloading: 0,
        waiting: 0,
        owed: 0,
        arrivals: 0,
        settled: settled,
        transfers: transfers
    )
}

func config(enabled: Bool = true, nearbyOnly: Bool = false, server: String = "") -> SyncConfig {
    SyncConfig(
        enabled: enabled,
        nearbyOnly: nearbyOnly,
        server: server,
        key: "k",
        device: "",
        favourites: false,
        playlists: false,
        everything: false,
        meteredAllowed: false
    )
}

@Suite struct FetchingHeadingTests {
    @Test func theHeadingSaysHowManyAreDownloadingAndHowManyWait() {
        #expect(fetchingHeading(downloading: 0, waiting: 0) == "Fetching")
        #expect(fetchingHeading(downloading: 2, waiting: 340) == "Fetching · 2 downloading · 340 waiting")
        #expect(fetchingHeading(downloading: 0, waiting: 3) == "Fetching · 3 waiting")
        #expect(fetchingHeading(downloading: 1, waiting: 0) == "Fetching · 1 downloading")
    }
}

@Suite struct SyncStandingTests {
    let others = [device("This phone", online: true, thisDevice: true), device("Desktop", online: true), device("Tablet", nearby: true)]

    @Test func syncSwitchedOffOrNotConfiguredReadsOff() {
        #expect(syncStanding(status(), config: nil, online: true) == "Off")
        #expect(syncStanding(status(), config: config(enabled: false), online: true) == "Off")
    }

    @Test func withOnlineFeaturesOffItWaitsUnlessKeptToTheNetwork() {
        #expect(syncStanding(status(), config: config(), online: false) == "Waiting for online features")
        #expect(syncStanding(status(.nearby), config: config(nearbyOnly: true), online: false) == "Looking on this network")
    }

    @Test func connectedCountsOtherDevicesOnlineAndNeverThisOne() {
        #expect(syncStanding(status(.connected, devices: others), config: config(), online: true) == "1 device online")
        #expect(syncStanding(status(.connected, devices: [others[0]]), config: config(), online: true) == "No other device online")
        #expect(syncStanding(status(.connected), config: config(), online: true) == "No other device online")
    }

    @Test func nearbyCountsDevicesOnThisNetwork() {
        let two = [device("A", nearby: true), device("B", nearby: true)]

        #expect(syncStanding(status(.nearby, devices: two), config: config(), online: true) == "2 devices on this network")
        #expect(syncStanding(status(.nearby), config: config(), online: true) == "Looking on this network")
    }

    @Test func anUnreachableServerSaysSoWithOrWithoutDevicesNearby() {
        #expect(syncStanding(status(.unreachable), config: config(), online: true) == "Can't reach the server")
        #expect(
            syncStanding(status(.unreachable, devices: [device("A", nearby: true)]), config: config(), online: true)
                == "Server unreachable · 1 device on this network"
        )
    }

    @Test func aRefusalConnectingAndStartingEachHaveTheirWords() {
        #expect(syncStanding(status(.refused), config: config(), online: true) == "The server refused this iPhone")
        #expect(syncStanding(status(.connecting), config: config(), online: true) == "Connecting…")
        #expect(
            syncStanding(status(.connecting, devices: [device("A", nearby: true)]), config: config(), online: true)
                == "Connecting · 1 device on this network"
        )
        #expect(syncStanding(status(.off), config: config(), online: true) == "Starting…")
        #expect(syncStanding(nil, config: config(), online: true) == "Starting…")
    }
}

@Suite struct SyncRouteTests {
    @Test func nearbyOnlyWinsWhateverTheServerSays() {
        #expect(SyncRoute(config(nearbyOnly: true, server: "https://own.example")) == .nearby)
    }

    @Test func aBlankOrSpacedServerIsTheHostedOne() {
        #expect(SyncRoute(config()) == .hosted)
        #expect(SyncRoute(config(server: "   ")) == .hosted)
    }

    @Test func anAddressTypedIsYourOwnServer() {
        #expect(SyncRoute(config(server: "https://own.example")) == .own)
    }
}

@Suite struct SyncTransferKeysTests {
    @Test func aRepeatOfTheSameSongFromTheSameDeviceIsNumberedSoNoTwoRowsShareAKey() {
        let rows = SyncTransferKeys.keyed([transfer("Echoes"), transfer("Echoes"), transfer("Hey You")], as: "active")

        #expect(Set(rows.map(\.key)).count == 3)
        #expect(rows[0].key.hasSuffix("0"))
        #expect(rows[1].key.hasSuffix("1"))
    }

    @Test func aKeyDoesNotChangeWhenAnotherRowArrivesAfterIt() {
        let before = SyncTransferKeys.keyed([transfer("A"), transfer("B")], as: "active").map(\.key)
        let after = SyncTransferKeys.keyed([transfer("A"), transfer("B"), transfer("C")], as: "active").map(\.key)

        #expect(Array(after.prefix(2)) == before)
    }

    @Test func theSameSongInDifferentGroupsOrFromDifferentDevicesKeysApart() {
        let active = SyncTransferKeys.keyed([transfer("Echoes")], as: "active")[0].key
        let finished = SyncTransferKeys.keyed([transfer("Echoes")], as: "finished")[0].key
        let elsewhere = SyncTransferKeys.keyed([transfer("Echoes", from: "Tablet")], as: "active")[0].key

        #expect(active != finished)
        #expect(active != elsewhere)
    }

    @Test func aKeyKeepsItsRowWhateverTheProgress() {
        let early = SyncTransferKeys.keyed([transfer("Echoes", received: 1, size: 10, .moving)], as: "active")[0]
        let later = SyncTransferKeys.keyed([transfer("Echoes", received: 9, size: 10, .moving)], as: "active")[0]

        #expect(early.key == later.key)
    }
}

@Suite struct SyncActivityTests {
    @Test func nothingIsHappeningWhenSyncIsOffOrUnknownOrNothingWasAsked() {
        #expect(SyncActivity.of(nil, landing: 0, asked: true) == nil)
        #expect(SyncActivity.of(status(.off), landing: 3, asked: true) == nil)
        #expect(SyncActivity.of(status(), landing: 0, asked: false) == nil)
    }

    @Test func anAskedSyncWithNoTransfersIsSyncing() {
        #expect(SyncActivity.of(status(), landing: 0, asked: true) == .syncing)
    }

    @Test func fetchingCountsSongsWaitingOrMovingAndThoseBeingWrittenIntoTheFolder() {
        let transfers = [
            transfer("A", received: 50, size: 100, .moving),
            transfer("B", .waiting),
            transfer("C", received: 100, size: 100, .landed),
        ]

        let activity = SyncActivity.of(status(transfers: transfers), landing: 2, asked: false)

        #expect(activity == .fetching(songs: 4, fraction: nil))
    }

    @Test func theFractionIsWhatHasArrivedOfWhatIsKnownOnlyWhereEverySizeIsKnown() {
        let sized = [transfer("A", received: 50, size: 100, .moving), transfer("B", received: 30, size: 100, .moving)]

        #expect(SyncActivity.of(status(transfers: sized), landing: 0, asked: false) == .fetching(songs: 2, fraction: 0.4))
        #expect(
            SyncActivity.of(status(transfers: sized + [transfer("C", .waiting)]), landing: 0, asked: false)
                == .fetching(songs: 3, fraction: nil)
        )
    }

    @Test func aTransferNeverCountsMoreThanItsSizeAndLandingAloneShowsNoFraction() {
        let over = [transfer("A", received: 500, size: 100, .moving)]

        #expect(SyncActivity.of(status(transfers: over), landing: 0, asked: false) == .fetching(songs: 1, fraction: 1))
        #expect(SyncActivity.of(status(), landing: 2, asked: false) == .fetching(songs: 2, fraction: nil))
    }

    @Test func theContinuedProgressSumsWhatIsLeftToFetchAndNamesTheSongsLeft() {
        let progress = ContinuedProgress.fetched(
            status(transfers: [
                transfer("A", received: 40, size: 100, .moving),
                transfer("B", .waiting),
                transfer("C", received: 100, size: 100, .landed),
            ])
        )

        #expect(progress == ContinuedProgress(done: 40, total: 100, subtitle: "2 songs left"))
    }
}

@Suite struct SyncAnswerTests {
    func answer(
        _ status: SyncStatus,
        activity: SyncActivity? = nil,
        waited: TimeInterval = 5,
        online: Bool = true
    ) -> SyncAnswer {
        SyncAnswer.after(status, activity: activity, config: config(), online: online, waited: waited)
    }

    @Test func nothingIsSaidBeforeTheServiceHadAMomentToAnswer() {
        #expect(answer(status(.refused), waited: 1) == .waiting)
        #expect(answer(status(.off), waited: 0) == .waiting)
    }

    @Test func syncTurnedOffEndsTheAskWithoutAWord() {
        #expect(answer(status(.off)) == .forget)
    }

    @Test func aSettledSyncIsToldWithTheDevicesItReached() {
        let withOthers = status(devices: [device("Desktop", online: true), device("Phone", thisDevice: true)])

        #expect(answer(withOthers, activity: .syncing) == .tell("Synced with 1 device"))
        #expect(answer(status(), activity: .syncing) == .tell("Synced · no other device is online"))
    }

    @Test func aSyncNotYetSettledKeepsWaiting() {
        #expect(answer(status(settled: false), activity: .syncing) == .waiting)
    }

    @Test func aRefusalOrAnUnreachableServerWithNobodyNearbyIsToldAtOnce() {
        #expect(answer(status(.refused)) == .tell("The server refused this iPhone"))
        #expect(answer(status(.unreachable)) == .tell("Can't reach the server"))
    }

    @Test func anUnreachableServerWithADeviceNearbyKeepsAskingTillItGivesUp() {
        let nearby = status(.unreachable, devices: [device("Tablet", nearby: true)])

        #expect(answer(nearby, waited: 10) == .waiting)
        #expect(answer(nearby, waited: SyncAnswer.givesUpAfter) == .forget)
    }

    @Test func anAskStillUnansweredGivesUpAfterTheLimit() {
        #expect(answer(status(.connecting), waited: SyncAnswer.givesUpAfter - 1) == .waiting)
        #expect(answer(status(.connecting), waited: SyncAnswer.givesUpAfter) == .forget)
    }
}

@Suite struct LandingDestinationTests {
    let music = MusicFolder(id: "music", name: "Music", path: "/music", readable: true, writable: true)
    let drive = MusicFolder(id: "drive", name: "Drive", path: "/drive", readable: false, writable: true)
    let shared = MusicFolder(id: "shared", name: "Shared", path: "/shared", readable: true, writable: false)

    @Test func aFolderOutOfReachHoldsTheSongsBackRatherThanSendingThemElsewhere() {
        #expect(LandingDestination.of([drive, music], chosen: "drive") == .away)
        #expect(LandingDestination.of([drive, music], chosen: nil) == .away)
    }

    @Test func theChosenFolderIsTakenElseTheFirstWritableOne() {
        #expect(LandingDestination.of([music, drive], chosen: nil) == .into(music))
        #expect(LandingDestination.of([shared, music], chosen: "shared") == .into(music))
        #expect(LandingDestination.of([music, drive], chosen: "gone") == .into(music))
    }

    @Test func noWritableFolderLeavesNowhereToLand() {
        #expect(LandingDestination.of([shared], chosen: nil) == .nowhere)
        #expect(LandingDestination.of([], chosen: "music") == .nowhere)
    }
}

@Suite struct LandingBackoffTests {
    func received(_ id: UInt64) -> ReceivedSong {
        ReceivedSong(id: id, name: "\(id).flac", extension: "flac", song: searchSong("\(id)"))
    }

    let now = Date(timeIntervalSince1970: 1_000_000)

    @Test func everySongWaitingIsLandableUntilOneIsRefused() {
        var backoff = LandingBackoff()
        let waiting = [received(1), received(2)]

        #expect(backoff.landable(waiting, now: now).map(\.id) == [1, 2])

        let dropped1 = backoff.refuse(1, now: now)

        #expect(!dropped1)
        #expect(backoff.landable(waiting, now: now).map(\.id) == [2])
    }

    @Test func aRefusedSongWaitsAMinuteAndThenIsTriedAgain() {
        var backoff = LandingBackoff()
        _ = backoff.refuse(1, now: now)

        #expect(backoff.landable([received(1)], now: now.addingTimeInterval(LandingBackoff.pause - 1)).isEmpty)
        #expect(backoff.landable([received(1)], now: now.addingTimeInterval(LandingBackoff.pause)).count == 1)
    }

    @Test func aSongRefusedThreeTimesInARowIsDroppedAtTheThirdAndForgotten() {
        var backoff = LandingBackoff()

        let dropped2 = backoff.refuse(1, now: now)

        #expect(!dropped2)
        let dropped3 = backoff.refuse(1, now: now)
        #expect(!dropped3)
        let dropped4 = backoff.refuse(1, now: now)
        #expect(dropped4)
        #expect(backoff.landable([received(1)], now: now).count == 1)
        let dropped5 = backoff.refuse(1, now: now)
        #expect(!dropped5)
    }

    @Test func aLandingForgetsItsRefusalsSoTheNextCountStartsAgain() {
        var backoff = LandingBackoff()
        _ = backoff.refuse(1, now: now)
        _ = backoff.refuse(1, now: now)

        backoff.landed(1)

        let dropped6 = backoff.refuse(1, now: now)

        #expect(!dropped6)
        let dropped7 = backoff.refuse(1, now: now)
        #expect(!dropped7)
    }

    @Test func songsNoLongerWaitingAreForgotten() {
        var backoff = LandingBackoff()
        _ = backoff.refuse(1, now: now)
        _ = backoff.refuse(2, now: now)

        backoff.keeping(only: [2])

        #expect(backoff.landable([received(1), received(2)], now: now).map(\.id) == [1])
    }

    @Test func oneSongsRefusalsDoNotPauseTheRest() {
        var backoff = LandingBackoff()
        _ = backoff.refuse(1, now: now)

        #expect(backoff.landable([received(1), received(2), received(3)], now: now).map(\.id) == [2, 3])
    }
}
