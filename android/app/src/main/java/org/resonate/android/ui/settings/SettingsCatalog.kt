package org.resonate.android.ui.settings

import java.text.Normalizer
import java.util.Locale

enum class SettingsGroup(val title: String) {
    SOUND("Sound"),
    LIBRARY("Library"),
    ONLINE("Online & services"),
    DEVICES("Devices"),
    APPEARANCE("Appearance"),
    APP("App"),
}

enum class Setting(
    val group: SettingsGroup,
    val title: String,
    val detail: String,
    val inner: List<String> = emptyList(),
    val onRoot: Boolean = true,
) {
    EQUALISER(
        SettingsGroup.SOUND,
        "Equaliser",
        "Shape your sound",
        listOf(
            "Presets",
            "Bands",
            "Add band",
            "Preamp",
            "Fit the preamp",
            "Reset curve",
            "Parametric",
            "Only for this output",
        ),
    ),
    HEADPHONE_CORRECTION(
        SettingsGroup.SOUND,
        "Headphone correction",
        "AutoEq profiles and pasted EqualizerAPO text",
        listOf("AutoEq", "Headphone model", "Import a profile", "EqualizerAPO", "GraphicEQ"),
        onRoot = false,
    ),
    MEASUREMENT(
        SettingsGroup.SOUND,
        "Measure with a microphone",
        "Fit a measurement or a sweep toward a target",
        listOf("Microphone", "Sweep", "Measurement file", "Target curve", "Calibration", "REW"),
        onRoot = false,
    ),
    ROOM_CORRECTION(
        SettingsGroup.SOUND,
        "Room correction",
        "A measured response convolved on every output",
        listOf(
            "Impulse response",
            "Convolution",
            "SOFA",
            "Binaural",
            "Speaker angle",
            "Speaker elevation",
            "Head turned",
        ),
        onRoot = false,
    ),
    LISTENING_TEST(
        SettingsGroup.SOUND,
        "Listening test",
        "A blind ABX test of the curve or a setting",
        listOf("ABX", "Blind test", "Compare"),
        onRoot = false,
    ),
    OUTPUT_CURVES(
        SettingsGroup.SOUND,
        "Curves for other outputs",
        "Curves kept for one output",
        listOf("Forget a curve", "Kept curve"),
        onRoot = false,
    ),
    PROCESSING(
        SettingsGroup.SOUND,
        "Sound processing",
        "ReplayGain, resampling and clip prevention",
        listOf(
            "ReplayGain",
            "Resampling quality",
            "Filter phase",
            "True-peak clip prevention",
            "Lossy restoration",
            "Play DSD at PCM level",
            "Playback buffer",
        ),
    ),
    OUTPUT(
        SettingsGroup.SOUND,
        "Audio output",
        "Choose where your music plays",
        listOf("Speaker", "Headphones", "Bluetooth", "USB DAC", "Route"),
    ),
    FOLDERS(
        SettingsGroup.LIBRARY,
        "Music folders",
        "Where your music comes from",
        listOf("Add a folder", "Use Downloads", "Rescan folders", "Remove folder access"),
    ),
    LIBRARY_TOOLS(
        SettingsGroup.LIBRARY,
        "Library tools",
        "Artwork, details and recognition",
        listOf(
            "Fetch artwork automatically",
            "Only on Wi-Fi",
            "Refresh artwork and details",
            "Look again for missing covers",
            "MusicBrainz",
            "Lyrics",
            "AcoustID key",
            "Recent searches",
            "Search history",
        ),
    ),
    STATISTICS(
        SettingsGroup.LIBRARY,
        "Listening statistics",
        "What you play most",
        listOf(
            "Top songs",
            "Most played albums",
            "Most played artists",
            "Listening by day",
            "Formats",
            "Sample rates",
            "Genres",
            "Decades",
        ),
    ),
    SUGGESTIONS(
        SettingsGroup.LIBRARY,
        "Made for you",
        "Lists from your library and listening",
        listOf(
            "On repeat",
            "More like",
            "Deep cuts",
            "Discover",
            "Old flames",
            "Never heard",
            "Most played",
            "Recently added",
        ),
    ),
    WANTED(
        SettingsGroup.LIBRARY,
        "Saved for later",
        "Recordings you want to find",
        listOf("Wanted", "Missing songs"),
    ),
    ONLINE(
        SettingsGroup.ONLINE,
        "Online features",
        "Artwork, lyrics and music search",
        listOf("Internet", "Offline", "Network"),
    ),
    PROVIDERS(
        SettingsGroup.ONLINE,
        "Download sources",
        "Monochrome, hifi-api, Subsonic and TIDAL",
        listOf("Download provider", "Add a source", "Server address", "Sign in with TIDAL"),
    ),
    SERVER(
        SettingsGroup.ONLINE,
        "Server",
        "Browse and play from your connected server",
        listOf("Subsonic", "Navidrome", "TIDAL", "Monochrome", "Browse", "Stream"),
    ),
    SCROBBLING(
        SettingsGroup.ONLINE,
        "Scrobbling",
        "ListenBrainz and Last.fm",
        listOf("Sign in to Last.fm", "ListenBrainz token", "Loves", "Listening history"),
    ),
    SPOTIFY(
        SettingsGroup.ONLINE,
        "Spotify",
        "Your playlists and liked songs, with an app you register",
        listOf("Client ID", "Sign in to Spotify", "Follow the account", "Move to Resonate once"),
    ),
    SYNC(
        SettingsGroup.DEVICES,
        "Device sync",
        "Your music across your devices",
        listOf(
            "Device sync",
            "Pairing code",
            "Join a group",
            "Sync server",
            "Name other people see",
            "Only this network",
            "Mobile data",
            "Sync now",
        ),
    ),
    APPEARANCE(
        SettingsGroup.APPEARANCE,
        "Appearance",
        "Light, dark and the colour Resonate wears",
        listOf(
            "Theme",
            "Light theme",
            "Dark theme",
            "System theme",
            "Theme and accent colour",
            "Accent colour",
            "Wallpaper colours",
            "Mauve",
            "Blue",
            "Teal",
            "Green",
            "Amber",
            "Peach",
            "Red",
            "Pitch black",
            "High contrast",
            "Player background",
            "Player intensity",
            "Now Playing",
        ),
    ),
    NOTIFICATIONS(
        SettingsGroup.APP,
        "Playback notifications",
        "Controls on the lock screen and in the shade",
        listOf("Permission", "Media controls"),
    ),
    VERSION(SettingsGroup.APP, "Version", "Resonate", listOf("About")),
}

data class SettingMatch(val setting: Setting, val shownDetail: String)

fun searchWords(text: String): List<String> {
    val folded =
        Normalizer.normalize(text, Normalizer.Form.NFD)
            .replace(Regex("\\p{Mn}+"), "")
            .lowercase(Locale.ROOT)
    return folded.split(Regex("[^\\p{L}\\p{N}-]+")).filter { it.isNotEmpty() }
}

private fun queryWords(text: String): List<String> =
    searchWords(text).map { it.replace("-", "") }.filter { it.isNotEmpty() }

private fun indexWords(text: String): Set<String> =
    searchWords(text)
        .flatMap { word -> listOf(word.replace("-", "")) + word.split('-') }
        .filter { it.isNotEmpty() }
        .toSet()

private fun List<String>.allWithin(index: Set<String>): Boolean = all { asked ->
    index.any { it.startsWith(asked) }
}

fun Setting.match(query: String): SettingMatch? {
    val asked = queryWords(query)
    if (asked.isEmpty()) return if (onRoot) SettingMatch(this, detail) else null
    if (asked.allWithin(indexWords("$title $detail"))) return SettingMatch(this, detail)
    inner
        .firstOrNull { asked.allWithin(indexWords(it)) }
        ?.let {
            return SettingMatch(this, it)
        }
    val everything = indexWords(listOf(title, detail, group.title).plus(inner).joinToString(" "))
    return if (asked.allWithin(everything)) SettingMatch(this, detail) else null
}

fun matchingSettings(query: String): List<SettingMatch> =
    Setting.entries.mapNotNull { it.match(query) }
