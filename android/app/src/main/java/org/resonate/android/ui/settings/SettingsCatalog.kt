package org.resonate.android.ui.settings

import androidx.annotation.StringRes
import java.text.Normalizer
import java.util.Locale
import org.resonate.android.R
import org.resonate.android.Words

enum class SettingsGroup(@StringRes val title: Int) {
    SOUND(R.string.settings_group_sound),
    LIBRARY(R.string.settings_group_library),
    ONLINE(R.string.settings_group_online),
    DEVICES(R.string.settings_group_devices),
    APPEARANCE(R.string.settings_group_appearance),
    APP(R.string.settings_group_app),
}

enum class Setting(
    val group: SettingsGroup,
    @StringRes val title: Int,
    @StringRes val detail: Int,
    val inner: List<Int> = emptyList(),
    val names: List<String> = emptyList(),
    val onRoot: Boolean = true,
) {
    EQUALISER(
        SettingsGroup.SOUND,
        R.string.settings_equaliser_title,
        R.string.settings_equaliser_detail,
        listOf(
            R.string.settings_inner_presets,
            R.string.settings_inner_bands,
            R.string.settings_inner_add_band,
            R.string.settings_inner_preamp,
            R.string.settings_inner_fit_the_preamp,
            R.string.settings_inner_reset_curve,
            R.string.settings_inner_parametric,
            R.string.settings_inner_only_for_this_output,
        ),
    ),
    HEADPHONE_CORRECTION(
        SettingsGroup.SOUND,
        R.string.settings_headphone_correction_title,
        R.string.settings_headphone_correction_detail,
        listOf(R.string.settings_inner_headphone_model, R.string.settings_inner_import_a_profile),
        names = listOf("AutoEq", "EqualizerAPO", "GraphicEQ"),
        onRoot = false,
    ),
    MEASUREMENT(
        SettingsGroup.SOUND,
        R.string.settings_measurement_title,
        R.string.settings_measurement_detail,
        listOf(
            R.string.settings_inner_microphone,
            R.string.settings_inner_sweep,
            R.string.settings_inner_measurement_file,
            R.string.settings_inner_target_curve,
            R.string.settings_inner_calibration,
        ),
        names = listOf("REW"),
        onRoot = false,
    ),
    ROOM_CORRECTION(
        SettingsGroup.SOUND,
        R.string.settings_room_correction_title,
        R.string.settings_room_correction_detail,
        listOf(
            R.string.settings_inner_impulse_response,
            R.string.settings_inner_convolution,
            R.string.settings_inner_binaural,
            R.string.settings_inner_speaker_angle,
            R.string.settings_inner_speaker_elevation,
            R.string.settings_inner_head_turned,
        ),
        names = listOf("SOFA"),
        onRoot = false,
    ),
    LISTENING_TEST(
        SettingsGroup.SOUND,
        R.string.settings_listening_test_title,
        R.string.settings_listening_test_detail,
        listOf(R.string.settings_inner_blind_test, R.string.settings_inner_compare),
        names = listOf("ABX"),
        onRoot = false,
    ),
    OUTPUT_CURVES(
        SettingsGroup.SOUND,
        R.string.settings_output_curves_title,
        R.string.settings_output_curves_detail,
        listOf(R.string.settings_inner_forget_a_curve, R.string.settings_inner_kept_curve),
        onRoot = false,
    ),
    PROCESSING(
        SettingsGroup.SOUND,
        R.string.settings_processing_title,
        R.string.settings_processing_detail,
        listOf(
            R.string.settings_inner_resampling_quality,
            R.string.settings_inner_filter_phase,
            R.string.settings_inner_true_peak_clip_prevention,
            R.string.settings_inner_lossy_restoration,
            R.string.settings_inner_play_dsd_at_pcm_level,
            R.string.settings_inner_playback_buffer,
        ),
        names = listOf("ReplayGain"),
    ),
    OUTPUT(
        SettingsGroup.SOUND,
        R.string.settings_output_title,
        R.string.settings_output_detail,
        listOf(
            R.string.settings_inner_speaker,
            R.string.settings_inner_headphones,
            R.string.settings_inner_usb_dac,
            R.string.settings_inner_route,
        ),
        names = listOf("Bluetooth"),
    ),
    FOLDERS(
        SettingsGroup.LIBRARY,
        R.string.settings_folders_title,
        R.string.settings_folders_detail,
        listOf(
            R.string.settings_inner_add_a_folder,
            R.string.settings_inner_use_downloads,
            R.string.settings_inner_rescan_folders,
            R.string.settings_inner_remove_folder_access,
        ),
    ),
    LIBRARY_TOOLS(
        SettingsGroup.LIBRARY,
        R.string.settings_library_tools_title,
        R.string.settings_library_tools_detail,
        listOf(
            R.string.settings_inner_fetch_artwork_automatically,
            R.string.settings_inner_only_on_wi_fi,
            R.string.settings_inner_refresh_artwork_and_details,
            R.string.settings_inner_look_again_for_missing_covers,
            R.string.settings_inner_lyrics,
            R.string.settings_inner_acoustid_key,
            R.string.settings_inner_recent_searches,
            R.string.settings_inner_search_history,
        ),
        names = listOf("MusicBrainz"),
    ),
    STATISTICS(
        SettingsGroup.LIBRARY,
        R.string.settings_statistics_title,
        R.string.settings_statistics_detail,
        listOf(
            R.string.settings_inner_top_songs,
            R.string.settings_inner_most_played_albums,
            R.string.settings_inner_most_played_artists,
            R.string.settings_inner_listening_by_day,
            R.string.settings_inner_formats,
            R.string.settings_inner_sample_rates,
            R.string.settings_inner_genres,
            R.string.settings_inner_decades,
        ),
    ),
    SUGGESTIONS(
        SettingsGroup.LIBRARY,
        R.string.settings_suggestions_title,
        R.string.settings_suggestions_detail,
        listOf(
            R.string.settings_inner_on_repeat,
            R.string.settings_inner_more_like,
            R.string.settings_inner_deep_cuts,
            R.string.settings_inner_discover,
            R.string.settings_inner_old_flames,
            R.string.settings_inner_never_heard,
            R.string.settings_inner_most_played,
            R.string.settings_inner_recently_added,
        ),
    ),
    WANTED(
        SettingsGroup.LIBRARY,
        R.string.settings_wanted_title,
        R.string.settings_wanted_detail,
        listOf(R.string.settings_inner_wanted, R.string.settings_inner_missing_songs),
    ),
    ONLINE(
        SettingsGroup.ONLINE,
        R.string.settings_online_title,
        R.string.settings_online_detail,
        listOf(
            R.string.settings_inner_internet,
            R.string.settings_inner_offline,
            R.string.settings_inner_network,
        ),
    ),
    PROVIDERS(
        SettingsGroup.ONLINE,
        R.string.settings_providers_title,
        R.string.settings_providers_detail,
        listOf(
            R.string.settings_inner_download_provider,
            R.string.settings_inner_add_a_source,
            R.string.settings_inner_server_address,
            R.string.settings_inner_sign_in_with_tidal,
        ),
    ),
    SERVER(
        SettingsGroup.ONLINE,
        R.string.settings_server_title,
        R.string.settings_server_detail,
        listOf(R.string.settings_inner_browse, R.string.settings_inner_stream),
        names = listOf("Subsonic", "Navidrome", "TIDAL", "Monochrome"),
    ),
    SCROBBLING(
        SettingsGroup.ONLINE,
        R.string.settings_scrobbling_title,
        R.string.settings_scrobbling_detail,
        listOf(
            R.string.settings_inner_sign_in_to_last_fm,
            R.string.settings_inner_listenbrainz_token,
            R.string.settings_inner_loves,
            R.string.settings_inner_listening_history,
        ),
    ),
    SPOTIFY(
        SettingsGroup.ONLINE,
        R.string.settings_spotify_title,
        R.string.settings_spotify_detail,
        listOf(
            R.string.settings_inner_client_id,
            R.string.settings_inner_sign_in_to_spotify,
            R.string.settings_inner_follow_the_account,
            R.string.settings_inner_move_to_resonate_once,
        ),
    ),
    SYNC(
        SettingsGroup.DEVICES,
        R.string.settings_sync_title,
        R.string.settings_sync_detail,
        listOf(
            R.string.settings_sync_title,
            R.string.settings_inner_pairing_code,
            R.string.settings_inner_join_a_group,
            R.string.settings_inner_sync_server,
            R.string.settings_inner_sync_stun,
            R.string.settings_inner_sync_certificate,
            R.string.settings_inner_name_other_people_see,
            R.string.settings_inner_only_this_network,
            R.string.settings_inner_mobile_data,
            R.string.settings_inner_sync_now,
        ),
    ),
    APPEARANCE(
        SettingsGroup.APPEARANCE,
        R.string.settings_appearance_title,
        R.string.settings_appearance_detail,
        listOf(
            R.string.settings_inner_theme,
            R.string.settings_inner_light_theme,
            R.string.settings_inner_dark_theme,
            R.string.settings_inner_system_theme,
            R.string.settings_inner_theme_and_accent_colour,
            R.string.settings_inner_accent_colour,
            R.string.settings_inner_wallpaper_colours,
            R.string.appearance_accent_mauve,
            R.string.appearance_accent_blue,
            R.string.appearance_accent_teal,
            R.string.appearance_accent_green,
            R.string.appearance_accent_amber,
            R.string.appearance_accent_peach,
            R.string.appearance_accent_red,
            R.string.settings_inner_pitch_black,
            R.string.settings_inner_high_contrast,
            R.string.settings_inner_player_background,
            R.string.settings_inner_player_intensity,
            R.string.settings_inner_now_playing,
        ),
    ),
    NOTIFICATIONS(
        SettingsGroup.APP,
        R.string.settings_notifications_title,
        R.string.settings_notifications_detail,
        listOf(R.string.settings_inner_permission, R.string.settings_inner_media_controls),
    ),
    VERSION(
        SettingsGroup.APP,
        R.string.settings_version_title,
        R.string.app_name,
        listOf(R.string.settings_inner_about),
    ),
}

data class SettingMatch(val setting: Setting, val shownDetail: String)

fun searchWords(text: String): List<String> {
    val folded =
        Normalizer.normalize(text, Normalizer.Form.NFD)
            .replace(Regex("\\p{M}+"), "")
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

fun Setting.match(words: Words, query: String): SettingMatch? {
    val asked = queryWords(query)
    val shownTitle = words.string(title)
    val shownDetail = words.string(detail)
    if (asked.isEmpty()) return if (onRoot) SettingMatch(this, shownDetail) else null
    if (asked.allWithin(indexWords("$shownTitle $shownDetail"))) {
        return SettingMatch(this, shownDetail)
    }
    val shownInner = inner.map { words.string(it) } + names
    shownInner
        .firstOrNull { asked.allWithin(indexWords(it)) }
        ?.let {
            return SettingMatch(this, it)
        }
    val everything =
        indexWords(
            listOf(shownTitle, shownDetail, words.string(group.title))
                .plus(shownInner)
                .joinToString(" ")
        )
    return if (asked.allWithin(everything)) SettingMatch(this, shownDetail) else null
}

fun matchingSettings(words: Words, query: String): List<SettingMatch> =
    Setting.entries.mapNotNull { it.match(words, query) }
