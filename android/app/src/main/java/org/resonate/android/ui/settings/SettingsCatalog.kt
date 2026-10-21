package org.resonate.android.ui.settings

import java.text.Normalizer
import java.util.Locale
import org.resonate.android.ui.Mix

enum class SettingsGroup(val title: String) {
    SOUND("Sound"),
    LIBRARY("Library"),
    ONLINE("Online & services"),
    APPEARANCE("Appearance"),
    NOTIFICATIONS("Notifications"),
    ABOUT("About"),
}

enum class Setting(
    val group: SettingsGroup,
    val title: String,
    val detail: String,
    val inner: List<String> = emptyList(),
) {
    EQUALISER(
        SettingsGroup.SOUND,
        "Equaliser",
        "Shape your sound",
        listOf("Presets", "Bands", "Preamp", "Import a profile", "Room correction"),
    ),
    HEADPHONE_CORRECTION(
        SettingsGroup.SOUND,
        "Headphone correction",
        "AutoEq profiles for your headphones",
        listOf("AutoEq", "Headphone model"),
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
        "Mixes from your library",
        Mix.entries.map { it.title },
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
        listOf("Download provider", "Server address", "Sign in with TIDAL"),
    ),
    SCROBBLING(
        SettingsGroup.ONLINE,
        "Scrobbling",
        "ListenBrainz and Last.fm",
        listOf("Sign in to Last.fm", "ListenBrainz token", "Loves", "Listening history"),
    ),
    SYNC(
        SettingsGroup.ONLINE,
        "Devices",
        "Your music across your devices",
        listOf(
            "Device sync",
            "Pairing code",
            "Join a group",
            "Sync server",
            "Only this network",
            "Mobile data",
            "Sync now",
        ),
    ),
    APPEARANCE(
        SettingsGroup.APPEARANCE,
        "Theme and accent colour",
        "Light, dark and the colour Resonate wears",
        listOf(
            "Theme",
            "Light theme",
            "Dark theme",
            "System theme",
            "Accent colour",
            "Wallpaper colours",
            "Mauve",
            "Blue",
            "Teal",
            "Green",
            "Amber",
            "Peach",
            "Red",
        ),
    ),
    NOTIFICATIONS(
        SettingsGroup.NOTIFICATIONS,
        "Playback notifications",
        "Controls on the lock screen and in the shade",
        listOf("Permission", "Media controls"),
    ),
    VERSION(SettingsGroup.ABOUT, "Version", "Resonate", listOf("About")),
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
    if (asked.isEmpty()) return SettingMatch(this, detail)
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
