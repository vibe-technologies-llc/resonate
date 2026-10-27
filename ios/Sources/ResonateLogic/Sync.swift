import Foundation
import ResonateNative

public enum SyncActivity: Equatable {
    case syncing
    case fetching(songs: Int, fraction: Double?)

    public static func of(_ status: SyncStatus?, landing: Int, asked: Bool) -> SyncActivity? {
        guard let status, status.standing != .off else { return nil }
        let active = status.transfers.filter { $0.state == .waiting || $0.state == .moving }
        if !active.isEmpty || landing > 0 {
            let sized = active.filter { $0.size > 0 }
            let size = sized.reduce(UInt64(0)) { $0 + $1.size }
            let received = sized.reduce(UInt64(0)) { $0 + min($1.received, $1.size) }
            let fraction = size == 0 || sized.count != active.count ? nil : Double(received) / Double(size)
            return .fetching(songs: active.count + landing, fraction: fraction)
        }
        return asked ? .syncing : nil
    }
}

public func fetchingHeading(downloading: UInt32, waiting: UInt32) -> String {
    let counts = [
        downloading > 0 ? "\(downloading) downloading" : nil,
        waiting > 0 ? "\(waiting) waiting" : nil,
    ].compactMap { $0 }
    return (["Fetching"] + counts).joined(separator: " · ")
}

public struct ContinuedProgress: Equatable {
    public var done: Int64 = 0
    public var total: Int64 = 0
    public var subtitle = ""

    public init(done: Int64 = 0, total: Int64 = 0, subtitle: String = "") {
        self.done = done
        self.total = total
        self.subtitle = subtitle
    }

    public static func fetched(_ status: SyncStatus) -> ContinuedProgress {
        let active = status.transfers.filter { $0.state == .waiting || $0.state == .moving }
        let size = active.reduce(UInt64(0)) { $0 + $1.size }
        let received = active.reduce(UInt64(0)) { $0 + min($1.received, $1.size) }
        return ContinuedProgress(
            done: Int64(clamping: received),
            total: Int64(clamping: size),
            subtitle: counted(active.count, "song") + " left"
        )
    }
}

public func syncStanding(_ status: SyncStatus?, config: SyncConfig?, online: Bool) -> String {
    guard let config, config.enabled else { return "Off" }
    if !online, !config.nearbyOnly { return "Waiting for online features" }
    let others = status?.devices.filter { !$0.thisDevice } ?? []
    let nearby = others.filter(\.nearby).count
    let reached = others.filter(\.online).count
    switch status?.standing {
    case .connected:
        return reached == 0 ? "No other device online" : "\(counted(reached, "device")) online"
    case .nearby:
        return nearby == 0 ? "Looking on this network" : "\(counted(nearby, "device")) on this network"
    case .unreachable:
        return nearby == 0 ? "Can't reach the server" : "Server unreachable · \(counted(nearby, "device")) on this network"
    case .refused:
        return "The server refused this iPhone"
    case .connecting:
        return nearby == 0 ? "Connecting…" : "Connecting · \(counted(nearby, "device")) on this network"
    case .off, nil:
        return "Starting…"
    }
}

public enum SyncRoute: Hashable {
    case hosted
    case own
    case nearby

    public init(_ config: SyncConfig) {
        if config.nearbyOnly {
            self = .nearby
        } else if config.server.trimmingCharacters(in: .whitespaces).isEmpty {
            self = .hosted
        } else {
            self = .own
        }
    }
}

public struct KeyedTransfer {
    public let key: String
    public let transfer: SyncTransfer
}

public enum SyncTransferKeys {
    public static func keyed(_ transfers: [SyncTransfer], as group: String) -> [KeyedTransfer] {
        var seen: [String: Int] = [:]
        return transfers.map { transfer in
            let identity = "\(group)\u{1f}\(transfer.from)\u{1f}\(transfer.artist)\u{1f}\(transfer.title)"
            let occurrence = seen[identity, default: 0]
            seen[identity] = occurrence + 1
            return KeyedTransfer(key: "\(identity)\u{1f}\(occurrence)", transfer: transfer)
        }
    }
}

public enum SyncAnswer: Equatable {
    case waiting
    case forget
    case tell(String)

    public static let answersAfter: TimeInterval = 1.5
    public static let givesUpAfter: TimeInterval = 45

    public static func after(
        _ status: SyncStatus,
        activity: SyncActivity?,
        config: SyncConfig?,
        online: Bool,
        waited: TimeInterval
    ) -> SyncAnswer {
        guard waited >= answersAfter else { return .waiting }
        let others = status.devices.filter { !$0.thisDevice && ($0.online || $0.nearby) }
        if status.standing == .off {
            return .forget
        } else if activity == .syncing, status.settled {
            return .tell(others.isEmpty ? "Synced · no other device is online" : "Synced with \(counted(others.count, "device"))")
        } else if status.standing == .refused || (status.standing == .unreachable && others.isEmpty) {
            return .tell(syncStanding(status, config: config, online: online))
        } else if waited >= givesUpAfter {
            return .forget
        }
        return .waiting
    }
}

public enum LandingDestination: Equatable {
    case into(MusicFolder)
    case away
    case nowhere

    public static let awayText = "The folder songs from your devices go to cannot be reached; they wait until it can"
    public static let nowhereText = "Add a music folder with write access to keep songs from your devices"

    public static func of(_ folders: [MusicFolder], chosen: String?) -> LandingDestination {
        guard let folder = folders.first(where: { $0.id == chosen && $0.writable }) ?? folders.first(where: \.writable) else {
            return .nowhere
        }
        return folder.readable ? .into(folder) : .away
    }

    public var folder: MusicFolder? {
        if case .into(let folder) = self { return folder }
        return nil
    }
}

public struct LandingBackoff {
    public static let pause: TimeInterval = 60
    public static let triesAtMost = 3

    private struct Refused {
        let until: Date
        let tries: Int
    }

    private var refused: [UInt64: Refused] = [:]

    public init() {}

    public func landable(_ waiting: [ReceivedSong], now: Date) -> [ReceivedSong] {
        waiting.filter { (refused[$0.id]?.until ?? .distantPast) <= now }
    }

    public mutating func keeping(only still: Set<UInt64>) {
        refused = refused.filter { still.contains($0.key) }
    }

    public mutating func landed(_ id: UInt64) {
        refused[id] = nil
    }

    public mutating func refuse(_ id: UInt64, now: Date) -> Bool {
        let tries = (refused[id]?.tries ?? 0) + 1
        guard tries >= Self.triesAtMost else {
            refused[id] = Refused(until: now.addingTimeInterval(Self.pause), tries: tries)
            return false
        }
        refused[id] = nil
        return true
    }
}
