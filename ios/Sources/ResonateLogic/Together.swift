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
        if !syncOn { return "Sync must be on to share playlists" }
        if nearbyOnly { return "This iPhone syncs on this network only, so playlists can't be shared" }
        if !online { return "Online features must be on to share playlists" }
        return nil
    }

    public static func party(online: Bool, nearbyOnly: Bool) -> String? {
        if !online { return "Online features must be on to hold a party" }
        if nearbyOnly { return "This iPhone syncs on this network only, so it can't reach a party" }
        return nil
    }
}

public enum TogetherWords {
    public static let unreadableLink = "That link could not be read"
    public static let notAnInvitation = "The clipboard holds no shared playlist or party link"
    public static let pressAgain = "Press again"

    public static func person(_ person: TogetherPerson) -> String {
        let name = person.isMe ? "You" : person.name.isEmpty ? "Someone" : person.name
        return person.isHost ? "\(name) · Host" : name
    }
}

public enum SharedPlaylistWords {
    public static let editLink = "Anyone with this link can edit"
    public static let followLink = "Anyone with this link can follow"
    public static let following = "You follow this playlist. Whoever shared it decides what is on it."
    public static let notHeldNamedAtMost = 5

    public static func kind(mayEdit: Bool) -> String {
        mayEdit ? "Shared" : "Followed"
    }

    public static func reach(_ reach: SharedPlaylistReach) -> String? {
        switch reach {
        case .waiting: "Reaching the server…"
        case .reached: nil
        case .unreached: "The server cannot be reached. Edits are kept and go when it can."
        case .serverTooOld: "The server is too old to keep shared playlists."
        case .turnedAway: "The server turned this playlist away."
        case .noServer: "This device is set to use no server, so nothing can be shared."
        }
    }

    public static func notHeld(_ titles: [String]) -> (named: [String], more: String?) {
        let left = titles.count - notHeldNamedAtMost
        return (Array(titles.prefix(notHeldNamedAtMost)), left > 0 ? "and \(left) more" : nil)
    }
}

public enum PartyWords {
    public static let guestAsks = "Adding a song to the queue asks the host."
    public static let songNotHeld = "You don't have that song to play"

    public static func following(_ following: PartyFollowing) -> String {
        switch following {
        case .waitingForTheHost: "Waiting for the host to play something"
        case .finding: "Looking for this song"
        case .notHeld: "You don't have this song, so you sit it out"
        case .cannotStartMidSong: "This song can't start part way through here, so you join at the next"
        case .satOut: "Paused here; press play to rejoin"
        case .hostPaused: "The host has paused"
        case .inStep: "In step with the host"
        }
    }

    public static func connection(_ connection: PartyConnection) -> String? {
        switch connection {
        case .connecting: "Reaching the server…"
        case .here: nil
        case .reconnecting: "Lost the server; trying again…"
        case .ended: "The party is over"
        case .noSuchParty: "That party has ended, or was never begun"
        case .turnedAway: "The server turned that link away"
        case .serverTooOld: "The server is too old to hold parties"
        }
    }

    public static func isOver(_ connection: PartyConnection) -> Bool {
        switch connection {
        case .connecting, .here, .reconnecting: false
        case .ended, .noSuchParty, .turnedAway, .serverTooOld: true
        }
    }

    public static func roundTrip(_ milliseconds: UInt64?) -> String? {
        milliseconds.map { "The server answers in \($0) ms" }
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
