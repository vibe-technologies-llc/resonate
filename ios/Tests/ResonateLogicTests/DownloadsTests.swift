import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

func searchSong(_ title: String = "Echoes") -> SearchSong {
    SearchSong(
        recording: "rec-\(title)",
        title: title,
        artist: "Pink Floyd",
        album: "Meddle",
        durationMs: 1_400_000,
        isrcs: []
    )
}

let folder = MusicFolder(id: "local", name: "On this iPhone", path: "/music", readable: true, writable: true)

@Suite struct DownloadJobTests {
    func job() -> DownloadJob {
        DownloadJob(id: 1, song: searchSong(), folder: folder)
    }

    @Test func aJobStartsWaitingWithNothingReceived() {
        let job = job()

        #expect(job.state == .waiting)
        #expect(job.received == 0)
        #expect(job.total == nil)
        #expect(job.fraction == nil)
    }

    @Test func theProgressOfAJobInFlightMovesItsStateAndCounts() {
        var job = job()
        job.state = .finding

        job.follow(DownloadProgress(phase: .downloading, received: 400, total: 1_000, provider: .subsonic))

        #expect(job.state == .downloading)
        #expect(job.received == 400)
        #expect(job.total == 1_000)
        #expect(job.provider == .subsonic)
        #expect(job.fraction == 0.4)

        job.follow(DownloadProgress(phase: .validating, received: 1_000, total: 1_000, provider: .subsonic))
        #expect(job.state == .validating)
        #expect(job.fraction == 1)
    }

    @Test func aTotalOrProviderNotYetKnownKeepsWhatWasKnown() {
        var job = job()
        job.state = .downloading
        job.follow(DownloadProgress(phase: .downloading, received: 1, total: 100, provider: .tidal))

        job.follow(DownloadProgress(phase: .downloading, received: 2, total: nil, provider: nil))

        #expect(job.total == 100)
        #expect(job.provider == .tidal)
        #expect(job.received == 2)
    }

    @Test(arguments: [DownloadPhase.done, .failed, .cancelled])
    func anEndingPhaseLeavesTheStateToTheCallerWhoSetsItWithTheOutcome(phase: DownloadPhase) {
        var job = job()
        job.state = .downloading

        job.follow(DownloadProgress(phase: phase, received: 9, total: 9, provider: nil))

        #expect(job.state == .downloading)
        #expect(job.received == 9)
    }

    @Test func aJobNotInFlightKeepsItsStateWhateverTheProgressSays() {
        for state in [DownloadState.waiting, .failed("x"), .cancelled] {
            var job = job()
            job.state = state

            job.follow(DownloadProgress(phase: .downloading, received: 5, total: 10, provider: nil))

            #expect(job.state == state)
            #expect(job.received == 5)
        }
    }

    @Test func aFractionNeverPassesOneNorDividesByZero() {
        var job = job()
        job.received = 120
        job.total = 100
        #expect(job.fraction == 1)

        job.total = 0
        #expect(job.fraction == nil)
    }

    @Test func theStatesSayWhetherTheyAreInFlightOrSettled() {
        #expect([DownloadState.finding, .downloading, .validating].allSatisfy { $0.inFlight && !$0.settled })
        #expect([DownloadState.cancelled, .failed("x")].allSatisfy { $0.settled && !$0.inFlight })
        #expect(!DownloadState.waiting.inFlight && !DownloadState.waiting.settled)
        #expect(DownloadState.done(track(1)).settled)
    }
}

@Suite struct ProviderReadinessTests {
    func account(
        _ provider: MusicProvider,
        server: String = "",
        user: String = "",
        password: String = "",
        clientId: String = "",
        refreshToken: String = ""
    ) -> DownloadAccount {
        DownloadAccount(
            provider: provider,
            server: server,
            user: user,
            password: password,
            clientId: clientId,
            clientSecret: "",
            refreshToken: refreshToken
        )
    }

    @Test func theHostedServicesAreAlwaysReady() {
        #expect(account(.monochrome).readiness == .ready)
        #expect(account(.hifiApi).readiness == .ready)
    }

    @Test func subsonicNeedsAServerThenALogin() {
        #expect(account(.subsonic).readiness == .missingServer)
        #expect(account(.subsonic, server: "   ").readiness == .missingServer)
        #expect(account(.subsonic, server: "https://m.example").readiness == .missingLogin)
        #expect(account(.subsonic, server: "https://m.example", user: "me").readiness == .missingLogin)
        #expect(account(.subsonic, server: "https://m.example", user: "me", password: "pw").readiness == .ready)
    }

    @Test func tidalNeedsAClientThenASignIn() {
        #expect(account(.tidal).readiness == .missingClient)
        #expect(account(.tidal, clientId: "c").readiness == .needsSignIn)
        #expect(account(.tidal, clientId: "c", refreshToken: " ").readiness == .needsSignIn)
        #expect(account(.tidal, clientId: "c", refreshToken: "r").readiness == .ready)
    }

    @Test func anAccountUsesItsOwnServerOnlyWhereOneIsTyped() {
        #expect(!account(.monochrome).usesItsOwnServer)
        #expect(!account(.monochrome, server: "  ").usesItsOwnServer)
        #expect(account(.monochrome, server: "https://m.example").usesItsOwnServer)
    }
}

@Suite struct DownloadJournalTests {
    let defaults = Defaults()

    var journal: DownloadJournal { DownloadJournal(suite: defaults.name) }

    @Test func aJournalOfNothingListsNothing() {
        #expect(journal.uris.isEmpty)
    }

    @Test func aDocumentIsNotedWhileBeingWrittenAndForgottenOnceItLandsOrIsDeleted() {
        journal.change { $0.append("resonate-folder://a/1.flac") }
        journal.change { $0.append("resonate-folder://a/2.flac") }
        #expect(journal.uris == ["resonate-folder://a/1.flac", "resonate-folder://a/2.flac"])

        journal.change { $0.removeAll { $0 == "resonate-folder://a/1.flac" } }
        #expect(journal.uris == ["resonate-folder://a/2.flac"])

        journal.change { $0.removeAll() }
        #expect(journal.uris.isEmpty)
        #expect(defaults.first.object(forKey: DownloadJournal.key) == nil)
    }

    @Test func aSingleStringAnOlderBuildLeftIsReadAsAListOfOne() {
        defaults.first.set("resonate-folder://a/old.flac", forKey: DownloadJournal.key)

        #expect(journal.uris == ["resonate-folder://a/old.flac"])
    }

    @Test func recoveryDropsWhatWasSettledAndKeepsWhatCouldNotBeDeleted() {
        journal.change { $0 = ["a", "b", "c"] }

        journal.settleRecovery(left: ["a", "b", "c"], kept: ["b"])

        #expect(journal.uris == ["b"])
    }

    @Test func recoveryKeepsADocumentJournalledWhileItRanAndEmptiesWhenNothingIsLeft() {
        journal.change { $0 = ["a", "b"] }
        journal.change { $0.append("new") }

        journal.settleRecovery(left: ["a", "b"], kept: [])
        #expect(journal.uris == ["new"])

        journal.change { $0.removeAll() }
        journal.settleRecovery(left: ["new"], kept: [])
        #expect(defaults.first.object(forKey: DownloadJournal.key) == nil)
    }
}
