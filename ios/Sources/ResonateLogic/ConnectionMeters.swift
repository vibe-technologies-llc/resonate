import ResonateNative

public enum ConnectionMeters {
    public static let waitingLine = "Waiting for a network that is not metered"

    public static func of(satisfied: Bool, expensive: Bool, constrained: Bool) -> ConnectionMeter {
        guard satisfied else { return .unknown }
        return expensive || constrained ? .metered : .unmetered
    }

    public static func line(waiting: Bool) -> String? {
        waiting ? waitingLine : nil
    }
}
