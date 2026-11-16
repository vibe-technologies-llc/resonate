import Foundation

public enum MeteredDownloads {
    static let allowedKey = "download-on-metered"
    static let stalledAsFirstSaved = "stall-downloads-on-metered"

    public static func allowed(in settings: UserDefaults) -> Bool {
        if let allowed = settings.object(forKey: allowedKey) as? Bool { return allowed }
        if let stalled = settings.object(forKey: stalledAsFirstSaved) as? Bool { return !stalled }
        return true
    }

    public static func keep(_ allowed: Bool, in settings: UserDefaults) {
        settings.set(allowed, forKey: allowedKey)
        settings.removeObject(forKey: stalledAsFirstSaved)
    }
}
