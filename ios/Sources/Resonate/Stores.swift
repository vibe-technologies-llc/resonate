import AVFoundation
import Foundation
import ResonateNative
import Security

struct PlaybackPreferencesStore: Sendable {
    private static let suite = "playback"
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

    private var storage: UserDefaults { UserDefaults(suiteName: Self.suite) ?? .standard }

    var volume: Float {
        get { storage.object(forKey: Self.volumeKey) == nil ? 1 : min(max(storage.float(forKey: Self.volumeKey), 0), 1) }
        nonmutating set { storage.set(newValue, forKey: Self.volumeKey) }
    }

    func read(for route: OutputRoute?) -> PlaybackPreferences {
        var preferences = read()
        if let curve = curve(boundTo: route) { preferences.equaliser = curve }
        return preferences
    }

    func write(_ preferences: PlaybackPreferences, for route: OutputRoute?) {
        if let route, isBound(route) {
            writeChoices(preferences)
            keepCurve(preferences.equaliser, for: route)
        } else {
            write(preferences)
        }
    }

    func boundRoutes() -> [OutputRoute] {
        let names = storage.dictionary(forKey: Self.routeNamesKey) as? [String: String] ?? [:]
        return names.map { OutputRoute(id: $0.key, name: $0.value) }.sorted { $0.name < $1.name }
    }

    func isBound(_ route: OutputRoute) -> Bool {
        (storage.dictionary(forKey: Self.routeCurvesKey) as? [String: String])?[route.id] != nil
    }

    func bind(_ route: OutputRoute, to curve: EqualiserCurve) {
        var names = storage.dictionary(forKey: Self.routeNamesKey) as? [String: String] ?? [:]
        names[route.id] = route.name
        storage.set(names, forKey: Self.routeNamesKey)
        keepCurve(curve, for: route)
    }

    func unbind(_ route: OutputRoute) {
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

struct RouteReading: Equatable, Sendable {
    struct Output: Equatable, Sendable {
        let id: String
        let name: String
        let channels: Int
    }

    let outputs: [Output]
    let sampleRate: Double
    let latencyMs: Double

    static let unread = RouteReading(outputs: [], sampleRate: 0, latencyMs: 0)

    nonisolated static var current: RouteReading {
        let session = AVAudioSession.sharedInstance()
        return RouteReading(
            outputs: session.currentRoute.outputs.map { Output(id: $0.uid, name: $0.portName, channels: $0.channels?.count ?? 2) },
            sampleRate: session.sampleRate,
            latencyMs: (session.outputLatency + session.ioBufferDuration) * 1000
        )
    }
}

struct OutputRoute: Equatable, Sendable {
    let id: String
    let name: String

    static var current: OutputRoute? {
        AVAudioSession.sharedInstance().currentRoute.outputs.first.map { OutputRoute(id: $0.uid, name: $0.portName) }
    }
}

extension GainMode {
    static let every: [GainMode] = [.off, .track, .album]
    var name: String { "\(self)" }
    static func named(_ name: String?) -> GainMode? { every.first { $0.name == name } }
}

extension ResamplingQuality {
    static let every: [ResamplingQuality] = [.fast, .balanced, .high, .veryHigh]
    var name: String { "\(self)" }
    static func named(_ name: String?) -> ResamplingQuality? { every.first { $0.name == name } }
}

extension ResamplingPhase {
    static let every: [ResamplingPhase] = [.linear, .intermediate, .minimum]
    var name: String { "\(self)" }
    static func named(_ name: String?) -> ResamplingPhase? { every.first { $0.name == name } }
}

extension LossyRestoration {
    static let every: [LossyRestoration] = [.off, .repair, .extend]
    var name: String { "\(self)" }
    static func named(_ name: String?) -> LossyRestoration? { every.first { $0.name == name } }
}

extension MusicProvider {
    static let every: [MusicProvider] = [.subsonic, .tidal, .hifiApi, .monochrome]
    var name: String { "\(self)" }
    static func named(_ name: String?) -> MusicProvider? { every.first { $0.name == name } }
}

enum KeychainFailure: Swift.Error, LocalizedError {
    case refused(OSStatus)
    case unread(OSStatus)

    var errorDescription: String? {
        switch self {
        case .refused(let status): "The keychain refused the secret (\(status))"
        case .unread(let status): "The keychain could not be read (\(status))"
        }
    }
}

struct Keychain: Sendable {
    private static let service = "org.resonate.accounts"

    func read(_ name: String) throws -> String {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.service,
            kSecAttrAccount as String: name,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var found: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &found)
        if status == errSecItemNotFound { return "" }
        guard status == errSecSuccess, let data = found as? Data else { throw KeychainFailure.unread(status) }
        return String(decoding: data, as: UTF8.self)
    }

    func write(_ name: String, _ value: String) throws {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.service,
            kSecAttrAccount as String: name,
        ]
        guard !value.isEmpty else {
            let removed = SecItemDelete(query as CFDictionary)
            guard removed == errSecSuccess || removed == errSecItemNotFound else { throw KeychainFailure.refused(removed) }
            return
        }
        let data = Data(value.utf8)
        let updated = SecItemUpdate(query as CFDictionary, [kSecValueData as String: data] as CFDictionary)
        if updated == errSecSuccess { return }
        guard updated == errSecItemNotFound else { throw KeychainFailure.refused(updated) }

        var added = query
        added[kSecValueData as String] = data
        added[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let status = SecItemAdd(added as CFDictionary, nil)
        guard status == errSecSuccess else { throw KeychainFailure.refused(status) }
    }
}

struct ProviderAccounts: Equatable, Sendable {
    private(set) var accounts: [DownloadAccount]
    private(set) var switchedOff: Set<MusicProvider>

    init(kept: [DownloadAccount] = [], switchedOff: Set<MusicProvider> = []) {
        accounts = MusicProvider.every.map { provider in
            kept.first { $0.provider == provider } ?? DownloadAccount.empty(for: provider)
        }
        self.switchedOff = switchedOff
    }

    var asked: [DownloadAccount] {
        accounts.filter { isOn($0.provider) && $0.readiness == .ready }
    }

    func account(_ provider: MusicProvider) -> DownloadAccount {
        accounts.first { $0.provider == provider } ?? DownloadAccount.empty(for: provider)
    }

    func isOn(_ provider: MusicProvider) -> Bool { !switchedOff.contains(provider) }

    mutating func keep(_ account: DownloadAccount) {
        guard let index = accounts.firstIndex(where: { $0.provider == account.provider }) else { return }
        accounts[index] = account
    }

    mutating func turn(_ provider: MusicProvider, on: Bool) {
        if on {
            switchedOff.remove(provider)
        } else {
            switchedOff.insert(provider)
        }
    }
}

extension DownloadAccount {
    static func empty(for provider: MusicProvider) -> DownloadAccount {
        DownloadAccount(provider: provider, server: "", user: "", password: "", clientId: "", clientSecret: "", refreshToken: "")
    }
}

private struct KeptProviderAccount: Codable {
    let provider: String
    let server: String
    let user: String
    let password: String
    let clientId: String
    let clientSecret: String
    let refreshToken: String

    init(_ account: DownloadAccount) {
        provider = account.provider.name
        server = account.server
        user = account.user
        password = account.password
        clientId = account.clientId
        clientSecret = account.clientSecret
        refreshToken = account.refreshToken
    }

    var account: DownloadAccount? {
        MusicProvider.named(provider).map {
            DownloadAccount(
                provider: $0,
                server: server,
                user: user,
                password: password,
                clientId: clientId,
                clientSecret: clientSecret,
                refreshToken: refreshToken
            )
        }
    }
}

struct AccountStore: Sendable {
    private static let providerAccountsKey = "download-accounts"
    private static let switchedOffKey = "download-providers-off"
    private static let singleAccountProviderKey = "provider"
    private static let singleAccountServerKey = "server"
    private static let singleAccountSecrets = ["user", "password", "client-id", "client-secret", "refresh-token"]

    private let secrets = Keychain()
    private var plain: UserDefaults { UserDefaults(suiteName: "provider-accounts") ?? .standard }
    private var choices: UserDefaults { UserDefaults(suiteName: "library-features") ?? .standard }

    func readProviders() throws -> ProviderAccounts {
        let kept = try secrets.read(Self.providerAccountsKey)
        let accounts = kept.isEmpty ? try migratedSingleAccount().map { [$0] } ?? [] : decoded(kept)
        let switchedOff = (choices.stringArray(forKey: Self.switchedOffKey) ?? []).compactMap(MusicProvider.named)
        return ProviderAccounts(kept: accounts, switchedOff: Set(switchedOff))
    }

    func writeProviderAccounts(_ accounts: [DownloadAccount]) throws {
        let encoded = try JSONEncoder().encode(accounts.map(KeptProviderAccount.init))
        try secrets.write(Self.providerAccountsKey, String(decoding: encoded, as: UTF8.self))
    }

    func writeSwitchedOff(_ providers: Set<MusicProvider>) {
        choices.set(MusicProvider.every.filter(providers.contains).map(\.name), forKey: Self.switchedOffKey)
    }

    private func decoded(_ kept: String) -> [DownloadAccount] {
        let accounts = try? JSONDecoder().decode([KeptProviderAccount].self, from: Data(kept.utf8))
        return accounts?.compactMap(\.account) ?? []
    }

    private func migratedSingleAccount() throws -> DownloadAccount? {
        let plain = plain
        guard let provider = MusicProvider.named(plain.string(forKey: Self.singleAccountProviderKey)) else { return nil }

        let account = DownloadAccount(
            provider: provider,
            server: plain.string(forKey: Self.singleAccountServerKey) ?? "",
            user: try secrets.read("user"),
            password: try secrets.read("password"),
            clientId: try secrets.read("client-id"),
            clientSecret: try secrets.read("client-secret"),
            refreshToken: try secrets.read("refresh-token")
        )
        guard (try? writeProviderAccounts([account])) != nil else { return account }

        for name in Self.singleAccountSecrets { try? secrets.write(name, "") }
        plain.removeObject(forKey: Self.singleAccountProviderKey)
        plain.removeObject(forKey: Self.singleAccountServerKey)
        return account
    }

    func readScrobbling() throws -> ScrobbleAccount {
        ScrobbleAccount(
            listenbrainzToken: try secrets.read("listenbrainz"),
            lastfmKey: try secrets.read("lastfm-key"),
            lastfmSecret: try secrets.read("lastfm-secret"),
            lastfmSession: try secrets.read("lastfm-session")
        )
    }

    func writeScrobbling(_ account: ScrobbleAccount) throws {
        try secrets.write("listenbrainz", account.listenbrainzToken)
        try secrets.write("lastfm-key", account.lastfmKey)
        try secrets.write("lastfm-secret", account.lastfmSecret)
        try secrets.write("lastfm-session", account.lastfmSession)
    }

    func readRecognitionKey() throws -> String { try secrets.read("acoustid-key") }

    func writeRecognitionKey(_ value: String) throws { try secrets.write("acoustid-key", value) }

    func readAuddToken() throws -> String { try secrets.read("audd-token") }

    func writeAuddToken(_ value: String) throws { try secrets.write("audd-token", value) }

    func readSync() throws -> SyncConfig {
        let plain = plain
        let key = try secrets.read("sync-key")
        func flag(_ key: String, _ fallback: Bool) -> Bool {
            plain.object(forKey: key) == nil ? fallback : plain.bool(forKey: key)
        }
        return SyncConfig(
            enabled: flag("sync-enabled", true),
            nearbyOnly: flag("sync-nearby-only", false),
            server: plain.string(forKey: "sync-server") ?? "",
            key: key,
            device: plain.string(forKey: "sync-device") ?? "",
            favourites: flag("sync-favourites", false),
            playlists: flag("sync-playlists", false),
            everything: flag("sync-everything", false),
            meteredAllowed: flag("sync-metered-allowed", false)
        )
    }

    func writeSync(_ config: SyncConfig) throws {
        let plain = plain
        plain.set(config.enabled, forKey: "sync-enabled")
        plain.set(config.nearbyOnly, forKey: "sync-nearby-only")
        plain.set(config.server, forKey: "sync-server")
        plain.set(config.device, forKey: "sync-device")
        plain.set(config.favourites, forKey: "sync-favourites")
        plain.set(config.playlists, forKey: "sync-playlists")
        plain.set(config.everything, forKey: "sync-everything")
        plain.set(config.meteredAllowed, forKey: "sync-metered-allowed")
        try secrets.write("sync-key", config.key)
    }
}
