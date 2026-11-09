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
        case .sound: "Sound"
        case .library: "Library"
        case .online: "Online & services"
        case .devices: "Devices"
        case .appearance: "Appearance"
        case .app: "App"
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
        case .equaliser: "Equaliser"
        case .headphoneCorrection: "Headphone correction"
        case .microphoneMeasurement: "Measure with a microphone"
        case .roomCorrection: "Room correction"
        case .listeningTest: "Listening test"
        case .otherOutputs: "Curves for other outputs"
        case .processing: "Sound processing"
        case .output: "Audio output"
        case .sleepTimer: "Sleep timer"
        case .folders: "Music folders"
        case .enrichment: "Artwork and details"
        case .online: "Online features"
        case .providers: "Download sources"
        case .server: "Server"
        case .scrobbling: "Scrobbling"
        case .spotify: "Spotify"
        case .sync: "Device sync"
        case .syncSongs: "Songs to sync"
        case .syncConnection: "Sync connection"
        case .syncControl: "Driven by your other devices"
        case .accent: "Accent colour"
        case .background: "Background"
        case .searchHistory: "Search history"
        case .notices: "Notices"
        case .version: "Resonate"
        case .erase: "Erase all data"
        }
    }

    public var summary: String {
        switch self {
        case .equaliser: "Shape your sound"
        case .headphoneCorrection: "Search AutoEq or paste a profile"
        case .microphoneMeasurement: "Fit a measurement or a sweep toward a target"
        case .roomCorrection: "No response chosen"
        case .listeningTest: "A blind ABX test of the curve or a setting"
        case .otherOutputs: "Curves kept for one output each"
        case .processing: "ReplayGain, resampling and clip prevention"
        case .output: "Choose a device and adjust buffering"
        case .sleepTimer: "Pause playback after a while"
        case .folders: "Where your music comes from"
        case .enrichment: "Covers, portraits, lyrics and MusicBrainz details"
        case .online: "MusicBrainz, artwork, lyrics, AutoEq and recognition"
        case .providers: "Monochrome, hifi-api, Subsonic, Jellyfin and TIDAL"
        case .server: "Browse and play from your connected server"
        case .scrobbling: "ListenBrainz and Last.fm"
        case .spotify: "Your playlists and liked songs, with an app you register"
        case .sync: "Your music across your devices"
        case .syncSongs: "Which songs follow you to this iPhone"
        case .syncConnection: "How this iPhone reaches your devices"
        case .syncControl: "What your other devices may do here"
        case .accent: "The colour Resonate and its icon wear"
        case .background: "The field behind every page"
        case .searchHistory: "Recent searches"
        case .notices: "What the library tells you"
        case .version: "Version"
        case .erase: "Start over on this iPhone"
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
        case .background: "sparkles.rectangle.stack"
        case .searchHistory: "clock.arrow.circlepath"
        case .notices: "bell.badge"
        case .version: "info.circle"
        case .erase: "trash"
        }
    }

    public var inner: [String] {
        switch self {
        case .equaliser:
            ["Presets", "Bands", "Add band", "Preamp", "Fit the preamp", "Parametric curve", "Only for this output", "Reset curve"]
        case .headphoneCorrection:
            ["AutoEq", "Headphone model", "Import a profile", "Paste a profile", "EqualizerAPO", "GraphicEQ"]
        case .microphoneMeasurement:
            ["Microphone", "Measure your room", "Sweep", "Fit toward a target", "Harman", "Microphone calibration", "REW measurement"]
        case .roomCorrection:
            ["Impulse response", "SOFA", "Binaural", "Speaker pair", "Speaker angle", "Speaker elevation", "Head turned", "Convolution"]
        case .listeningTest:
            ["ABX", "Blind test", "Compare A and B", "Restoration", "Resampler", "Dither"]
        case .otherOutputs:
            ["Bound curves", "Forget a curve", "Curve for each output"]
        case .processing:
            ["ReplayGain", "Resampling quality", "Filter phase", "True-peak clip prevention", "Crossfeed", "Stereo image", "Lossy restoration", "Player volume"]
        case .output:
            ["Route", "Bluetooth", "AirPlay", "USB DAC", "Playback buffer", "Match the song's sample rate", "DSD loudness", "Latency", "Underruns"]
        case .sleepTimer:
            ["Pause", "Minutes", "End of the song", "End of the queue"]
        case .folders:
            ["Add music folder", "Rescan library", "Remove access", "Organise", "Files"]
        case .enrichment:
            ["Fetch artwork automatically", "Only on Wi-Fi", "Metered connection", "Look again for missing covers", "MusicBrainz", "Portraits", "Lyrics"]
        case .online:
            ["Internet", "Offline", "Network"]
        case .providers:
            ["Download provider", "Server address", "Sign in with TIDAL", "Hosted services", "Wait on a metered connection"]
        case .server:
            ["Subsonic", "Navidrome", "Jellyfin", "Emby", "TIDAL", "Monochrome", "Browse", "Stream"]
        case .scrobbling:
            ["ListenBrainz token", "Sign in to Last.fm", "Loves", "Listening history"]
        case .spotify:
            ["Client ID", "Sign in to Spotify", "Follow the account", "Move to Resonate once"]
        case .sync:
            ["Pairing code", "Add a device", "Sync now", "Continue listening", "Remote control", "Transfers"]
        case .syncSongs:
            ["Favourites", "Kept playlists", "Everything", "Mobile data", "Folder for synced songs"]
        case .syncConnection:
            ["Resonate server", "Your own server", "This network only", "Device name"]
        case .syncControl:
            ["Remote control", "Driven by other devices", "Play and pause from elsewhere"]
        case .accent:
            ["Icon", "Tint", "Theme", "Colour"]
        case .background:
            ["Resonate background", "Flowing", "Strength", "Now Playing intensity", "Wallpaper"]
        case .searchHistory:
            ["Keep search history", "Recent searches", "Privacy"]
        case .notices:
            ["Found new songs", "Hidden songs", "Songs kept from your devices", "Notifications"]
        case .version:
            ["Version", "About"]
        case .erase:
            ["Reset", "Delete everything", "Start over"]
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
