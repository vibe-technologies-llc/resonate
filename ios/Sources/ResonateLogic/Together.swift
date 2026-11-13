import Foundation
import ResonateNative

public enum TogetherPace {
    public static let partyReadEvery: Duration = .milliseconds(500)
    public static let sheetReadEvery: Duration = .seconds(1)
}

public enum OpenedLink: Equatable {
    case invitation
    case unreadableInvitation
    case pairing
    case elsewhere

    private static let invitationHosts: Set<String> = ["shared", "party"]

    public static func of(_ url: URL) -> OpenedLink {
        if invitationKind(text: url.absoluteString) != nil { return .invitation }
        guard url.scheme?.lowercased() == "resonate" else { return .elsewhere }
        let host = URLComponents(url: url, resolvingAgainstBaseURL: false)?.host?.lowercased() ?? ""
        return invitationHosts.contains(host) ? .unreadableInvitation : .pairing
    }
}

public enum TogetherGate {
    public static func sharedPlaylists(syncOn: Bool, online: Bool, nearbyOnly: Bool) -> String? {
        if !syncOn { return said("together.gate.shared.sync-off") }
        if nearbyOnly { return said("together.gate.shared.nearby-only") }
        if !online { return said("together.gate.shared.offline") }
        return nil
    }

    public static func party(online: Bool, nearbyOnly: Bool) -> String? {
        if !online { return said("together.gate.party.offline") }
        if nearbyOnly { return said("together.gate.party.nearby-only") }
        return nil
    }
}

public enum TogetherWords {
    public static var unreadableLink: String { said("together.link.unreadable") }
    public static var notAnInvitation: String { said("together.link.not-an-invitation") }
    public static var pressAgain: String { said("together.press-again") }
    public static var joinThisPartyAsks: String { said("together.join-party.ask") }
    public static var joinThisPartySays: String { said("together.join-party.says") }
    public static var joinAListOnAnotherServerAsks: String { said("together.join-list.ask") }

    public static func joinAListOnAnotherServerSays(_ server: String) -> String {
        said("together.join-list.says", server)
    }

    public static func person(_ person: TogetherPerson) -> String {
        let name = person.isMe ? said("together.person.you") : person.name.isEmpty ? said("together.person.someone") : person.name
        return person.isHost ? said("together.person.host", name) : name
    }
}

public enum SharedPlaylistWords {
    public static var editLink: String { said("shared.link.edit") }
    public static var followLink: String { said("shared.link.follow") }
    public static var following: String { said("shared.following") }
    public static let notHeldNamedAtMost = 5

    public static func kind(mayEdit: Bool) -> String {
        mayEdit ? said("shared.kind.shared") : said("shared.kind.followed")
    }

    public static func mayEdit(shared: Bool?, followed: Bool) -> Bool {
        shared ?? !followed
    }

    public static func kind(shared mayEdit: Bool?, followed: Bool) -> String? {
        if let mayEdit { return kind(mayEdit: mayEdit) }
        return followed ? kind(mayEdit: false) : nil
    }

    public static func reach(_ reach: SharedPlaylistReach) -> String? {
        switch reach {
        case .waiting: said("together.server.reaching")
        case .reached: nil
        case .unreached: said("shared.reach.unreached")
        case .serverTooOld: said("shared.reach.too-old")
        case .turnedAway: said("shared.reach.turned-away")
        case .noServer: said("shared.reach.no-server")
        }
    }

    public static func notHeld(_ titles: [String]) -> (named: [String], more: String?) {
        let left = titles.count - notHeldNamedAtMost
        return (Array(titles.prefix(notHeldNamedAtMost)), left > 0 ? said("shared.not-held.more", left) : nil)
    }
}

public enum PartyWords {
    public static var guestAsks: String { said("party.guest-asks") }
    public static var songNotHeld: String { said("party.song-not-held") }

    public static func following(_ following: PartyFollowing) -> String {
        switch following {
        case .waitingForTheHost: said("party.following.waiting")
        case .finding: said("party.following.finding")
        case .notHeld: said("party.following.not-held")
        case .cannotStartMidSong: said("party.following.cannot-start-mid-song")
        case .satOut: said("party.following.sat-out")
        case .hostPaused: said("party.following.host-paused")
        case .inStep: said("party.following.in-step")
        }
    }

    public static func connection(_ connection: PartyConnection) -> String? {
        switch connection {
        case .connecting: said("together.server.reaching")
        case .here: nil
        case .reconnecting: said("party.connection.reconnecting")
        case .ended: said("party.connection.ended")
        case .handedOver: said("party.connection.handed-over")
        case .noSuchParty: said("party.connection.no-such-party")
        case .turnedAway: said("party.connection.turned-away")
        case .serverTooOld: said("party.connection.too-old")
        }
    }

    public static func isOver(_ connection: PartyConnection) -> Bool {
        switch connection {
        case .connecting, .here, .reconnecting: false
        case .ended, .handedOver, .noSuchParty, .turnedAway, .serverTooOld: true
        }
    }

    public static func handing(_ handing: PartyHandingOver) -> String {
        func named(_ name: String) -> String { name.isEmpty ? said("together.person.someone") : name }
        switch handing {
        case .handing(let name): return said("party.handing.to", named(name))
        case .notTaken(let name): return said("party.handing.not-taken", named(name))
        }
    }

    public static func handOverAsked(_ person: TogetherPerson) -> String {
        said("party.hand-over.asked", person.name.isEmpty ? said("together.person.someone") : person.name)
    }

    public static let chatLinesShown = 40
    public static let reactions: [PartyReactionKind] = [.love, .fire, .laugh, .clap, .dance, .wow]

    public static func reactionDrawn(_ reaction: PartyReactionKind) -> String {
        switch reaction {
        case .love: "\u{2764}\u{FE0F}"
        case .fire: "\u{1F525}"
        case .laugh: "\u{1F602}"
        case .clap: "\u{1F44F}"
        case .dance: "\u{1F483}"
        case .wow: "\u{1F62E}"
        }
    }

    public static func reactionNamed(_ reaction: PartyReactionKind) -> String {
        switch reaction {
        case .love: said("party.reaction.love")
        case .fire: said("party.reaction.fire")
        case .laugh: said("party.reaction.laugh")
        case .clap: said("party.reaction.clap")
        case .dance: said("party.reaction.dance")
        case .wow: said("party.reaction.wow")
        }
    }

    public static func reactWith(_ reaction: PartyReactionKind) -> String {
        said("party.react-with", reactionNamed(reaction))
    }

    public static func saidBy(_ line: PartyChatLine) -> String {
        line.isMe ? said("together.person.you") : line.name.isEmpty ? said("together.person.someone") : line.name
    }

    public static func shown(_ line: PartyChatLine) -> String {
        switch line.saying {
        case let .words(text): text
        case let .reaction(reaction): reactionDrawn(reaction)
        }
    }

    public static func heard(_ line: PartyChatLine) -> String {
        switch line.saying {
        case let .words(text): said("party.said-by", saidBy(line), text)
        case let .reaction(reaction): said("party.reacted-with", saidBy(line), reactionNamed(reaction))
        }
    }

    public static func roundTrip(_ milliseconds: UInt64?) -> String? {
        milliseconds.map { said("party.round-trip", Int($0)) }
    }

    public static func apart(_ apart: PartyClockApart?) -> String? {
        apart.map {
            let millis = String(format: "%+.1f", locale: Locale(identifier: "en_US_POSIX"), Double($0.nanos) / 1_000_000)
            return said($0.corrected ? "party.apart.corrected" : "party.apart", millis)
        }
    }
}

public enum SharedAct: Equatable, Sendable {
    case reset
    case leave
    case remove(Data)
}

public struct SecondPress<Action: Equatable>: Equatable {
    public private(set) var armed: Action?

    public init() {}

    public mutating func press(_ action: Action) -> Bool {
        guard armed == action else {
            armed = action
            return false
        }
        armed = nil
        return true
    }

    public func asks(_ action: Action) -> Bool {
        armed == action
    }
}
