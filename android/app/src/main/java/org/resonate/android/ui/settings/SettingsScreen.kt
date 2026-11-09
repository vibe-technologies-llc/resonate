package org.resonate.android.ui.settings

import android.content.Context
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.BookmarkBorder
import androidx.compose.material.icons.rounded.Clear
import androidx.compose.material.icons.rounded.Cloud
import androidx.compose.material.icons.rounded.CloudDownload
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.Headphones
import androidx.compose.material.icons.rounded.Hearing
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.BarChart
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material.icons.rounded.Link
import androidx.compose.material.icons.rounded.Mic
import androidx.compose.material.icons.rounded.Notifications
import androidx.compose.material.icons.rounded.Palette
import androidx.compose.material.icons.rounded.Public
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.SearchOff
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.SpeakerGroup
import androidx.compose.material.icons.rounded.SurroundSound
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.app.NotificationManagerCompat
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.PlaybackPreferences
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.TitleSize
import org.resonate.android.ui.equaliser.keptForOthers
import org.resonate.android.ui.equaliser.shownName
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.words

class SettingsViewModel(private val resonate: Resonate) : ViewModel() {
    val online = resonate.features.online
    val appearance = resonate.features.appearance.appearance
    val serving = resonate.features.served.serving
    val preferences = resonate.library.preferences
    val route = resonate.library.route
    val bound = resonate.library.boundRoutes

    fun setOnline(enabled: Boolean) = resonate.features.setOnline(enabled)

    fun allowNotifications() = resonate.host.allowNotifications()
}

private val Setting.icon: ImageVector
    get() =
        when (this) {
            Setting.EQUALISER -> Icons.Rounded.Equalizer
            Setting.HEADPHONE_CORRECTION -> Icons.Rounded.Headphones
            Setting.MEASUREMENT -> Icons.Rounded.Mic
            Setting.ROOM_CORRECTION -> Icons.Rounded.SurroundSound
            Setting.LISTENING_TEST -> Icons.Rounded.Hearing
            Setting.OUTPUT_CURVES -> Icons.Rounded.SpeakerGroup
            Setting.PROCESSING -> Icons.Rounded.GraphicEq
            Setting.OUTPUT -> Icons.Rounded.Speaker
            Setting.FOLDERS -> Icons.Rounded.FolderOpen
            Setting.LIBRARY_TOOLS -> Icons.Rounded.LibraryMusic
            Setting.STATISTICS -> Icons.Rounded.BarChart
            Setting.SUGGESTIONS -> Icons.AutoMirrored.Rounded.QueueMusic
            Setting.WANTED -> Icons.Rounded.BookmarkBorder
            Setting.ONLINE -> Icons.Rounded.Public
            Setting.PROVIDERS -> Icons.Rounded.CloudDownload
            Setting.SERVER -> Icons.Rounded.Cloud
            Setting.SCROBBLING -> Icons.Rounded.History
            Setting.SPOTIFY -> Icons.Rounded.Link
            Setting.SYNC -> Icons.Rounded.Devices
            Setting.APPEARANCE -> Icons.Rounded.Palette
            Setting.NOTIFICATIONS -> Icons.Rounded.Notifications
            Setting.VERSION -> Icons.Rounded.Info
        }

private val Setting.destination: Screen?
    get() =
        when (this) {
            Setting.EQUALISER -> Screen.Equaliser
            Setting.HEADPHONE_CORRECTION -> Screen.HeadphoneCorrection
            Setting.MEASUREMENT -> Screen.Measurement
            Setting.ROOM_CORRECTION -> Screen.RoomCorrection
            Setting.LISTENING_TEST -> Screen.ListeningTest
            Setting.OUTPUT_CURVES -> Screen.OutputCurves
            Setting.PROCESSING -> Screen.Processing
            Setting.OUTPUT -> Screen.Output
            Setting.FOLDERS -> Screen.Folders
            Setting.LIBRARY_TOOLS -> Screen.LibraryTools
            Setting.STATISTICS -> Screen.Statistics
            Setting.SUGGESTIONS -> Screen.Suggestions
            Setting.WANTED -> Screen.Wanted
            Setting.PROVIDERS -> Screen.Providers
            Setting.SERVER -> Screen.Server
            Setting.SCROBBLING -> Screen.Scrobbling
            Setting.SPOTIFY -> Screen.Spotify
            Setting.SYNC -> Screen.Sync
            Setting.APPEARANCE -> Screen.Appearance
            Setting.ONLINE,
            Setting.NOTIFICATIONS,
            Setting.VERSION -> null
        }

fun equaliserStatus(words: Words, preferences: PlaybackPreferences?): String =
    when {
        preferences == null -> words.string(Setting.EQUALISER.detail)
        preferences.equaliserEnabled ->
            words.string(R.string.settings_equaliser_on, shownName(words, preferences.equaliser))
        else -> words.string(R.string.settings_equaliser_off)
    }

private fun notificationsAllowed(context: Context) =
    NotificationManagerCompat.from(context).areNotificationsEnabled()

private fun versionOf(context: Context): String =
    try {
        context.packageManager.getPackageInfo(context.packageName, 0).versionName
            ?: context.getString(R.string.settings_version_unknown)
    } catch (failure: Exception) {
        context.getString(R.string.settings_version_unknown)
    }

@Composable
fun SettingsScreen() {
    val model = resonateViewModel { SettingsViewModel(it) }
    val navigator = LocalNavigator.current
    val context = LocalContext.current
    val words = words()
    val online by model.online.collectAsStateWithLifecycle()
    val appearance by model.appearance.collectAsStateWithLifecycle()
    val serving by model.serving.collectAsStateWithLifecycle()
    val preferences by model.preferences.collectAsStateWithLifecycle()
    val route by model.route.collectAsStateWithLifecycle()
    val bound by model.bound.collectAsStateWithLifecycle()
    val othersKept = keptForOthers(route, bound).isNotEmpty()
    var query by rememberSaveable { mutableStateOf("") }
    var notifications by remember { mutableStateOf(notificationsAllowed(context)) }
    val version = remember { versionOf(context) }
    LifecycleResumeEffect(Unit) {
        notifications = notificationsAllowed(context)
        onPauseOrDispose {}
    }
    val matches =
        remember(query, serving, othersKept, words) {
            matchingSettings(words, query).filter { match ->
                when (match.setting) {
                    Setting.SERVER -> serving
                    Setting.OUTPUT_CURVES -> othersKept
                    else -> true
                }
            }
        }
    Page(stringResource(R.string.settings_title), size = TitleSize.LARGE) { padding ->
        SettingsList(padding, Modifier.testTag("settings-list")) {
            item(key = "search") { SettingsSearch(query, { query = it }) }
            if (matches.isEmpty()) {
                item(key = "nothing") {
                    EmptyState(
                        Icons.Rounded.SearchOff,
                        stringResource(R.string.settings_search_nothing),
                        detail = stringResource(R.string.settings_search_nothing_detail),
                        modifier = Modifier.padding(top = 32.dp),
                    )
                }
            }
            SettingsGroup.entries.forEach { group ->
                val shown = matches.filter { it.setting.group == group }
                if (shown.isEmpty()) return@forEach
                item(key = "group-${group.name}") { SectionHeader(stringResource(group.title)) }
                items(shown, key = { it.setting.name }) { match ->
                    val setting = match.setting
                    val detail = match.shownDetail.takeIf { query.isNotBlank() }
                    val title = stringResource(setting.title)
                    val ownDetail = stringResource(setting.detail)
                    val equaliser = stringResource(Setting.EQUALISER.title)
                    when (setting) {
                        Setting.ONLINE ->
                            SwitchRow(
                                title = title,
                                icon = setting.icon,
                                supporting =
                                    detail
                                        ?: if (online) stringResource(R.string.settings_online_on)
                                        else stringResource(R.string.settings_online_off),
                                checked = online,
                                onCheckedChange = model::setOnline,
                                modifier = Modifier.testTag("setting-online"),
                            )
                        Setting.APPEARANCE ->
                            SettingRow(
                                title = title,
                                icon = setting.icon,
                                supporting =
                                    detail
                                        ?: listOf(
                                                stringResource(appearance.mode.label),
                                                stringResource(appearance.accentLabel),
                                            )
                                            .joinToString(" · "),
                                modifier = Modifier.testTag("setting-appearance"),
                                onClick = { setting.destination?.let(navigator::open) },
                            )
                        Setting.NOTIFICATIONS ->
                            SettingRow(
                                title = title,
                                icon = setting.icon,
                                supporting =
                                    if (notifications) {
                                        stringResource(R.string.settings_notifications_allowed)
                                    } else {
                                        stringResource(R.string.settings_notifications_not_allowed)
                                    },
                                trailing =
                                    if (notifications) null
                                    else {
                                        {
                                            FilledTonalButton(
                                                onClick = model::allowNotifications,
                                                modifier = Modifier.testTag("allow-notifications"),
                                            ) {
                                                Text(
                                                    stringResource(
                                                        R.string.settings_notifications_allow
                                                    )
                                                )
                                            }
                                        }
                                    },
                            )
                        Setting.VERSION ->
                            SettingRow(
                                title = stringResource(R.string.app_name),
                                icon = setting.icon,
                                supporting = stringResource(R.string.settings_version, version),
                            )
                        Setting.EQUALISER ->
                            SettingRow(
                                title = title,
                                icon = setting.icon,
                                supporting = detail ?: equaliserStatus(words, preferences),
                                modifier = Modifier.testTag("setting-equaliser"),
                                onClick = { setting.destination?.let(navigator::open) },
                            )
                        Setting.OUTPUT ->
                            SettingRow(
                                title = title,
                                icon = setting.icon,
                                supporting = detail ?: route?.name ?: ownDetail,
                                modifier = Modifier.testTag("setting-output"),
                                onClick = { setting.destination?.let(navigator::open) },
                            )
                        else ->
                            SettingRow(
                                title = title,
                                icon = setting.icon,
                                supporting =
                                    (detail ?: ownDetail).let {
                                        if (setting.onRoot) it else "$equaliser · $it"
                                    },
                                modifier = Modifier.testTag("setting-${setting.name.lowercase()}"),
                                onClick = { setting.destination?.let(navigator::open) },
                            )
                    }
                }
            }
        }
    }
}

@Composable
private fun SettingsSearch(query: String, onQuery: (String) -> Unit) {
    TextField(
        value = query,
        onValueChange = onQuery,
        placeholder = { Text(stringResource(R.string.settings_search)) },
        leadingIcon = { Icon(Icons.Rounded.Search, contentDescription = null) },
        trailingIcon =
            if (query.isEmpty()) null
            else {
                {
                    IconButton(onClick = { onQuery("") }) {
                        Icon(
                            Icons.Rounded.Clear,
                            contentDescription = stringResource(R.string.settings_search_clear),
                        )
                    }
                }
            },
        singleLine = true,
        shape = CircleShape,
        colors =
            TextFieldDefaults.colors(
                focusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                unfocusedContainerColor = MaterialTheme.colorScheme.surfaceContainerHigh,
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
            ),
        modifier =
            Modifier.fillMaxWidth()
                .padding(horizontal = Sizes.gutter, vertical = 8.dp)
                .testTag("settings-search"),
    )
}
