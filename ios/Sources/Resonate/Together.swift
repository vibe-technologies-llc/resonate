import Foundation
import Observation
import ResonateLogic
import ResonateNative

@MainActor
@Observable
final class TogetherState {
    private static let nameKey = "shared-name"

    @ObservationIgnored private unowned let library: Library
    @ObservationIgnored private let settings: UserDefaults
    @ObservationIgnored private var syncSettledOnce = false
    @ObservationIgnored private var invitationWaiting: String?
    @ObservationIgnored private var watches = 0

    private(set) var lists: [UInt64: SharedPlaylist] = [:]
    private(set) var party: ListeningParty?
    private(set) var joining = false
    private(set) var name: String
    var toShow: InvitationKind?
    var partyOffered: String?

    init(library: Library) {
        self.library = library
        let settings = UserDefaults(suiteName: "library-features") ?? .standard
        self.settings = settings
        name = settings.string(forKey: Self.nameKey) ?? ""
    }

    var sharingGate: String? {
        let features = library.features
        return TogetherGate.sharedPlaylists(
            syncOn: features.syncConfig?.enabled ?? false,
            online: features.online,
            nearbyOnly: features.syncConfig?.nearbyOnly ?? false
        )
    }

    var partyGate: String? {
        let features = library.features
        return TogetherGate.party(online: features.online, nearbyOnly: features.syncConfig?.nearbyOnly ?? false)
    }

    func shared(_ id: UInt64) -> SharedPlaylist? {
        lists[id]
    }

    func mayEdit(_ id: UInt64) -> Bool {
        lists[id]?.mayEdit ?? true
    }

    func tellName(to native: MobilePlayer) {
        native.togetherNamed(name: name)
    }

    func rename(_ typed: String) {
        let trimmed = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed != name else { return }
        name = trimmed
        settings.set(trimmed, forKey: Self.nameKey)
        Task {
            guard let native = try? await library.player() else { return }
            _ = try? await Work.aside { native.togetherNamed(name: trimmed) }
        }
    }

    func syncSettled() {
        syncSettledOnce = true
        if let waiting = invitationWaiting {
            invitationWaiting = nil
            accept(waiting)
        }
    }

    func networkRulesChanged() {
        guard party != nil, let gate = partyGate else { return }
        leaveParty()
        library.notice(gate)
    }

    func adopt(_ found: [SharedPlaylist]) {
        let keyed = Dictionary(
            found.compactMap { list in list.playlist.map { ($0, list) } },
            uniquingKeysWith: { kept, _ in kept }
        )
        if lists != keyed { lists = keyed }
    }

    func arrive(_ text: String) {
        if invitationKind(text: text) == .party {
            partyOffered = text
        } else {
            accept(text)
        }
    }

    func accept(_ text: String) {
        guard let kind = invitationKind(text: text) else {
            library.notice(TogetherWords.notAnInvitation)
            return
        }
        guard syncSettledOnce else {
            invitationWaiting = text
            return
        }
        if let gate = kind == .party ? partyGate : sharingGate {
            library.fail(gate)
            return
        }
        guard !joining else { return }
        joining = true
        Task {
            defer { joining = false }
            do {
                let native = try await library.player()
                let (accepted, lists, party) = try await Work.aside {
                    let accepted = try native.acceptInvitation(text: text)
                    return (accepted, native.sharedPlaylists(), native.partyShown())
                }
                adopt(lists)
                self.party = party
                library.features.loadPlaylists()
                toShow = accepted
            } catch {
                library.report(error)
            }
        }
    }

    func watch(_ id: UInt64) async {
        guard let native = try? await library.player() else { return }
        watches += 1
        let watch = watches
        _ = try? await Work.aside { native.sharedWatch(id: id) }
        while !Task.isCancelled {
            if let found = try? await Work.aside({ native.sharedPlaylists() }) { adopt(found) }
            try? await Task.sleep(for: TogetherPace.sheetReadEvery)
        }
        guard watch == watches else { return }
        _ = try? await Work.aside { native.sharedWatch(id: nil) }
    }

    func share(_ id: UInt64) async -> String? {
        await changing { _ = try $0.sharePlaylist(id: id) }
    }

    func leaveList(_ id: UInt64) async -> String? {
        await changing { try $0.sharedLeave(id: id) }
    }

    func reset(_ id: UInt64, removing: Data?) async -> String? {
        await changing { _ = try $0.sharedReset(id: id, removing: removing) }
    }

    private func changing(_ change: @escaping @Sendable (MobilePlayer) throws -> Void) async -> String? {
        do {
            let native = try await library.player()
            let found = try await Work.aside {
                try change(native)
                return native.sharedPlaylists()
            }
            adopt(found)
            library.features.loadPlaylists()
            return nil
        } catch {
            return explained(error)
        }
    }

    func followParty() async {
        guard let native = try? await library.player() else { return }
        while !Task.isCancelled {
            let shown = try? await Work.aside { native.partyShown() }
            if party != shown { party = shown }
            try? await Task.sleep(for: TogetherPace.partyReadEvery)
        }
    }

    func beginParty() {
        guard !joining else { return }
        if let gate = partyGate {
            library.fail(gate)
            return
        }
        joining = true
        Task {
            defer { joining = false }
            do {
                let native = try await library.player()
                party = try await Work.aside { try native.partyBegin() }
            } catch {
                library.report(error)
            }
        }
    }

    func leaveParty() {
        party = nil
        Task {
            guard let native = try? await library.player() else { return }
            _ = try? await Work.aside { native.partyLeave() }
        }
    }

    func turnOut(_ person: TogetherPerson) {
        let id = person.id
        Task {
            guard let native = try? await library.player() else { return }
            _ = try? await Work.aside { native.partyTurnOut(person: id) }
        }
    }

    func playNext(_ request: Int) {
        Task {
            guard let native = try? await library.player() else { return }
            let placed = (try? await Work.aside { native.partyPlayNext(request: UInt32(request)) }) ?? false
            if !placed { library.fail(PartyWords.songNotHeld) }
        }
    }

    func passOver(_ request: Int) {
        Task {
            guard let native = try? await library.player() else { return }
            _ = try? await Work.aside { native.partyPassOver(request: UInt32(request)) }
        }
    }
}
