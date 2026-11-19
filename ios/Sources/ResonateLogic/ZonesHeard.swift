import ResonateNative

public enum SessionSpeaksFor: Equatable, Sendable {
    case shown
    case elsewhere
}

public enum LockScreenCommand: Equatable, Sendable {
    case play
    case pause
    case toggle
}

public enum LockScreenAnswer: Equatable, Sendable {
    case nothing
    case resumeShown
    case pauseShown
    case toggleShown
    case pauseSounding
}

public enum ZonesHeard {
    public static func sounds(_ shown: Transport?, _ elsewhere: Transport?) -> Bool {
        shown.sounds || elsewhere.sounds
    }

    public static func speaksFor(shown: Transport?, elsewhere: Transport?, elsewhereNamesASong: Bool) -> SessionSpeaksFor {
        !shown.sounds && elsewhere.sounds && elsewhereNamesASong ? .elsewhere : .shown
    }

    public static func answering(_ command: LockScreenCommand, speaksFor: SessionSpeaksFor) -> LockScreenAnswer {
        switch (speaksFor, command) {
        case (.elsewhere, .play): .nothing
        case (.elsewhere, .pause), (.elsewhere, .toggle): .pauseSounding
        case (.shown, .play): .resumeShown
        case (.shown, .pause): .pauseShown
        case (.shown, .toggle): .toggleShown
        }
    }
}

extension Transport {
    public var sounds: Bool { self == .playing || self == .loading }
}

extension Optional where Wrapped == Transport {
    public var sounds: Bool { self?.sounds == true }
}

extension Snapshot {
    public var anyZoneSounds: Bool { ZonesHeard.sounds(transport, heardElsewhere?.transport) }

    public var spokenElsewhere: HeardElsewhere? {
        guard let heard = heardElsewhere else { return nil }
        let speaksFor = ZonesHeard.speaksFor(
            shown: transport,
            elsewhere: heard.transport,
            elsewhereNamesASong: heard.current != nil
        )
        return speaksFor == .elsewhere ? heard : nil
    }
}

public struct NowPlayingSpoken: Equatable, Sendable {
    public let track: Track
    public let transport: Transport
    public let positionMs: UInt64
    public let durationMs: UInt64
    public let zoneHeard: UInt32?

    public init?(_ snapshot: Snapshot) {
        if let heard = snapshot.spokenElsewhere, let track = heard.current {
            self.track = track
            transport = heard.transport
            positionMs = heard.positionMs
            durationMs = heard.durationMs
            zoneHeard = heard.zone
        } else if let track = snapshot.current {
            self.track = track
            transport = snapshot.transport
            positionMs = snapshot.positionMs
            durationMs = snapshot.durationMs
            zoneHeard = nil
        } else {
            return nil
        }
    }
}
