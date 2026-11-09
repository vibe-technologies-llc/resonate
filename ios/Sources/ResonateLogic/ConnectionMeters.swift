import ResonateNative

public enum ConnectionMeters {
    public static func of(satisfied: Bool, expensive: Bool, constrained: Bool) -> ConnectionMeter {
        guard satisfied else { return .unknown }
        return expensive || constrained ? .metered : .unmetered
    }

    public static func line(waiting: Bool) -> String? {
        waiting ? said("downloads.waiting-for-unmetered") : nil
    }
}
