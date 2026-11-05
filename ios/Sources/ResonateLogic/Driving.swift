import ResonateNative

public enum SyncControls {
    public static func of(_ stored: String?) -> SyncControl {
        switch stored {
        case "transport": .transport
        case "nothing": .nothing
        default: .everything
        }
    }

    public static func spelt(_ control: SyncControl) -> String {
        switch control {
        case .everything: "everything"
        case .transport: "transport"
        case .nothing: "nothing"
        }
    }
}

public struct SessionAsked: Sendable {
    private var asked = false

    public init() {}

    public mutating func ask() {
        asked = true
    }

    public mutating func owed(by transport: Transport) -> Bool {
        guard transport == .playing || transport == .loading else {
            asked = false
            return false
        }
        defer { asked = true }
        return !asked
    }
}

public enum DrivenRows {
    public static func row(_ offset: Int, firstRow: UInt32) -> UInt32 {
        firstRow + UInt32(offset)
    }

    public static func moveUp(_ row: UInt32) -> RemoteControl? {
        row == 0 ? nil : .move(from: row, to: row - 1)
    }

    public static func moveDown(_ row: UInt32, queueLength: UInt32) -> RemoteControl? {
        row + 1 < queueLength ? .move(from: row, to: row + 1) : nil
    }

    public static func seek(to seconds: Double, lengthMillis: UInt64) -> RemoteControl {
        let asked = UInt64((max(seconds, 0) * 1000).rounded())
        return .seek(millis: min(asked, lengthMillis))
    }
}
