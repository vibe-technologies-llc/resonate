import Foundation
import ResonateNative

public struct ProviderAccounts: Equatable, Sendable {
    public private(set) var accounts: [DownloadAccount]
    public private(set) var switchedOff: Set<MusicProvider>

    public init(kept: [DownloadAccount] = [], switchedOff: Set<MusicProvider> = []) {
        accounts = MusicProvider.every.map { provider in
            kept.first { $0.provider == provider } ?? DownloadAccount.empty(for: provider)
        }
        self.switchedOff = switchedOff
    }

    public var asked: [DownloadAccount] {
        accounts.filter { isOn($0.provider) && $0.readiness == .ready }
    }

    public func account(_ provider: MusicProvider) -> DownloadAccount {
        accounts.first { $0.provider == provider } ?? DownloadAccount.empty(for: provider)
    }

    public func isOn(_ provider: MusicProvider) -> Bool { !switchedOff.contains(provider) }

    public mutating func keep(_ account: DownloadAccount) {
        guard let index = accounts.firstIndex(where: { $0.provider == account.provider }) else { return }
        accounts[index] = account
    }

    public mutating func turn(_ provider: MusicProvider, on: Bool) {
        if on {
            switchedOff.remove(provider)
        } else {
            switchedOff.insert(provider)
        }
    }
}

extension DownloadAccount {
    public static func empty(for provider: MusicProvider) -> DownloadAccount {
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

public struct AccountStore: Sendable {
    public static let plainSuite = "provider-accounts"
    public static let choicesSuite = "library-features"

    private static let providerAccountsKey = "download-accounts"
    private static let switchedOffKey = "download-providers-off"

    private let secrets: any Secrets
    private let plainSuite: String
    private let choicesSuite: String

    public init(
        secrets: any Secrets,
        plainSuite: String = AccountStore.plainSuite,
        choicesSuite: String = AccountStore.choicesSuite
    ) {
        self.secrets = secrets
        self.plainSuite = plainSuite
        self.choicesSuite = choicesSuite
    }

    private var plain: UserDefaults { UserDefaults(suiteName: plainSuite) ?? .standard }
    private var choices: UserDefaults { UserDefaults(suiteName: choicesSuite) ?? .standard }

    public func readProviders() throws -> ProviderAccounts {
        let kept = try secrets.read(Self.providerAccountsKey)
        let accounts = kept.isEmpty ? [] : decoded(kept)
        let switchedOff = (choices.stringArray(forKey: Self.switchedOffKey) ?? []).compactMap(MusicProvider.named)
        return ProviderAccounts(kept: accounts, switchedOff: Set(switchedOff))
    }

    public func writeProviderAccounts(_ accounts: [DownloadAccount]) throws {
        let encoded = try JSONEncoder().encode(accounts.map(KeptProviderAccount.init))
        try secrets.write(Self.providerAccountsKey, String(decoding: encoded, as: UTF8.self))
    }

    public func writeSwitchedOff(_ providers: Set<MusicProvider>) {
        choices.set(MusicProvider.every.filter(providers.contains).map(\.name), forKey: Self.switchedOffKey)
    }

    private func decoded(_ kept: String) -> [DownloadAccount] {
        let accounts = try? JSONDecoder().decode([KeptProviderAccount].self, from: Data(kept.utf8))
        return accounts?.compactMap(\.account) ?? []
    }

    public func readScrobbling() throws -> ScrobbleAccount {
        ScrobbleAccount(
            listenbrainzToken: try secrets.read("listenbrainz"),
            lastfmKey: try secrets.read("lastfm-key"),
            lastfmSecret: try secrets.read("lastfm-secret"),
            lastfmSession: try secrets.read("lastfm-session")
        )
    }

    public func writeScrobbling(_ account: ScrobbleAccount) throws {
        try secrets.write("listenbrainz", account.listenbrainzToken)
        try secrets.write("lastfm-key", account.lastfmKey)
        try secrets.write("lastfm-secret", account.lastfmSecret)
        try secrets.write("lastfm-session", account.lastfmSession)
    }

    public func readRecognitionKey() throws -> String { try secrets.read("acoustid-key") }

    public func writeRecognitionKey(_ value: String) throws { try secrets.write("acoustid-key", value) }

    public func readAuddToken() throws -> String { try secrets.read("audd-token") }

    public func writeAuddToken(_ value: String) throws { try secrets.write("audd-token", value) }

    public func readSync() throws -> SyncConfig {
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

    public func writeSync(_ config: SyncConfig) throws {
        try secrets.write("sync-key", config.key)

        let plain = plain
        plain.set(config.enabled, forKey: "sync-enabled")
        plain.set(config.nearbyOnly, forKey: "sync-nearby-only")
        plain.set(config.server, forKey: "sync-server")
        plain.set(config.device, forKey: "sync-device")
        plain.set(config.favourites, forKey: "sync-favourites")
        plain.set(config.playlists, forKey: "sync-playlists")
        plain.set(config.everything, forKey: "sync-everything")
        plain.set(config.meteredAllowed, forKey: "sync-metered-allowed")
    }
}
