import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

final class Defaults: @unchecked Sendable {
    let names: [String]

    init(_ count: Int = 1) {
        names = (0..<count).map { _ in "resonate-test-\(UUID().uuidString)" }
    }

    var name: String { names[0] }

    var first: UserDefaults { UserDefaults(suiteName: name)! }

    deinit {
        for name in names { UserDefaults().removePersistentDomain(forName: name) }
    }
}

final class FakeItems: SecretItems, @unchecked Sendable {
    private let lock = NSLock()
    private var kept: [String: Data] = [:]
    private var scripted: [String: Int32] = [:]

    func script(_ operation: String, _ status: Int32) { lock.withLock { scripted[operation] = status } }

    func has(_ account: String) -> Bool { lock.withLock { kept[account] != nil } }

    func hold(_ account: String, _ value: String) { lock.withLock { kept[account] = Data(value.utf8) } }

    func value(_ account: String) -> String? {
        lock.withLock { kept[account].map { String(decoding: $0, as: UTF8.self) } }
    }

    func copy(_ account: String) -> (status: Int32, data: Data?) {
        lock.withLock {
            if let status = scripted["copy"] { return (status, nil) }
            guard let data = kept[account] else { return (KeychainStatus.itemNotFound, nil) }
            return (KeychainStatus.success, data)
        }
    }

    func update(_ account: String, _ data: Data) -> Int32 {
        lock.withLock {
            if let status = scripted["update"] { return status }
            guard kept[account] != nil else { return KeychainStatus.itemNotFound }
            kept[account] = data
            return KeychainStatus.success
        }
    }

    func add(_ account: String, _ data: Data) -> Int32 {
        lock.withLock {
            if let status = scripted["add"] { return status }
            kept[account] = data
            return KeychainStatus.success
        }
    }

    func delete(_ account: String) -> Int32 {
        lock.withLock {
            if let status = scripted["delete"] { return status }
            return kept.removeValue(forKey: account) == nil ? KeychainStatus.itemNotFound : KeychainStatus.success
        }
    }
}

let interactionNotAllowed: Int32 = -25308

@Suite struct KeychainTests {
    let items = FakeItems()

    var keychain: Keychain { Keychain(items: items) }

    @Test func anItemThatIsNotThereReadsBlank() throws {
        #expect(try keychain.read("missing") == "")
    }

    @Test func anItemThatIsThereReadsAsTheTextItHolds() throws {
        items.hold("sync-key", "pair-me")

        #expect(try keychain.read("sync-key") == "pair-me")
    }

    @Test func aReadTheKeychainRefusesIsAFailureNeverABlank() {
        items.hold("sync-key", "pair-me")
        items.script("copy", interactionNotAllowed)

        #expect(throws: KeychainFailure.self) { try keychain.read("sync-key") }
    }

    @Test func aSuccessCarryingNoDataIsAFailureToo() {
        items.script("copy", KeychainStatus.success)

        #expect(throws: KeychainFailure.self) { try keychain.read("sync-key") }
    }

    @Test func aWriteUpdatesAnItemInPlace() throws {
        items.hold("token", "old")

        try keychain.write("token", "new")

        #expect(items.value("token") == "new")
    }

    @Test func aWriteAddsAnItemWhereNoneIsThere() throws {
        try keychain.write("token", "new")

        #expect(items.value("token") == "new")
    }

    @Test func aRefusedUpdateKeepsTheOldSecretAndAddsNothing() {
        items.hold("token", "old")
        items.script("update", interactionNotAllowed)
        items.script("add", KeychainStatus.success)

        #expect(throws: KeychainFailure.self) { try keychain.write("token", "new") }
        #expect(items.value("token") == "old")
    }

    @Test func aRefusedAddIsAFailure() {
        items.script("add", interactionNotAllowed)

        #expect(throws: KeychainFailure.self) { try keychain.write("token", "new") }
        #expect(!items.has("token"))
    }

    @Test func aBlankWriteRemovesTheItemAndAMissingOneIsNoFailure() throws {
        items.hold("token", "old")

        try keychain.write("token", "")
        try keychain.write("token", "")

        #expect(!items.has("token"))
    }

    @Test func aBlankWriteTheKeychainRefusesIsAFailure() {
        items.hold("token", "old")
        items.script("delete", interactionNotAllowed)

        #expect(throws: KeychainFailure.self) { try keychain.write("token", "") }
        #expect(items.value("token") == "old")
    }

    @Test func theFailuresReadAsSentencesWithTheirStatus() {
        #expect(KeychainFailure.unread(-1).errorDescription == "The keychain could not be read (-1)")
        #expect(KeychainFailure.refused(-2).errorDescription == "The keychain refused the secret (-2)")
    }
}

@Suite struct NamesTests {
    @Test func eachEnginePreferenceIsNamedByItsCaseAndFoundByThatName() {
        #expect(GainMode.named("album") == .album)
        #expect(ResamplingQuality.named("veryHigh") == .veryHigh)
        #expect(ResamplingPhase.named("intermediate") == .intermediate)
        #expect(LossyRestoration.named("repair") == .repair)
        #expect(MusicProvider.named("hifiApi") == .hifiApi)
    }

    @Test func aNameNoCaseHasIsNotFound() {
        #expect(GainMode.named("loud") == nil)
        #expect(GainMode.named(nil) == nil)
        #expect(MusicProvider.named("Tidal") == nil)
    }

    @Test func everyCaseIsListedAndNamedBackToItself() {
        for mode in GainMode.every { #expect(GainMode.named(mode.name) == mode) }
        for quality in ResamplingQuality.every { #expect(ResamplingQuality.named(quality.name) == quality) }
        for phase in ResamplingPhase.every { #expect(ResamplingPhase.named(phase.name) == phase) }
        for restoration in LossyRestoration.every { #expect(LossyRestoration.named(restoration.name) == restoration) }
        for provider in MusicProvider.every { #expect(MusicProvider.named(provider.name) == provider) }
        #expect(MusicProvider.every.count == 4)
    }
}

@Suite struct PlaybackPreferencesTests {
    let defaults = Defaults()

    var store: PlaybackPreferencesStore { PlaybackPreferencesStore(suite: defaults.name) }

    @Test func nothingStoredReadsTheEnginesDefaults() {
        #expect(store.read(for: nil) == defaultPlaybackPreferences())
    }

    @Test func whatIsWrittenIsReadBack() {
        var chosen = defaultPlaybackPreferences()
        chosen.equaliserEnabled = true
        chosen.replayGain = .album
        chosen.quality = .veryHigh
        chosen.phase = .minimum
        chosen.restoration = .repair
        chosen.truePeak = !chosen.truePeak
        chosen.dsdLikePcm = !chosen.dsdLikePcm
        chosen.bufferMs = 750

        store.write(chosen, for: nil)

        let read = store.read(for: nil)
        #expect(read.equaliserEnabled)
        #expect(read.replayGain == .album)
        #expect(read.quality == .veryHigh)
        #expect(read.phase == .minimum)
        #expect(read.restoration == .repair)
        #expect(read.truePeak == chosen.truePeak)
        #expect(read.dsdLikePcm == chosen.dsdLikePcm)
        #expect(read.bufferMs == 750)
    }

    @Test(arguments: [(0, UInt32(50)), (49, 50), (50, 50), (2_000, 2_000), (2_001, 2_000), (-5, 50)])
    func aBufferOutsideWhatTheEngineAcceptsIsHeldToTheRange(stored: Int, read: UInt32) {
        defaults.first.set(stored, forKey: "buffer-ms")

        #expect(store.read(for: nil).bufferMs == read)
    }

    @Test func aNameNoCaseHasFallsBackToTheDefault() {
        defaults.first.set("louder", forKey: "replay-gain")
        defaults.first.set("ultra", forKey: "quality")

        let read = store.read(for: nil)

        #expect(read.replayGain == defaultPlaybackPreferences().replayGain)
        #expect(read.quality == defaultPlaybackPreferences().quality)
    }

    @Test func theVolumeIsOneUntilSetAndHeldBetweenSilenceAndFull() {
        #expect(store.volume == 1)

        store.volume = 0.25
        #expect(store.volume == 0.25)

        defaults.first.set(7.0, forKey: "player-volume")
        #expect(store.volume == 1)
        defaults.first.set(-3.0, forKey: "player-volume")
        #expect(store.volume == 0)
    }

    @Test func aCurveIsKeptAsTextAndKeepsItsNameAndBands() {
        let curve = EqualiserCurve(
            name: "My headphones",
            preampDb: -4,
            bands: [EqualiserBand(filter: .peaking, frequencyHz: 1_000, gainDb: 3, q: 1.5)]
        )
        var preferences = defaultPlaybackPreferences()
        preferences.equaliser = curve

        store.write(preferences, for: nil)

        let read = store.read(for: nil).equaliser
        #expect(read.name == "My headphones")
        #expect(read.preampDb == -4)
        #expect(read.bands.count == 1)
        #expect(read.bands[0].frequencyHz == 1_000)
        #expect(read.bands[0].gainDb == 3)
        #expect(defaults.first.string(forKey: "equaliser-curve") != nil)
    }

    @Test func theOlderTenBandKeysAreReadOnceAndMigratedToTheCurve() {
        defaults.first.set(-2.5, forKey: "equaliser-preamp")
        defaults.first.set(4.0, forKey: "equaliser-gain-3")

        let read = store.read(for: nil).equaliser

        #expect(read.preampDb == -2.5)
        #expect(read.bands.contains { $0.gainDb == 4 })
        #expect(defaults.first.string(forKey: "equaliser-curve") != nil)
        #expect(defaults.first.object(forKey: "equaliser-gain-3") == nil)
        #expect(defaults.first.object(forKey: "equaliser-preamp") == nil)

        let again = store.read(for: nil).equaliser
        #expect(again.preampDb == read.preampDb)
        #expect(again.bands.map(\.gainDb) == read.bands.map(\.gainDb))
        #expect(again.bands.map(\.frequencyHz) == read.bands.map(\.frequencyHz))
        #expect(zip(again.bands, read.bands).allSatisfy { abs($0.q - $1.q) < 0.001 })
    }

    @Test func aCurveBoundToAnOutputIsPlayedOnThatOutputAndTheSharedOneElsewhere() {
        let route = OutputRoute(id: "uid-1", name: "AirPods")
        let curve = EqualiserCurve(
            name: "Buds",
            preampDb: -3,
            bands: [EqualiserBand(filter: .peaking, frequencyHz: 200, gainDb: -2, q: 1)]
        )
        var shared = defaultPlaybackPreferences()
        shared.equaliser = EqualiserCurve(name: "Shared", preampDb: 0, bands: [])
        store.write(shared, for: nil)

        store.bind(route, to: curve)

        #expect(store.isBound(route))
        #expect(store.read(for: route).equaliser.name == "Buds")
        #expect(store.read(for: nil).equaliser.name == "Shared")
        #expect(store.read(for: OutputRoute(id: "uid-2", name: "Speaker")).equaliser.name == "Shared")
    }

    @Test func aWriteOnABoundOutputChangesItsCurveAndLeavesTheSharedOne() {
        let route = OutputRoute(id: "uid-1", name: "AirPods")
        var shared = defaultPlaybackPreferences()
        shared.equaliser = EqualiserCurve(name: "Shared", preampDb: 0, bands: [])
        store.write(shared, for: nil)
        store.bind(route, to: EqualiserCurve(name: "Buds", preampDb: -1, bands: []))

        var edited = store.read(for: route)
        edited.equaliser = EqualiserCurve(name: "Buds edited", preampDb: -6, bands: [])
        edited.quality = .fast
        store.write(edited, for: route)

        #expect(store.read(for: route).equaliser.name == "Buds edited")
        #expect(store.read(for: nil).equaliser.name == "Shared")
        #expect(store.read(for: nil).quality == .fast)
    }

    @Test func boundOutputsAreListedByNameAndForgottenWhenUnbound() {
        let buds = OutputRoute(id: "uid-1", name: "AirPods")
        let car = OutputRoute(id: "uid-2", name: "Car")
        let curve = EqualiserCurve(name: "x", preampDb: 0, bands: [])
        store.bind(car, to: curve)
        store.bind(buds, to: curve)

        #expect(store.boundRoutes() == [buds, car])

        store.unbind(buds)
        #expect(store.boundRoutes() == [car])
        #expect(!store.isBound(buds))
        #expect(store.isBound(car))
    }
}

@Suite struct AccountStoreTests {
    let defaults = Defaults(2)
    let items = FakeItems()

    var store: AccountStore {
        AccountStore(secrets: Keychain(items: items), plainSuite: defaults.names[0], choicesSuite: defaults.names[1])
    }

    func account(_ provider: MusicProvider, server: String = "", user: String = "", password: String = "") -> DownloadAccount {
        DownloadAccount(
            provider: provider,
            server: server,
            user: user,
            password: password,
            clientId: "",
            clientSecret: "",
            refreshToken: ""
        )
    }

    @Test func nothingKeptGivesAnEmptyAccountForEveryProviderInOrderAndNoneSwitchedOff() throws {
        let read = try store.readProviders()

        #expect(read.accounts.map(\.provider) == MusicProvider.every)
        #expect(read.accounts.allSatisfy { $0 == DownloadAccount.empty(for: $0.provider) })
        #expect(read.switchedOff.isEmpty)
        #expect(read.asked.map(\.provider) == [.hifiApi, .monochrome])
    }

    @Test func accountsWrittenAreReadBackAndCarryEverySecret() throws {
        let kept = DownloadAccount(
            provider: .tidal,
            server: "",
            user: "",
            password: "",
            clientId: "client",
            clientSecret: "secret",
            refreshToken: "refresh"
        )
        try store.writeProviderAccounts([account(.subsonic, server: "https://music.example", user: "me", password: "pw"), kept])

        let read = try store.readProviders()

        #expect(read.account(.subsonic).server == "https://music.example")
        #expect(read.account(.tidal) == kept)
        #expect(read.account(.monochrome) == .empty(for: .monochrome))
        #expect(items.has("download-accounts"))
    }

    @Test func aProviderNoLongerKnownIsDroppedFromWhatWasKept() throws {
        items.hold(
            "download-accounts",
            #"[{"provider":"napster","server":"","user":"","password":"","clientId":"","clientSecret":"","refreshToken":""}]"#
        )

        let read = try store.readProviders()

        #expect(read.accounts.allSatisfy { $0 == .empty(for: $0.provider) })
    }

    @Test func providersSwitchedOffAreKeptInTheOrderOfTheListAndUnknownNamesAreIgnored() throws {
        store.writeSwitchedOff([.monochrome, .subsonic])
        defaults.first.set(["monochrome", "napster", "subsonic"], forKey: "download-providers-off")

        #expect(try store.readProviders().switchedOff == [.subsonic, .monochrome])

        store.writeSwitchedOff([.tidal, .subsonic])
        #expect(defaults.names[1] != defaults.names[0])
        #expect(UserDefaults(suiteName: defaults.names[1])?.stringArray(forKey: "download-providers-off") == ["subsonic", "tidal"])
    }

    @Test func aSwitchedOffProviderIsNotAskedEvenWhenReady() throws {
        store.writeSwitchedOff([.monochrome])

        #expect(try store.readProviders().asked.map(\.provider) == [.hifiApi])
    }

    @Test func theOlderSingleAccountIsMigratedToTheListAndItsSecretsAreCleared() throws {
        let plain = UserDefaults(suiteName: defaults.names[0])!
        plain.set("subsonic", forKey: "provider")
        plain.set("https://music.example", forKey: "server")
        for (name, value) in [("user", "me"), ("password", "pw"), ("client-id", ""), ("client-secret", ""), ("refresh-token", "")] {
            items.hold(name, value)
        }

        let read = try store.readProviders()

        #expect(read.account(.subsonic).server == "https://music.example")
        #expect(read.account(.subsonic).user == "me")
        #expect(items.has("download-accounts"))
        for name in ["user", "password", "client-id", "client-secret", "refresh-token"] { #expect(!items.has(name)) }
        #expect(plain.object(forKey: "provider") == nil)
        #expect(plain.object(forKey: "server") == nil)
        #expect(try store.readProviders().account(.subsonic).user == "me")
    }

    @Test func aMigrationTheKeychainCannotKeepLeavesTheOldSecretsInPlace() throws {
        let plain = UserDefaults(suiteName: defaults.names[0])!
        plain.set("subsonic", forKey: "provider")
        items.hold("user", "me")
        items.hold("password", "pw")
        items.script("add", interactionNotAllowed)

        let read = try store.readProviders()

        #expect(read.account(.subsonic).user == "me")
        #expect(items.value("user") == "me")
        #expect(plain.string(forKey: "provider") == "subsonic")
    }

    @Test func aKeychainThatCannotBeReadFailsTheReadInsteadOfReadingEmptyAccounts() {
        items.script("copy", interactionNotAllowed)

        #expect(throws: KeychainFailure.self) { try store.readProviders() }
        #expect(throws: KeychainFailure.self) { try store.readSync() }
    }

    @Test func syncStartsOnAndNearbyOffWithNothingKept() throws {
        let read = try store.readSync()

        #expect(read.enabled)
        #expect(!read.nearbyOnly)
        #expect(read.key == "")
        #expect(read.server == "")
        #expect(!read.favourites && !read.playlists && !read.everything && !read.meteredAllowed)
    }

    @Test func syncWrittenIsReadBackWithItsKeyInTheKeychainAndTheRestInTheDefaults() throws {
        let config = SyncConfig(
            enabled: false,
            nearbyOnly: true,
            server: "https://sync.example",
            key: "pair-me",
            device: "Kitchen phone",
            favourites: true,
            playlists: true,
            everything: false,
            meteredAllowed: true
        )

        try store.writeSync(config)

        #expect(try store.readSync() == config)
        #expect(items.value("sync-key") == "pair-me")
        #expect(UserDefaults(suiteName: defaults.names[0])?.string(forKey: "sync-key") == nil)
    }

    @Test func scrobblingAndTheKeysRoundTripThroughTheKeychain() throws {
        let scrobbling = ScrobbleAccount(listenbrainzToken: "lb", lastfmKey: "k", lastfmSecret: "s", lastfmSession: "x")

        try store.writeScrobbling(scrobbling)
        try store.writeRecognitionKey("acoustid")
        try store.writeAuddToken("audd")

        #expect(try store.readScrobbling() == scrobbling)
        #expect(try store.readRecognitionKey() == "acoustid")
        #expect(try store.readAuddToken() == "audd")
        #expect(items.value("listenbrainz") == "lb")
    }
}
