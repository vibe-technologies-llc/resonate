import Foundation
import ResonateLogic
import ResonateNative
import SwiftUI

struct PlayedArtist: Identifiable, Equatable, Sendable {
    let name: String
    let plays: UInt64
    let cover: Track?

    var id: String { name }
}

struct HomeShelves: Sendable {
    let played: [JumpBackTile]
    let recent: [AlbumHit]
}

struct HomeListened: Sendable {
    let glance: ListeningGlance
    let artists: [PlayedArtist]
}

@MainActor
@Observable
final class HomeFeed {
    nonisolated static let shelved = 12

    private(set) var played: [JumpBackTile] = []
    private(set) var recent: [AlbumHit] = []
    private(set) var mixes = OfferedMixes()
    private(set) var artists: [PlayedArtist] = []
    private(set) var glance: ListeningGlance?
    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private var moment: HomeMoment?
    @ObservationIgnored private var reading: Task<Void, Never>?
    @ObservationIgnored private var owed = false
    @ObservationIgnored private var owedAfresh = false

    init(library: Library) {
        self.library = library
    }

    func read(afresh: Bool = false) async {
        if let reading {
            owed = true
            owedAfresh = owedAfresh || afresh
            await reading.value
            return
        }
        let run = Task {
            var afresh = afresh
            repeat {
                owed = false
                owedAfresh = false
                await readOnce(afresh: afresh)
                afresh = owedAfresh
            } while owed
            reading = nil
        }
        reading = run
        await run.value
    }

    private func readOnce(afresh: Bool) async {
        guard let native = try? await library.player(),
            let now = try? await Work.onScreen({ HomeMoment.of(catalogRevision: native.catalogRevision()) })
        else { return }
        guard afresh || now != moment else { return }

        async let shelves = Self.shelves(in: native)
        async let offered = OfferedMixes.read(in: library)
        async let listened = Self.listened(in: native)
        let (readShelves, readMixes, readListened) = await (shelves, offered, listened)

        if let readShelves {
            if played != readShelves.played { played = readShelves.played }
            if recent != readShelves.recent { recent = readShelves.recent }
        }
        if let readMixes, mixes != readMixes { mixes = readMixes }
        if let readListened {
            if glance != readListened.glance { glance = readListened.glance }
            if artists != readListened.artists { artists = readListened.artists }
        }
        if readShelves != nil, readMixes != nil, readListened != nil { moment = now }
    }

    nonisolated private static func shelves(in native: MobilePlayer) async -> HomeShelves? {
        try? await Work.onScreen {
            let heard = try native.filteredTracks(filter: JumpBackIn.heardLately, offset: 0, limit: JumpBackIn.songsRead)
            let recent = Array(try native.albums(order: .added).prefix(shelved))
            return HomeShelves(played: JumpBackIn.albums(heard: heard), recent: recent)
        }
    }

    nonisolated private static func listened(in native: MobilePlayer) async -> HomeListened? {
        let zone = TimeZone.current
        let identifier = zone.identifier
        let offset = Int32(zone.secondsFromGMT())
        return try? await Work.onScreen {
            let statistics = try native.statistics(days: 0)
            let detail = try native.listeningDetail(days: 0, limit: UInt32(shelved), timeZone: identifier, utcOffsetSeconds: offset)
            let glance = ListeningGlance.of(
                ListeningGlance.Heard(
                    days: detail.days,
                    firstHeard: detail.firstHeard,
                    plays: statistics.plays,
                    listenedMs: statistics.listenedMs,
                    songsHeard: detail.songsHeard,
                    songs: statistics.songs
                )
            )
            let artists = try detail.artists.map { artist in
                PlayedArtist(name: artist.name, plays: artist.plays, cover: try native.mostPlayedBy(name: artist.name, limit: 1).first)
            }
            return HomeListened(glance: glance, artists: artists)
        }
    }
}
