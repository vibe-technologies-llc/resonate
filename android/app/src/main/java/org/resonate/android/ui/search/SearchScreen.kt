package org.resonate.android.ui.search

import org.resonate.android.ui.components.LocalSheets
import androidx.compose.material.icons.rounded.Hearing
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.BookmarkBorder
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material.icons.rounded.Public
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SearchBarDefaults
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.nativecore.SearchSong
import org.resonate.android.ui.components.SettingsAction
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen
import org.resonate.android.ui.resonateViewModel

val ContentWidth = 840.dp

private const val CROWDED_FONT_SCALE = 1.5f

@Composable
fun SearchScreen() {
    val model = resonateViewModel { SearchViewModel(it) }
    val trail by model.trail.collectAsStateWithLifecycle()
    val typed by model.typed.collectAsStateWithLifecycle()
    val onlinePage = trail.nested || isLink(typed)
    val libraryGrid = rememberLazyListState()
    val onlineScroll = remember { OnlineScroll() }
    var acting by remember { mutableStateOf<SearchSong?>(null) }
    var choosing by remember { mutableStateOf<List<SearchSong>?>(null) }
    val keyboard = LocalSoftwareKeyboardController.current
    val focus = LocalFocusManager.current
    val scrolled = if (onlinePage) onlineScroll.scrolled else libraryGrid.canScrollBackward
    val scrolling by remember {
        derivedStateOf { libraryGrid.isScrollInProgress || onlineScroll.moving }
    }
    LaunchedEffect(scrolling) {
        if (scrolling) {
            keyboard?.hide()
            focus.clearFocus()
        }
    }
    BackHandler(enabled = trail.nested) { model.back() }

    Scaffold(
        containerColor = MaterialTheme.colorScheme.surface,
        contentWindowInsets = WindowInsets.safeDrawing.only(WindowInsetsSides.Horizontal),
        topBar = { SearchHeader(model, scrolled) },
    ) { padding ->
        AnimatedContent(
            onlinePage,
            transitionSpec = { fadeIn(tween(220)) togetherWith fadeOut(tween(120)) },
            label = "search place",
            modifier = Modifier.fillMaxSize(),
        ) { shown ->
            val content = PaddingValues(top = padding.calculateTopPadding(), bottom = 16.dp)
            if (shown) {
                OnlineResults(
                    model = model,
                    scroll = onlineScroll,
                    padding = content,
                    act = { acting = it },
                )
            } else {
                LibraryResults(model, libraryGrid, content, act = { acting = it })
            }
        }
    }

    acting?.let { song ->
        OnlineSongSheet(
            song = song,
            held = song.held,
            dismiss = { acting = null },
            actions =
                onlineSongActions(
                    save = { model.save(song) },
                    download = { model.downloads.request(listOf(song)) { choosing = listOf(song) } },
                    album =
                        song.releaseGroup
                            .takeIf { it.isNotBlank() }
                            ?.let { group ->
                                { model.open(OnlineView.Editions(group, song.album, song.artist)) }
                            },
                    artist =
                        song.artistId
                            .takeIf { it.isNotBlank() }
                            ?.let { id -> { model.open(OnlineView.Discography(id, song.artist)) } },
                ),
        )
    }
    choosing?.let { songs -> DownloadChooser(songs, model.downloads) { choosing = null } }
}

@Composable
private fun SearchHeader(model: SearchViewModel, scrolled: Boolean) {
    val navigator = LocalNavigator.current
    val library by model.libraryShown.collectAsStateWithLifecycle()
    val container by
        animateColorAsState(
            if (scrolled) MaterialTheme.colorScheme.surfaceContainer
            else MaterialTheme.colorScheme.surface,
            label = "search header",
        )
    val busy = (library as? LibraryShown.Answered)?.asking == true
    Surface(color = container) {
        Column(
            Modifier.fillMaxWidth()
                .windowInsetsPadding(
                    WindowInsets.safeDrawing.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal)
                ),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Column(Modifier.widthIn(max = ContentWidth).fillMaxWidth()) {
                Row(
                    Modifier.padding(start = Sizes.gutter, end = 4.dp, top = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    SearchField(model, Modifier.weight(1f))
                    IconButton(
                        onClick = LocalSheets.current::listen,
                        modifier = Modifier.testTag("listen"),
                    ) {
                        Icon(Icons.Rounded.Hearing, contentDescription = "Listen")
                    }
                    SettingsAction()
                }
            }
            Box(Modifier.fillMaxWidth().height(4.dp)) {
                if (busy) {
                    LinearProgressIndicator(Modifier.fillMaxWidth().testTag("search-busy"))
                }
            }
        }
    }
}

@Composable
private fun SearchField(model: SearchViewModel, modifier: Modifier) {
    val typed by model.typed.collectAsStateWithLifecycle()
    val keyboard = LocalSoftwareKeyboardController.current
    val scheme = MaterialTheme.colorScheme
    TextField(
        state = model.field,
        modifier = modifier.heightIn(min = 56.dp).testTag("search-field"),
        placeholder = {
            Text(
                "Songs, albums, artists or a link",
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        },
        leadingIcon = { Icon(Icons.Rounded.Search, contentDescription = null) },
        trailingIcon =
            if (typed.isNotEmpty() || model.field.text.isNotEmpty()) {
                {
                    IconButton(onClick = model::clear, modifier = Modifier.testTag("search-clear")) {
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
