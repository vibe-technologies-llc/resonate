import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

private func person(_ name: String, me: Bool = false, host: Bool = false) -> TogetherPerson {
    TogetherPerson(id: Data(repeating: 1, count: 16), name: name, isMe: me, isHost: host)
}

@Suite struct OpenedLinkTests {
    private func opened(_ text: String) throws -> OpenedLink {
        try OpenedLink.of(#require(URL(string: text)))
    }

    @Test func aSharedListsLinkAndAPartysLinkAreInvitationsHoweverSpelled() throws {
        #expect(try opened("resonate://shared/E-ABCDE") == .invitation)
        #expect(try opened("resonate://party/ABCDE") == .invitation)
        #expect(try opened("RESONATE://SHARED/V-ABCDE?server=wss://own.example/") == .invitation)
        #expect(try opened("https://own.example/s/#E-ABCDE") == .invitation)
        #expect(try opened("https://own.example/p/#ABCDE") == .invitation)
    }

    @Test func anInvitationHoldingNoCodeIsUnreadableAndNeverAskedForAPairingCode() throws {
        #expect(try opened("resonate://shared/") == .unreadableInvitation)
        #expect(try opened("resonate://party") == .unreadableInvitation)
    }

    @Test func aPairingLinkOrASongOfTheSchemeStaysWithPairingSoNothingQueuesIt() throws {
        #expect(try opened("resonate://pair/ABCD-EFGH") == .pairing)
        #expect(try opened("resonate:track/42") == .pairing)
    }

    @Test func aFileIsNoInvitationWhateverItsPathHolds() throws {
        #expect(try opened("file:///Music/s/song.flac") == .elsewhere)
        #expect(try opened("file:///Music/party/song.flac") == .elsewhere)
    }

    @Test func thePastedTextIsToldApartByTheBoundary() {
        #expect(invitationKind(text: "  resonate://shared/E-ABCDE\n") == .playlist)
        #expect(invitationKind(text: "resonate://party/ABCDE") == .party)
        #expect(invitationKind(text: "love song") == nil)
    }
}

@Suite struct TogetherGateTests {
    @Test func sharedPlaylistsSayWhatIsMissing() {
        #expect(TogetherGate.sharedPlaylists(syncOn: false, online: true, nearbyOnly: false) == "Sync must be on to share playlists")
        #expect(TogetherGate.sharedPlaylists(syncOn: true, online: false, nearbyOnly: false) == "Online features must be on to share playlists")
        #expect(TogetherGate.sharedPlaylists(syncOn: true, online: true, nearbyOnly: true)?.contains("this network only") == true)
        #expect(TogetherGate.sharedPlaylists(syncOn: true, online: true, nearbyOnly: false) == nil)
    }

    @Test func aPartyNeedsOnlineAndAServerButNotSync() {
        #expect(TogetherGate.party(online: false, nearbyOnly: false) == "Online features must be on to hold a party")
        #expect(TogetherGate.party(online: true, nearbyOnly: true)?.contains("this network only") == true)
        #expect(TogetherGate.party(online: true, nearbyOnly: false) == nil)
    }
}

@Suite struct TogetherWordsTests {
    @Test func aPersonIsYouTheirNameOrSomeoneAndTheHostIsMarked() {
        #expect(TogetherWords.person(person("Ada", me: true)) == "You")
        #expect(TogetherWords.person(person("Ada")) == "Ada")
        #expect(TogetherWords.person(person("")) == "Someone")
        #expect(TogetherWords.person(person("Ada", host: true)) == "Ada · Host")
    }

    @Test func aListIsSharedOrFollowedByWhetherItMayBeEdited() {
        #expect(SharedPlaylistWords.kind(mayEdit: true) == "Shared")
        #expect(SharedPlaylistWords.kind(mayEdit: false) == "Followed")
    }

    @Test func aFollowedListIsToldFromTheOthersWithSyncOffToo() {
        #expect(SharedPlaylistWords.mayEdit(shared: nil, followed: false))
        #expect(!SharedPlaylistWords.mayEdit(shared: nil, followed: true))
        #expect(SharedPlaylistWords.mayEdit(shared: true, followed: true))
        #expect(!SharedPlaylistWords.mayEdit(shared: false, followed: false))
        #expect(SharedPlaylistWords.kind(shared: nil, followed: true) == "Followed")
        #expect(SharedPlaylistWords.kind(shared: nil, followed: false) == nil)
        #expect(SharedPlaylistWords.kind(shared: true, followed: false) == "Shared")
    }

    @Test func reachIsSaidOnlyWhenTheServerWasNotReached() {
        #expect(SharedPlaylistWords.reach(.reached) == nil)
        for reach in [SharedPlaylistReach.waiting, .unreached, .serverTooOld, .turnedAway, .noServer] {
            #expect(SharedPlaylistWords.reach(reach)?.isEmpty == false)
        }
    }

    @Test func songsNotHeldAreNamedUpToALimitAndTheRestCounted() {
        let few = SharedPlaylistWords.notHeld(["a", "b"])
        let many = SharedPlaylistWords.notHeld((1...8).map(String.init))

        #expect(few.named == ["a", "b"])
        #expect(few.more == nil)
        #expect(many.named == ["1", "2", "3", "4", "5"])
        #expect(many.more == "and 3 more")
    }
}

@Suite struct PartyWordsTests {
    @Test func everyStandingHasItsSentence() {
        #expect(PartyWords.following(.waitingForTheHost) == "Waiting for the host to play something")
        #expect(PartyWords.following(.finding) == "Looking for this song")
        #expect(PartyWords.following(.notHeld) == "You don't have this song, so you sit it out")
        #expect(PartyWords.following(.cannotStartMidSong) == "This song can't start part way through here, so you join at the next")
        #expect(PartyWords.following(.satOut) == "Paused here; press play to rejoin")
        #expect(PartyWords.following(.hostPaused) == "The host has paused")
        #expect(PartyWords.following(.inStep) == "In step with the host")
    }

    @Test func aConnectionIsSaidOnlyWhenNotHere() {
        #expect(PartyWords.connection(.here) == nil)
        #expect(PartyWords.connection(.connecting) == "Reaching the server…")
        #expect(PartyWords.connection(.reconnecting) == "Lost the server; trying again…")
        #expect(PartyWords.connection(.ended) == "The party is over")
        #expect(PartyWords.connection(.noSuchParty) == "That party has ended, or was never begun")
        #expect(PartyWords.connection(.turnedAway) == "The server turned that link away")
        #expect(PartyWords.connection(.serverTooOld) == "The server is too old to hold parties")
    }

    @Test func aPartyIsOverOnceNoReconnectingWouldBringItBack() {
        for going in [PartyConnection.connecting, .here, .reconnecting] {
            #expect(!PartyWords.isOver(going))
        }
        for over in [PartyConnection.ended, .noSuchParty, .turnedAway, .serverTooOld] {
            #expect(PartyWords.isOver(over))
        }
    }

    @Test func theRoundTripIsSaidOnlyOnceMeasured() {
        #expect(PartyWords.roundTrip(nil) == nil)
        #expect(PartyWords.roundTrip(34) == "The server answers in 34 ms")
    }
}

@Suite struct SecondPressTests {
    private let ada = SharedAct.remove(Data(repeating: 1, count: 16))
    private let ben = SharedAct.remove(Data(repeating: 2, count: 16))

    @Test func theFirstPressAsksAndTheSecondActs() {
        var presses = SecondPress<SharedAct>()

        let first = presses.press(.reset)
        let asked = presses.asks(.reset)
        let second = presses.press(.reset)

        #expect(!first)
        #expect(asked)
        #expect(second)
        #expect(!presses.asks(.reset))
    }

    @Test func pressingSomethingElseAsksAboutThatInstead() {
        var presses = SecondPress<SharedAct>()

        let reset = presses.press(.reset)
        let leave = presses.press(.leave)
        let stillAsksReset = presses.asks(.reset)
        let leaveAgain = presses.press(.leave)

        #expect(!reset)
        #expect(!leave)
        #expect(!stillAsksReset)
        #expect(leaveAgain)
    }

    @Test func removingOnePersonIsNotConfirmedByPressingRemoveOnAnother() {
        var presses = SecondPress<SharedAct>()

        let first = presses.press(ada)
        let other = presses.press(ben)

        #expect(!first)
        #expect(!other)
        #expect(presses.asks(ben))
        #expect(!presses.asks(ada))
    }
}
