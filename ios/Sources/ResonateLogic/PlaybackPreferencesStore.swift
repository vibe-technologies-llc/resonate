import Foundation
import ResonateNative

public struct PlaybackPreferencesStore: Sendable {
    public static let suite = "playback"
    private static let bufferRange: ClosedRange<Int> = 50...2000
    private static let curveKey = "equaliser-curve"
    private static let curveNameKey = "equaliser-curve-name"
    private static let tenBandPreampKey = "equaliser-preamp"
    private static let tenBandProfileNameKey = "equaliser-profile-name"
    private static let tenBandProfileTextKey = "equaliser-profile-text"
    private static let tenBands = 10
    private static let tenBandGainKeys = (0..<tenBands).map { "equaliser-gain-\($0)" }

    private static let routeCurvesKey = "equaliser-curve-for"
    private static let routeCurveNamesKey = "equaliser-curve-name-for"
    private static let routeNamesKey = "equaliser-route-names"

    private static let volumeKey = "player-volume"

    private let suiteName: String

    public init(suite: String = PlaybackPreferencesStore.suite) {
        suiteName = suite
    }

    private var storage: UserDefaults { UserDefaults(suiteName: suiteName) ?? .standard }

    public var volume: Float {
        get { storage.object(forKey: Self.volumeKey) == nil ? 1 : min(max(storage.float(forKey: Self.volumeKey), 0), 1) }
        nonmutating set { storage.set(newValue, forKey: Self.volumeKey) }
    }

    public func read(for route: OutputRoute?) -> PlaybackPreferences {
        var preferences = read()
        if let curve = curve(boundTo: route) { preferences.equaliser = curve }
        return preferences
    }

    public func write(_ preferences: PlaybackPreferences, for route: OutputRoute?) {
        if let route, isBound(route) {
            writeChoices(preferences)
            keepCurve(preferences.equaliser, for: route)
        } else {
            write(preferences)
        }
    }

    public func boundRoutes() -> [OutputRoute] {
        let names = storage.dictionary(forKey: Self.routeNamesKey) as? [String: String] ?? [:]
        return names.map { OutputRoute(id: $0.key, name: $0.value) }.sorted { $0.name < $1.name }
    }

    public func isBound(_ route: OutputRoute) -> Bool {
        (storage.dictionary(forKey: Self.routeCurvesKey) as? [String: String])?[route.id] != nil
    }

    public func bind(_ route: OutputRoute, to curve: EqualiserCurve) {
        var names = storage.dictionary(forKey: Self.routeNamesKey) as? [String: String] ?? [:]
        names[route.id] = route.name
        storage.set(names, forKey: Self.routeNamesKey)
        keepCurve(curve, for: route)
    }

    public func unbind(_ route: OutputRoute) {
        for key in [Self.routeCurvesKey, Self.routeCurveNamesKey, Self.routeNamesKey] {
            var kept = storage.dictionary(forKey: key) as? [String: String] ?? [:]
            kept[route.id] = nil
            storage.set(kept, forKey: key)
        }
    }

    private func curve(boundTo route: OutputRoute?) -> EqualiserCurve? {
        guard let route,
            let text = (storage.dictionary(forKey: Self.routeCurvesKey) as? [String: String])?[route.id]
        else { return nil }
        let name = (storage.dictionary(forKey: Self.routeCurveNamesKey) as? [String: String])?[route.id] ?? ""
        return try? parseEqProfile(bytes: Data(text.utf8), name: name).curve
    }

    private func keepCurve(_ curve: EqualiserCurve, for route: OutputRoute) {
        guard let text = try? equaliserText(curve: curve) else { return }
        var texts = storage.dictionary(forKey: Self.routeCurvesKey) as? [String: String] ?? [:]
        var names = storage.dictionary(forKey: Self.routeCurveNamesKey) as? [String: String] ?? [:]
        texts[route.id] = text
        names[route.id] = curve.name
        storage.set(texts, forKey: Self.routeCurvesKey)
        storage.set(names, forKey: Self.routeCurveNamesKey)
    }

    private func read() -> PlaybackPreferences {
        var preferences = defaultPlaybackPreferences()
        let storage = storage
        if storage.object(forKey: "equaliser-enabled") != nil {
            preferences.equaliserEnabled = storage.bool(forKey: "equaliser-enabled")
        }
        preferences.replayGain = GainMode.named(storage.string(forKey: "replay-gain")) ?? preferences.replayGain
        preferences.quality = ResamplingQuality.named(storage.string(forKey: "quality")) ?? preferences.quality
        preferences.phase = ResamplingPhase.named(storage.string(forKey: "phase")) ?? preferences.phase
        preferences.restoration =
            LossyRestoration.named(storage.string(forKey: "restoration")) ?? preferences.restoration
        if storage.object(forKey: "buffer-ms") != nil {
            let buffer = storage.integer(forKey: "buffer-ms")
            preferences.bufferMs = UInt32(min(max(buffer, Self.bufferRange.lowerBound), Self.bufferRange.upperBound))
        }
        if storage.object(forKey: "dsd-like-pcm") != nil {
            preferences.dsdLikePcm = storage.bool(forKey: "dsd-like-pcm")
        }
        if storage.object(forKey: "true-peak") != nil {
            preferences.truePeak = storage.bool(forKey: "true-peak")
        }

        if let kept = storage.string(forKey: Self.curveKey) {
            let name = storage.string(forKey: Self.curveNameKey) ?? ""
            preferences.equaliser = (try? parseEqProfile(bytes: Data(kept.utf8), name: name).curve)
                ?? preferences.equaliser
        } else if let migrated = tenBandCurve(storage) {
            preferences.equaliser = migrated
            keepCurve(migrated, in: storage)
        }
        return preferences
    }

    private func write(_ preferences: PlaybackPreferences) {
        writeChoices(preferences)
        keepCurve(preferences.equaliser, in: storage)
    }

    private func writeChoices(_ preferences: PlaybackPreferences) {
        let storage = storage
        storage.set(preferences.equaliserEnabled, forKey: "equaliser-enabled")
        storage.set(preferences.replayGain.name, forKey: "replay-gain")
        storage.set(preferences.quality.name, forKey: "quality")
        storage.set(preferences.phase.name, forKey: "phase")
        storage.set(Int(preferences.bufferMs), forKey: "buffer-ms")
        storage.set(preferences.dsdLikePcm, forKey: "dsd-like-pcm")
        storage.set(preferences.truePeak, forKey: "true-peak")
        storage.set(preferences.restoration.name, forKey: "restoration")
    }

    private func keepCurve(_ curve: EqualiserCurve, in storage: UserDefaults) {
        guard let text = try? equaliserText(curve: curve) else { return }
        storage.set(text, forKey: Self.curveKey)
        storage.set(curve.name, forKey: Self.curveNameKey)
        for key in [Self.tenBandPreampKey, Self.tenBandProfileNameKey, Self.tenBandProfileTextKey] + Self.tenBandGainKeys {
            storage.removeObject(forKey: key)
        }
    }

    private func tenBandCurve(_ storage: UserDefaults) -> EqualiserCurve? {
        let profileName = storage.string(forKey: Self.tenBandProfileNameKey) ?? ""
        if let profile = storage.string(forKey: Self.tenBandProfileTextKey), !profile.isEmpty,
            let imported = try? parseEqProfile(bytes: Data(profile.utf8), name: profileName)
        {
            return imported.curve
        }
        let stored = ([Self.tenBandPreampKey] + Self.tenBandGainKeys).contains { storage.object(forKey: $0) != nil }
        guard stored else { return nil }
        return ResonateNative.tenBandCurve(
            preampDb: level(Self.tenBandPreampKey, in: storage),
            gainsDb: Self.tenBandGainKeys.map { level($0, in: storage) }
        )
    }

    private func level(_ key: String, in storage: UserDefaults) -> Float {
        guard storage.object(forKey: key) != nil else { return 0 }
        return storage.float(forKey: key)
    }
}
