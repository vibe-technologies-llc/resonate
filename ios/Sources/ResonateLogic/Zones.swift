import Foundation
import ResonateNative

public struct ZonesListed: Equatable {
    public static let readEveryTicks = 8

    public let zone: UInt32
    public let revision: UInt64

    public init(_ state: Snapshot) {
        zone = state.zone
        revision = state.zonesRevision
    }

    public func isDue(after read: ZonesListed?, zones: Int, ticks: Int) -> Bool {
        guard let read else { return true }
        return read != self || (zones > 1 && ticks >= Self.readEveryTicks)
    }
}

public enum ZonePlan {
    public static let first: UInt32 = 1

    public static func mayHandOver(to zone: UInt32, shown: UInt32, shownHolds: Bool) -> Bool {
        zone != shown && shownHolds
    }

    public static func mayRemove(_ zone: UInt32) -> Bool {
        zone != first
    }

    public static func rendererLine(host: String, viaCast: Bool) -> String {
        viaCast ? said("zones.renderer.cast", host) : host
    }

    public static func line(_ transport: Transport, playing: String?, on renderer: String? = nil) -> String {
        let doing = switch transport {
        case .playing: playing.map { said("zones.line.playing-song", $0) } ?? said("zones.line.playing")
        case .loading: said("zones.line.loading")
        case .paused: playing.map { said("zones.line.paused-on", $0) } ?? said("zones.line.paused")
        case .stopped: said("zones.line.stopped")
        }
        return renderer.map { "\($0) · \(doing)" } ?? doing
    }
}
