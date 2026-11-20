package org.resonate.android.ui.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.R
import org.resonate.android.nativecore.SuggestedList
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.icon
import org.resonate.android.ui.library.Loaded
import org.resonate.android.ui.library.Retry
import org.resonate.android.ui.library.catalogRead
import org.resonate.android.ui.library.icon
import org.resonate.android.ui.library.rememberExplorePlaylists
import org.resonate.android.ui.library.valueOrNull
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.screen
import org.resonate.android.ui.suggestions

private val CARD_LEAST = 168.dp

class SuggestionsViewModel(resonate: Resonate) : ViewModel() {
    val retry = Retry()

    val lists: StateFlow<Loaded<List<SuggestedList>>> =
        resonate
            .catalogRead(retry) { native -> native.suggestions() }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), Loaded.Loading)
}

@Composable
fun SuggestionsScreen() {
    val model = resonateViewModel { SuggestionsViewModel(it) }
    val loaded by model.lists.collectAsStateWithLifecycle()
    val explore by LocalResonate.current.features.explore.collectAsStateWithLifecycle()
    val online by LocalResonate.current.features.online.collectAsStateWithLifecycle()
    var exploreRetry by remember { mutableIntStateOf(0) }
    val exploreFeed = if (explore) rememberExplorePlaylists(exploreRetry) else null
    val navigator = LocalNavigator.current
    val scheme = MaterialTheme.colorScheme
    val palettes =
        listOf(
            scheme.primaryContainer to scheme.onPrimaryContainer,
            scheme.secondaryContainer to scheme.onSecondaryContainer,
            scheme.surfaceContainerHighest to scheme.onSurface,
        )
    val shelves = loaded.valueOrNull().orEmpty().groupBy { it.shelfName }
    Page(stringResource(R.string.suggestions_title)) { padding ->
        val direction = LocalLayoutDirection.current
        val fontScale = LocalDensity.current.fontScale
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = CARD_LEAST * fontScale.coerceIn(1f, 1.6f)),
                modifier =
                    Modifier.widthIn(max = PageWidth)
                        .fillMaxWidth()
                        .fillMaxHeight()
                        .testTag("suggestions"),
                contentPadding =
                    PaddingValues(
                        start = padding.calculateStartPadding(direction) + Sizes.gutter,
                        end = padding.calculateEndPadding(direction) + Sizes.gutter,
                        top = padding.calculateTopPadding() + 8.dp,
                        bottom = padding.calculateBottomPadding(),
                    ),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                item(span = { GridItemSpan(maxLineSpan) }) {
                    Text(
                        stringResource(R.string.suggestions_intro),
                        style = MaterialTheme.typography.bodyMedium,
                        color = scheme.onSurfaceVariant,
                        modifier = Modifier.padding(bottom = 4.dp),
                    )
                }
                item(key = "favourites") {
                    SuggestionCard(
                        key = "favourites",
                        title = stringResource(R.string.suggestions_favourites),
                        subtitle = stringResource(R.string.suggestions_favourites_detail),
                        icon = Icons.Rounded.Favorite,
                        container = palettes[0].first,
                        content = palettes[0].second,
                    ) {
                        navigator.open(Screen.Favourites)
                    }
                }
                if (loaded is Loaded.Failed) {
                    item(key = "unread", span = { GridItemSpan(maxLineSpan) }) {
                        TextButton(onClick = model.retry::again) {
                            Text(stringResource(R.string.suggestions_unread))
                        }
                    }
                }
                val shelfEntries = shelves.entries.toList()
                val placeExploreAfter = if (shelfEntries.isNotEmpty()) 0 else -1
                if (explore && placeExploreAfter < 0) {
                    item(key = "explore-shelf", span = { GridItemSpan(maxLineSpan) }) {
                        Text(
                            stringResource(R.string.explore_title),
                            style = MaterialTheme.typography.titleMedium,
                            modifier = Modifier.padding(top = 8.dp),
                        )
                    }
                    val playlists = exploreFeed?.valueOrNull()?.playlists.orEmpty()
                    items(playlists.size, key = { "explore:${playlists[it].id}" }) { index ->
                        val playlist = playlists[index]
                        val (container, content) = palettes[playlist.kind.ordinal % palettes.size]
                        SuggestionCard(
                            key = "explore-${playlist.id}",
                            title = playlist.name,
                            subtitle = playlist.detail,
                            icon = playlist.kind.icon,
                            container = container,
                            content = content,
                        ) {
                            navigator.open(Screen.ExplorePlaylist(playlist.id, playlist.name))
                        }
                    }
                    val message =
                        when {
                            !online -> R.string.explore_offline
                            exploreFeed is Loaded.Loading -> R.string.explore_loading
                            exploreFeed is Loaded.Failed -> R.string.explore_failed
                            exploreFeed?.valueOrNull()?.services == 0u -> R.string.explore_services
                            playlists.isEmpty() -> R.string.explore_empty
                            else -> null
                        }
                    if (message != null) {
                        item(key = "explore-status", span = { GridItemSpan(maxLineSpan) }) {
                            Column {
                                Text(
                                    stringResource(message),
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = scheme.onSurfaceVariant,
                                )
                                if (exploreFeed is Loaded.Failed || (exploreFeed?.valueOrNull()?.failed ?: 0u) > 0u) {
                                    TextButton(onClick = { exploreRetry++ }) {
                                        Text(stringResource(R.string.explore_retry))
                                    }
                                }
                            }
                        }
                    }
                }
                shelfEntries.forEachIndexed { shelfIndex, (shelf, lists) ->
                    item(key = "shelf:$shelf", span = { GridItemSpan(maxLineSpan) }) {
                        Text(
                            shelf,
                            style = MaterialTheme.typography.titleMedium,
                            modifier = Modifier.padding(top = 8.dp),
                        )
                    }
                    items(lists.size, key = { lists[it].id }) { index ->
                        val list = lists[index]
                        val (container, content) = palettes[list.reason.ordinal % palettes.size]
                        SuggestionCard(
                            key = list.reason.name,
                            title = list.name,
                            subtitle = list.detail,
                            icon = list.reason.icon,
                            container = container,
                            content = content,
                        ) {
                            navigator.open(list.screen)
                        }
                    }
                    if (explore && shelfIndex == placeExploreAfter) {
                        item(key = "explore-shelf", span = { GridItemSpan(maxLineSpan) }) {
                            Text(
                                stringResource(R.string.explore_title),
                                style = MaterialTheme.typography.titleMedium,
                                modifier = Modifier.padding(top = 8.dp),
                            )
                        }
                        val playlists = exploreFeed?.valueOrNull()?.playlists.orEmpty()
                        items(playlists.size, key = { "explore:${playlists[it].id}" }) { index ->
                            val playlist = playlists[index]
                            val (container, content) = palettes[playlist.kind.ordinal % palettes.size]
                            SuggestionCard(
                                key = "explore-${playlist.id}",
                                title = playlist.name,
                                subtitle = playlist.detail,
                                icon = playlist.kind.icon,
                                container = container,
                                content = content,
                            ) {
                                navigator.open(Screen.ExplorePlaylist(playlist.id, playlist.name))
                            }
                        }
                        val message =
                            when {
                                !online -> R.string.explore_offline
                                exploreFeed is Loaded.Loading -> R.string.explore_loading
                                exploreFeed is Loaded.Failed -> R.string.explore_failed
                                exploreFeed?.valueOrNull()?.services == 0u -> R.string.explore_services
                                playlists.isEmpty() -> R.string.explore_empty
                                else -> null
                            }
                        if (message != null) {
                            item(key = "explore-status", span = { GridItemSpan(maxLineSpan) }) {
                                Column {
                                    Text(
                                        stringResource(message),
                                        style = MaterialTheme.typography.bodyMedium,
                                        color = scheme.onSurfaceVariant,
                                    )
                                    if (exploreFeed is Loaded.Failed || (exploreFeed?.valueOrNull()?.failed ?: 0u) > 0u) {
                                        TextButton(onClick = { exploreRetry++ }) {
                                            Text(stringResource(R.string.explore_retry))
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun SuggestionCard(
    key: String,
    title: String,
    subtitle: String,
    icon: ImageVector,
    container: Color,
    content: Color,
    onClick: () -> Unit,
) {
    Card(
        onClick = onClick,
        shape = MaterialTheme.shapes.extraLarge,
        colors = CardDefaults.cardColors(containerColor = container, contentColor = content),
        modifier =
            Modifier.fillMaxWidth()
                .heightIn(min = 176.dp)
                .testTag("mix-${key.lowercase()}"),
    ) {
        Column(Modifier.padding(16.dp)) {
            Surface(
                color = content,
                contentColor = container,
                shape = MaterialTheme.shapes.large,
                modifier = Modifier.size(48.dp),
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Icon(icon, contentDescription = null)
                }
            }
            Spacer(Modifier.height(20.dp))
            Text(title, style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(4.dp))
            Text(subtitle, style = MaterialTheme.typography.bodyMedium)
        }
    }
}
