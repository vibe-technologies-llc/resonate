import Foundation

public enum SettingsGroup: String, CaseIterable, Identifiable, Sendable {
    case sound
    case library
    case online
    case devices
    case appearance
    case app

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .sound: said("settings.group.sound")
        case .library: said("settings.group.library")
        case .online: said("settings.group.online")
        case .devices: said("settings.group.devices")
        case .appearance: said("settings.group.appearance")
        case .app: said("settings.group.app")
        }
    }
}

public enum SettingsOpening: Equatable, Sendable {
    case page(Destination)
    case sleepTimer
    case online
    case version
    case erase
}

public enum SettingsPlace: String, CaseIterable, Identifiable, Sendable {
    case equaliser
    case headphoneCorrection
    case microphoneMeasurement
    case roomCorrection
    case listeningTest
    case otherOutputs
    case processing
    case output
    case sleepTimer
    case folders
    case enrichment
    case online
    case providers
    case server
    case scrobbling
    case spotify
    case sync
    case syncSongs
    case syncConnection
    case syncControl
    case accent
    case background
    case searchHistory
    case notices
    case version
    case erase

    public var id: String { rawValue }

    public var group: SettingsGroup {
        switch self {
        case .equaliser, .headphoneCorrection, .microphoneMeasurement, .roomCorrection, .listeningTest, .otherOutputs,
            .processing, .output, .sleepTimer:
            .sound
        case .folders, .enrichment: .library
        case .online, .providers, .server, .scrobbling, .spotify: .online
        case .sync, .syncSongs, .syncConnection, .syncControl: .devices
        case .accent, .background: .appearance
        case .searchHistory, .notices, .version, .erase: .app
        }
    }

    public var parent: SettingsPlace? {
        switch self {
        case .headphoneCorrection, .microphoneMeasurement, .roomCorrection, .listeningTest, .otherOutputs: .equaliser
        case .syncSongs, .syncConnection, .syncControl: .sync
        default: nil
        }
    }

    public var opening: SettingsOpening {
        switch self {
        case .equaliser: .page(.sound)
        case .headphoneCorrection: .page(.headphoneCorrection)
        case .microphoneMeasurement: .page(.microphoneMeasurement)
        case .roomCorrection: .page(.roomCorrection)
        case .listeningTest: .page(.listeningTest)
        case .otherOutputs: .page(.otherOutputs)
        case .processing: .page(.processing)
        case .output: .page(.output)
        case .sleepTimer: .sleepTimer
        case .folders: .page(.folders)
        case .enrichment: .page(.enrichment)
        case .online: .online
        case .providers: .page(.providers)
        case .server: .page(.server)
        case .scrobbling: .page(.scrobbling)
        case .spotify: .page(.spotify)
        case .sync: .page(.sync)
        case .syncSongs: .page(.syncSongs)
        case .syncConnection: .page(.syncConnection)
        case .syncControl: .page(.syncControl)
        case .accent: .page(.appearance)
        case .background: .page(.background)
        case .searchHistory: .page(.searchHistory)
        case .notices: .page(.notices)
        case .version: .version
        case .erase: .erase
        }
    }

    public var title: String {
        switch self {
        case .equaliser: said("settings.equaliser.title")
        case .headphoneCorrection: said("settings.headphone-correction.title")
        case .microphoneMeasurement: said("settings.microphone-measurement.title")
        case .roomCorrection: said("settings.room-correction.title")
        case .listeningTest: said("settings.listening-test.title")
        case .otherOutputs: said("settings.other-outputs.title")
        case .processing: said("settings.processing.title")
        case .output: said("settings.output.title")
        case .sleepTimer: said("settings.sleep-timer.title")
        case .folders: said("settings.folders.title")
        case .enrichment: said("settings.enrichment.title")
        case .online: said("settings.online.title")
        case .providers: said("settings.providers.title")
        case .server: said("settings.server.title")
        case .scrobbling: said("settings.scrobbling.title")
        case .spotify: "Spotify"
        case .sync: said("settings.sync.title")
        case .syncSongs: said("settings.sync-songs.title")
        case .syncConnection: said("settings.sync-connection.title")
        case .syncControl: said("settings.sync-control.title")
        case .accent: said("settings.accent.title")
        case .background: said("settings.background.title")
        case .searchHistory: said("settings.search-history.title")
        case .notices: said("settings.notices.title")
        case .version: "Resonate"
        case .erase: said("settings.erase.title")
        }
    }

    public var summary: String {
        switch self {
        case .equaliser: said("settings.equaliser.summary")
        case .headphoneCorrection: said("settings.headphone-correction.summary")
        case .microphoneMeasurement: said("settings.microphone-measurement.summary")
        case .roomCorrection: said("settings.room-correction.summary")
        case .listeningTest: said("settings.listening-test.summary")
        case .otherOutputs: said("settings.other-outputs.summary")
        case .processing: said("settings.processing.summary")
        case .output: said("settings.output.summary")
        case .sleepTimer: said("settings.sleep-timer.summary")
        case .folders: said("settings.folders.summary")
        case .enrichment: said("settings.enrichment.summary")
        case .online: said("settings.online.summary")
        case .providers: said("settings.providers.summary")
        case .server: said("settings.server.summary")
        case .scrobbling: said("settings.scrobbling.summary")
        case .spotify: said("settings.spotify.summary")
        case .sync: said("settings.sync.summary")
        case .syncSongs: said("settings.sync-songs.summary")
        case .syncConnection: said("settings.sync-connection.summary")
        case .syncControl: said("settings.sync-control.summary")
        case .accent: said("settings.accent.summary")
        case .background: said("settings.background.summary")
        case .searchHistory: said("settings.search-history.summary")
        case .notices: said("settings.notices.summary")
        case .version: said("settings.version.summary")
        case .erase: said("settings.erase.summary")
        }
    }

    public var symbol: String {
        switch self {
        case .equaliser: "slider.vertical.3"
        case .headphoneCorrection: "headphones"
        case .microphoneMeasurement: "mic"
        case .roomCorrection: "waveform.path"
        case .listeningTest: "ear"
        case .otherOutputs: "hifispeaker.2"
        case .processing: "dial.medium"
        case .output: "hifispeaker"
        case .sleepTimer: "moon"
        case .folders: "folder"
        case .enrichment: "photo.on.rectangle.angled"
        case .online: "globe"
        case .providers: "arrow.down.circle"
        case .server: "server.rack"
        case .scrobbling: "dot.radiowaves.left.and.right"
        case .spotify: "link"
        case .sync: "arrow.triangle.2.circlepath"
        case .syncSongs: "music.note"
        case .syncConnection: "point.3.connected.trianglepath.dotted"
        case .syncControl: "dial.medium"
        case .accent: "paintpalette"
        case .background: "square.2.layers.3d"
        case .searchHistory: "clock.arrow.circlepath"
        case .notices: "bell.badge"
        case .version: "info.circle"
        case .erase: "trash"
        }
    }

    public var inner: [String] {
        switch self {
        case .equaliser:
            [
                said("settings.equaliser.inner.presets"),
                said("settings.equaliser.inner.bands"),
                said("settings.equaliser.inner.add-band"),
                said("settings.equaliser.inner.preamp"),
                said("settings.equaliser.inner.fit-the-preamp"),
                said("settings.equaliser.inner.parametric-curve"),
                said("settings.equaliser.inner.only-for-this-output"),
                said("settings.equaliser.inner.reset-curve"),
            ]
        case .headphoneCorrection:
            [
                said("settings.headphone-correction.inner.autoeq"),
                said("settings.headphone-correction.inner.headphone-model"),
                said("settings.headphone-correction.inner.import-a-profile"),
                said("settings.headphone-correction.inner.paste-a-profile"),
                said("settings.headphone-correction.inner.equalizerapo"),
                said("settings.headphone-correction.inner.graphiceq"),
            ]
        case .microphoneMeasurement:
            [
                said("settings.microphone-measurement.inner.microphone"),
                said("settings.microphone-measurement.inner.measure-your-room"),
                said("settings.microphone-measurement.inner.sweep"),
                said("settings.microphone-measurement.inner.fit-toward-a-target"),
                said("settings.microphone-measurement.inner.harman"),
                said("settings.microphone-measurement.inner.microphone-calibration"),
                said("settings.microphone-measurement.inner.rew-measurement"),
            ]
        case .roomCorrection:
            [
                said("settings.room-correction.inner.impulse-response"),
                "SOFA",
                said("settings.room-correction.inner.binaural"),
                said("settings.room-correction.inner.speaker-pair"),
                said("settings.room-correction.inner.speaker-angle"),
                said("settings.room-correction.inner.speaker-elevation"),
                said("settings.room-correction.inner.head-turned"),
                said("settings.room-correction.inner.convolution"),
            ]
        case .listeningTest:
            [
                "ABX",
                said("settings.listening-test.inner.blind-test"),
                said("settings.listening-test.inner.compare-a-and-b"),
                said("settings.listening-test.inner.restoration"),
                said("settings.listening-test.inner.resampler"),
                said("settings.listening-test.inner.dither"),
            ]
        case .otherOutputs:
            [
                said("settings.other-outputs.inner.bound-curves"),
                said("settings.other-outputs.inner.forget-a-curve"),
                said("settings.other-outputs.inner.curve-for-each-output"),
            ]
        case .processing:
            [
                said("settings.processing.inner.replaygain"),
                said("settings.processing.inner.resampling-quality"),
                said("settings.processing.inner.filter-phase"),
                said("settings.processing.inner.true-peak-clip-prevention"),
                said("settings.processing.inner.crossfeed"),
                said("settings.processing.inner.stereo-image"),
                said("settings.processing.inner.lossy-restoration"),
                said("settings.processing.inner.player-volume"),
            ]
        case .output:
            [
                said("settings.output.inner.route"),
                said("settings.output.inner.bluetooth"),
                "AirPlay",
                said("settings.output.inner.usb-dac"),
                said("settings.output.inner.playback-buffer"),
                said("settings.output.inner.match-the-song-s-sample-rate"),
                said("settings.output.inner.dsd-loudness"),
                said("settings.output.inner.latency"),
                said("settings.output.inner.underruns"),
            ]
        case .sleepTimer:
            [
                said("settings.sleep-timer.inner.pause"),
                said("settings.sleep-timer.inner.minutes"),
                said("settings.sleep-timer.inner.end-of-the-song"),
                said("settings.sleep-timer.inner.end-of-the-queue"),
            ]
        case .folders:
            [
                said("settings.folders.inner.add-music-folder"),
                said("settings.folders.inner.rescan-library"),
                said("settings.folders.inner.remove-access"),
                said("settings.folders.inner.organise"),
                said("settings.folders.inner.files"),
            ]
        case .enrichment:
            [
                said("settings.enrichment.inner.fetch-artwork-automatically"),
                said("settings.enrichment.inner.only-on-wi-fi"),
                said("settings.enrichment.inner.metered-connection"),
                said("settings.enrichment.inner.look-again-for-missing-covers"),
                said("settings.enrichment.inner.musicbrainz"),
                said("settings.enrichment.inner.portraits"),
                said("settings.enrichment.inner.lyrics"),
            ]
        case .online:
            [
                said("settings.online.inner.internet"),
                said("settings.online.inner.offline"),
                said("settings.online.inner.network"),
            ]
        case .providers:
            [
                said("settings.providers.inner.download-provider"),
                said("settings.providers.inner.server-address"),
                said("settings.providers.inner.sign-in-with-tidal"),
                said("settings.providers.inner.hosted-services"),
                said("settings.providers.inner.wait-on-a-metered-connection"),
            ]
        case .server:
            [
                "Subsonic",
                said("settings.server.inner.navidrome"),
                "Jellyfin",
                said("settings.server.inner.emby"),
                "TIDAL",
                "Monochrome",
                said("settings.server.inner.browse"),
                said("settings.server.inner.stream"),
            ]
        case .scrobbling:
            [
                said("settings.scrobbling.inner.listenbrainz-token"),
                said("settings.scrobbling.inner.sign-in-to-last-fm"),
                said("settings.scrobbling.inner.loves"),
                said("settings.scrobbling.inner.listening-history"),
            ]
        case .spotify:
            [
                said("settings.spotify.inner.client-id"),
                said("settings.spotify.inner.sign-in-to-spotify"),
                said("settings.spotify.inner.follow-the-account"),
                said("settings.spotify.inner.move-to-resonate-once"),
            ]
        case .sync:
            [
                said("settings.sync.inner.pairing-code"),
                said("settings.sync.inner.add-a-device"),
                said("settings.sync.inner.sync-now"),
                said("settings.sync.inner.continue-listening"),
                said("settings.sync.inner.remote-control"),
                said("settings.sync.inner.transfers"),
            ]
        case .syncSongs:
            [
                said("settings.sync-songs.inner.favourites"),
                said("settings.sync-songs.inner.kept-playlists"),
                said("settings.sync-songs.inner.everything"),
                said("settings.sync-songs.inner.mobile-data"),
                said("settings.sync-songs.inner.folder-for-synced-songs"),
            ]
        case .syncConnection:
            [
                said("settings.sync-connection.inner.resonate-server"),
                said("settings.sync-connection.inner.your-own-server"),
                said("settings.sync-connection.inner.stun-server"),
                said("settings.sync-connection.inner.server-certificate"),
                said("settings.sync-connection.inner.this-network-only"),
                said("settings.sync-connection.inner.device-name"),
            ]
        case .syncControl:
            [
                said("settings.sync-control.inner.remote-control"),
                said("settings.sync-control.inner.driven-by-other-devices"),
                said("settings.sync-control.inner.play-and-pause-from-elsewhere"),
            ]
        case .accent:
            [
                said("settings.accent.inner.icon"),
                said("settings.accent.inner.tint"),
                said("settings.accent.inner.theme"),
                said("settings.accent.inner.colour"),
            ]
        case .background:
            [
                said("settings.background.inner.resonate-background"),
                said("settings.background.inner.flowing"),
                said("settings.background.inner.strength"),
                said("settings.background.inner.now-playing-intensity"),
                said("settings.background.inner.wallpaper"),
            ]
        case .searchHistory:
            [
                said("settings.search-history.inner.keep-search-history"),
                said("settings.search-history.inner.recent-searches"),
                said("settings.search-history.inner.privacy"),
            ]
        case .notices:
            [
                said("settings.notices.inner.found-new-songs"),
                said("settings.notices.inner.hidden-songs"),
                said("settings.notices.inner.songs-kept-from-your-devices"),
                said("settings.notices.inner.notifications"),
            ]
        case .version:
            [said("settings.version.inner.version"), said("settings.version.inner.about")]
        case .erase:
            [
                said("settings.erase.inner.reset"),
                said("settings.erase.inner.delete-everything"),
                said("settings.erase.inner.start-over"),
            ]
        }
    }

    public var shownUnsearched: Bool {
        parent == nil
    }
}

public struct SettingsMatch: Equatable, Identifiable, Sendable {
    public let place: SettingsPlace
    public let said: String?

    public init(place: SettingsPlace, said: String?) {
        self.place = place
        self.said = said
    }

    public var id: String { place.id }

    public func detail(status: String) -> String {
        let shown = said ?? status
        return place.parent.map { "\($0.title) · \(shown)" } ?? shown
    }
}

public enum SettingsSearch {
    public static func words(_ text: String) -> [String] {
        text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)
            .split { !($0.isLetter || $0.isNumber || $0 == "-") }
            .map(String.init)
    }

    static func asked(_ text: String) -> [String] {
        words(text).map { $0.replacingOccurrences(of: "-", with: "") }.filter { !$0.isEmpty }
    }

    static func indexed(_ text: String) -> Set<String> {
        Set(words(text).flatMap { word in
            [word.replacingOccurrences(of: "-", with: "")] + word.split(separator: "-").map(String.init)
        }.filter { !$0.isEmpty })
    }

    static func within(_ asked: [String], _ index: Set<String>) -> Bool {
        asked.allSatisfy { word in index.contains { $0.hasPrefix(word) } }
    }

    static func ranked(_ place: SettingsPlace, _ asked: [String]) -> (strength: Int, said: String?)? {
        if within(asked, indexed(place.title)) { return (3, nil) }
        if within(asked, indexed("\(place.title) \(place.summary)")) { return (2, nil) }
        if let phrase = place.inner.first(where: { within(asked, indexed($0)) }) { return (2, phrase) }
        let everything = ([place.title, place.summary, place.group.title] + place.inner).joined(separator: " ")
        return within(asked, indexed(everything)) ? (1, nil) : nil
    }

    public static func matching(_ query: String, hiding hidden: Set<SettingsPlace> = []) -> [SettingsMatch] {
        let asked = asked(query)
        let offered = SettingsPlace.allCases.filter { !hidden.contains($0) }
        guard !asked.isEmpty else {
            return offered.filter(\.shownUnsearched).map { SettingsMatch(place: $0, said: nil) }
        }
        let groups = SettingsGroup.allCases
        return offered.enumerated()
            .compactMap { offset, place in ranked(place, asked).map { (offset, place, $0.strength, $0.said) } }
            .sorted { first, second in
                let firstGroup = groups.firstIndex(of: first.1.group) ?? 0
                let secondGroup = groups.firstIndex(of: second.1.group) ?? 0
                if firstGroup != secondGroup { return firstGroup < secondGroup }
                if first.2 != second.2 { return first.2 > second.2 }
                return first.0 < second.0
            }
            .map { SettingsMatch(place: $0.1, said: $0.3) }
    }
}
