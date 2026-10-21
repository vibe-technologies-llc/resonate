package org.resonate.android.ui.settings

import android.content.Context
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.BarChart
import androidx.compose.material.icons.rounded.BookmarkBorder
import androidx.compose.material.icons.rounded.Clear
import androidx.compose.material.icons.rounded.CloudDownload
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.Headphones
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material.icons.rounded.Notifications
import androidx.compose.material.icons.rounded.Palette
import androidx.compose.material.icons.rounded.Public
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material.icons.rounded.SearchOff
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.core.app.NotificationManagerCompat
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.TitleSize
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel

class SettingsViewModel(private val resonate: Resonate) : ViewModel() {
    val online = resonate.features.online
    val appearance = resonate.features.appearance.appearance

    fun setOnline(enabled: Boolean) = resonate.features.setOnline(enabled)

    fun allowNotifications() = resonate.host.allowNotifications()
}

private val Setting.icon: ImageVector
    get() =
        when (this) {
            Setting.EQUALISER -> Icons.Rounded.Equalizer
            Setting.HEADPHONE_CORRECTION -> Icons.Rounded.Headphones
            Setting.PROCESSING -> Icons.Rounded.GraphicEq
            Setting.OUTPUT -> Icons.Rounded.Speaker
            Setting.FOLDERS -> Icons.Rounded.FolderOpen
            Setting.LIBRARY_TOOLS -> Icons.Rounded.LibraryMusic
            Setting.STATISTICS -> Icons.Rounded.BarChart
            Setting.SUGGESTIONS -> Icons.Rounded.AutoAwesome
            Setting.WANTED -> Icons.Rounded.BookmarkBorder
            Setting.ONLINE -> Icons.Rounded.Public
            Setting.PROVIDERS -> Icons.Rounded.CloudDownload
            Setting.SCROBBLING -> Icons.Rounded.History
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
            Setting.PROCESSING -> Screen.Processing
            Setting.OUTPUT -> Screen.Output
            Setting.FOLDERS -> Screen.Folders
            Setting.LIBRARY_TOOLS -> Screen.LibraryTools
            Setting.STATISTICS -> Screen.Statistics
            Setting.SUGGESTIONS -> Screen.Suggestions
            Setting.WANTED -> Screen.Wanted
            Setting.PROVIDERS -> Screen.Providers
            Setting.SCROBBLING -> Screen.Scrobbling
            Setting.SYNC -> Screen.Sync
            Setting.APPEARANCE -> Screen.Appearance
            Setting.ONLINE,
            Setting.NOTIFICATIONS,
            Setting.VERSION -> null
        }

private fun notificationsAllowed(context: Context) =
    NotificationManagerCompat.from(context).areNotificationsEnabled()

private fun versionOf(context: Context): String =
    try {
        context.packageManager.getPackageInfo(context.packageName, 0).versionName ?: "Unknown"
    } catch (failure: Exception) {
        "Unknown"
    }

@Composable
fun SettingsScreen() {
    val model = resonateViewModel { SettingsViewModel(it) }
    val navigator = LocalNavigator.current
    val context = LocalContext.current
    val online by model.online.collectAsStateWithLifecycle()
    val appearance by model.appearance.collectAsStateWithLifecycle()
    var query by rememberSaveable { mutableStateOf("") }
    var notifications by remember { mutableStateOf(notificationsAllowed(context)) }
    val version = remember { versionOf(context) }
    LifecycleResumeEffect(Unit) {
        notifications = notificationsAllowed(context)
        onPauseOrDispose {}
    }
    val matches = remember(query) { matchingSettings(query) }
    Page("Settings", size = TitleSize.LARGE) { padding ->
        SettingsList(padding, Modifier.testTag("settings-list")) {
            item(key = "search") { SettingsSearch(query, { query = it }) }
            if (matches.isEmpty()) {
                item(key = "nothing") {
                    EmptyState(
                        Icons.Rounded.SearchOff,
                        "No settings found",
                        detail = "Try sound, folders or the name of a service.",
                        modifier = Modifier.padding(top = 32.dp),
                    )
                }
            }
            SettingsGroup.entries.forEach { group ->
                val shown = matches.filter { it.setting.group == group }
                if (shown.isEmpty()) return@forEach
                item(key = "group-${group.name}") { SectionHeader(group.title) }
                items(shown, key = { it.setting.name }) { match ->
                    val setting = match.setting
                    val detail = match.shownDetail.takeIf { query.isNotBlank() }
                    when (setting) {
                        Setting.ONLINE ->
                            SwitchRow(
                                title = setting.title,
                                icon = setting.icon,
                                supporting =
                                    detail
                                        ?: if (online) "Artwork, lyrics and music search are on"
                                        else "Resonate stays offline",
                                checked = online,
                                onCheckedChange = model::setOnline,
                                modifier = Modifier.testTag("setting-online"),
                            )
                        Setting.APPEARANCE ->
                            SettingRow(
                                title = setting.title,
                                icon = setting.icon,
                                supporting =
                                    detail ?: "${appearance.mode.label} · ${appearance.accentLabel}",
                                modifier = Modifier.testTag("setting-appearance"),
                                onClick = { setting.destination?.let(navigator::open) },
                            )
                        Setting.NOTIFICATIONS ->
                            SettingRow(
                                title = setting.title,
                                icon = setting.icon,
                                supporting =
                                    if (notifications) "Allowed" else "Not allowed yet",
                                trailing =
                                    if (notifications) null
                                    else {
                                        {
                                            FilledTonalButton(
                                                onClick = model::allowNotifications,
                                                modifier = Modifier.testTag("allow-notifications"),
                                            ) {
                                                Text("Allow")
                                            }
                                        }
                                    },
                            )
                        Setting.VERSION ->
                            SettingRow(
                                title = "Resonate",
                                icon = setting.icon,
                                supporting = "Version $version",
                            )
                        else ->
                            SettingRow(
                                title = setting.title,
                                icon = setting.icon,
                                supporting = detail ?: setting.detail,
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
        placeholder = { Text("Search settings") },
        leadingIcon = { Icon(Icons.Rounded.Search, contentDescription = null) },
        trailingIcon =
            if (query.isEmpty()) null
            else {
                {
                    IconButton(onClick = { onQuery("") }) {
                        Icon(Icons.Rounded.Clear, contentDescription = "Clear search")
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
