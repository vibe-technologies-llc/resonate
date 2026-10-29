package org.resonate.android.ui.server

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SearchBarDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.nativecore.ServedFound
import org.resonate.android.nativecore.ServedService
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.SettingsAction
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.TitleSize
import org.resonate.android.ui.library.LoadedContent
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.resonateViewModel

@Composable
fun ServerScreen() {
    val model = resonateViewModel { ServerViewModel(it) }
    val actions = rememberServerActions()
    val services by model.services.collectAsStateWithLifecycle()
    val chosen by model.chosen.collectAsStateWithLifecycle()
    val firstPage by model.firstPage.collectAsStateWithLifecycle()
    val found by model.found.collectAsStateWithLifecycle()
    val title = chosen?.title.orEmpty()
    Page("Server", size = TitleSize.LARGE, back = false, actions = { SettingsAction() }) { padding ->
        val direction = LocalLayoutDirection.current
        val below =
            PaddingValues(
                start = padding.calculateStartPadding(direction),
                end = padding.calculateEndPadding(direction),
                bottom = padding.calculateBottomPadding(),
            )
        Column(Modifier.fillMaxSize().padding(top = padding.calculateTopPadding())) {
            ServerSearchField(model, title)
            if (services.size > 1) {
                ServiceChooser(services, chosen, model::choose)
            }
            val asked = found
            if (asked == null) {
                LoadedContent(firstPage, model.firstPageRetry::again) { shown ->
                    FoundList(shown, below, actions, "served-first-page") {
                        EmptyState(
                            Icons.Rounded.Search,
                            searchHeadline(title),
                            detail = firstPageEmpty(title),
                        )
                    }
                }
            } else {
                LoadedContent(asked, model.searchRetry::again) { shown ->
                    FoundList(shown, below, actions, "served-found") {
                        EmptyState(Icons.Rounded.Search, "Nothing matches", detail = nothingMatches(title))
                    }
                }
            }
        }
    }
}

@Composable
private fun ServiceChooser(
    services: List<ServedService>,
    chosen: ServedService?,
    onChoose: (ServedService) -> Unit,
) {
    Row(
        Modifier.fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = Sizes.gutter, vertical = 4.dp)
            .selectableGroup()
            .testTag("served-services"),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        services.forEach { service ->
            FilterChip(
                selected = service.scheme == chosen?.scheme,
                onClick = { onChoose(service) },
                label = { Text(service.title) },
                modifier = Modifier.testTag("served-service-${service.scheme}"),
            )
        }
    }
}

@Composable
private fun FoundList(
    found: ServedFound,
    padding: PaddingValues,
    actions: ServerActions,
    tag: String,
    empty: @Composable () -> Unit,
) {
    val navigator = LocalNavigator.current
    if (found.isEmpty()) {
        empty()
        return
    }
    LazyColumn(contentPadding = padding, modifier = Modifier.fillMaxSize().testTag(tag)) {
        item("summary") { SectionHeader(foundSummary(found)) }
        if (found.artists.isNotEmpty()) {
            item("artists") { SectionHeader("Artists") }
            items(found.artists, key = { "artist:${it.uri}" }) { artist ->
                ServedArtistRow(artist, onClick = { navigator.open(artist.screen()) }, actions = actions)
            }
        }
        if (found.albums.isNotEmpty()) {
            item("albums") { SectionHeader("Albums") }
            items(found.albums, key = { "album:${it.uri}" }) { album ->
                ServedAlbumRow(album, onClick = { navigator.open(album.screen()) }, actions = actions)
            }
        }
        if (found.songs.isNotEmpty()) {
            item("songs") { SectionHeader("Songs") }
            items(found.songs.size, key = { "song:${found.songs[it].uri}" }) { index ->
                val song = found.songs[index]
                ServedSongRow(song, onClick = { actions.playFrom(found.songs, index) }, actions = actions)
            }
        }
    }
}

@Composable
private fun ServerSearchField(model: ServerViewModel, title: String) {
    val keyboard = LocalSoftwareKeyboardController.current
    val scheme = MaterialTheme.colorScheme
    val typed by model.typed.collectAsStateWithLifecycle()
    TextField(
        state = model.field,
        modifier =
            Modifier.fillMaxWidth()
                .padding(horizontal = Sizes.gutter, vertical = 8.dp)
                .heightIn(min = 56.dp)
                .testTag("served-search-field"),
        placeholder = { Text(searchHeadline(title), maxLines = 1, overflow = TextOverflow.Ellipsis) },
        leadingIcon = { Icon(Icons.Rounded.Search, contentDescription = null) },
        trailingIcon =
            if (typed) {
                {
                    IconButton(onClick = model::clear, modifier = Modifier.testTag("served-search-clear")) {
                        Icon(Icons.Rounded.Close, contentDescription = "Clear search")
                    }
                }
            } else null,
        lineLimits = TextFieldLineLimits.SingleLine,
        shape = SearchBarDefaults.inputFieldShape,
        colors =
            TextFieldDefaults.colors(
                focusedContainerColor = scheme.surfaceContainerHigh,
                unfocusedContainerColor = scheme.surfaceContainerHigh,
                focusedIndicatorColor = Color.Transparent,
                unfocusedIndicatorColor = Color.Transparent,
                disabledIndicatorColor = Color.Transparent,
            ),
        keyboardOptions =
            KeyboardOptions(
                capitalization = KeyboardCapitalization.None,
                autoCorrectEnabled = false,
                imeAction = ImeAction.Search,
            ),
        onKeyboardAction = {
            model.submit()
            keyboard?.hide()
        },
    )
}
