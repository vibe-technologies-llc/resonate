package org.resonate.android.ui.player

import androidx.activity.compose.PredictiveBackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.basicMarquee
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.automirrored.rounded.VolumeDown
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.Bedtime
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.FavoriteBorder
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.Lyrics
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Repeat
import androidx.compose.material.icons.rounded.RepeatOne
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material.icons.automirrored.rounded.VolumeOff
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.material3.FilledTonalToggleButtonDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.launch
import org.resonate.android.ArtworkSizes
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Repeat
import org.resonate.android.ui.clock
import org.resonate.android.ui.components.CommitSlider
import org.resonate.android.ui.components.LocalSheets
import org.resonate.android.ui.components.Picture
import org.resonate.android.ui.components.favourite
import org.resonate.android.ui.components.openAlbumOf
import org.resonate.android.ui.components.openArtistOf
import org.resonate.android.ui.components.rememberArtwork
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.quality
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.shownTitle
import org.resonate.android.ui.theme.ArtworkTheme
import org.resonate.android.ui.theme.rememberSeedOf
import org.resonate.android.ui.tools.LyricsPane

private enum class Pane {
    ARTWORK,
    LYRICS,
}

@Composable
fun NowPlaying(model: PlayerViewModel, expansion: PlayerExpansion) {
    val playing by model.playing.collectAsStateWithLifecycle()
    val track = playing?.track
    val strength =
        LocalResonate.current.features.appearance.appearance
            .collectAsStateWithLifecycle()
            .value
            .backgroundStrength
    val scope = rememberCoroutineScope()
    var height by remember { mutableFloatStateOf(1f) }
    var dragged by remember { mutableFloatStateOf(0f) }
    var queueOpen by rememberSaveable { mutableStateOf(false) }
    val sheets = LocalSheets.current
    var pane by rememberSaveable { mutableStateOf(Pane.ARTWORK) }
    PredictiveBackHandler(enabled = expansion.expanded) { events ->
        try {
            events.collect { event -> expansion.follow(1f - event.progress * BACK_PEEK) }
            expansion.collapse()
        } catch (cancelled: CancellationException) {
            expansion.expand()
            throw cancelled
        }
    }
    val sample = rememberArtwork(track?.id, track?.artRevision ?: 0u, ArtworkSizes.edgeFor(96))
    val seed = rememberSeedOf(sample)
    val dragState = rememberDraggableState { delta ->
        dragged += delta
        scope.launch { expansion.follow(1f - dragged / height) }
    }
    ArtworkTheme(seed) {
        val scheme = MaterialTheme.colorScheme
        Surface(
            color = scheme.surface,
            modifier =
                Modifier.fillMaxSize()
                    .onSizeChanged { height = it.height.toFloat().coerceAtLeast(1f) }
                    .graphicsLayer {
                        val shown = expansion.progress.value
                        translationY = (1f - shown) * size.height
                        alpha = (shown * 3f).coerceAtMost(1f)
                    }
                    .testTag("now-playing"),
        ) {
            Box(
                Modifier.fillMaxSize()
                    .background(
                        Brush.verticalGradient(
                            0f to scheme.primaryContainer.copy(alpha = strength),
                            0.55f to scheme.surfaceContainerLow,
                            1f to scheme.surfaceContainerLow,
                        )
                    )
            ) {
                BoxWithConstraints(Modifier.fillMaxSize().safeDrawingPadding()) {
                    val wide = maxWidth > maxHeight
                    val drag =
                        Modifier.draggable(
                            orientation = Orientation.Vertical,
                            state = dragState,
                            onDragStarted = { dragged = 0f },
                            onDragStopped = { velocity ->
                                if (dragged > height * 0.18f || velocity > 1200f) expansion.collapse()
                                else expansion.expand()
                            },
                        )
                    val artwork: @Composable (Modifier) -> Unit = { slot ->
                        AnimatedContent(
                            pane,
                            transitionSpec = { fadeIn() togetherWith fadeOut() },
                            modifier = slot,
                            label = "player pane",
                        ) { shown ->
                            when {
                                shown == Pane.LYRICS && track != null ->
                                    LyricsPane(track, Modifier.fillMaxSize())
                                else -> Cover(track, Modifier.fillMaxSize())
                            }
                        }
                    }
                    val header: @Composable () -> Unit = {
                        PlayerHeader(
                            track,
                            onClose = expansion::collapse,
                            modifier = drag,
                        )
                    }
                    val controls: @Composable ColumnScope.() -> Unit = {
                        Controls(
                            model,
                            playing,
                            pane = pane,
                            onLyrics = {
                                pane = if (pane == Pane.LYRICS) Pane.ARTWORK else Pane.LYRICS
                            },
                            onQueue = { queueOpen = true },
                        )
                    }
                    if (wide) {
                        Row(Modifier.fillMaxSize().padding(horizontal = 24.dp)) {
                            Column(Modifier.weight(1f).fillMaxSize()) {
                                header()
                                artwork(Modifier.weight(1f).fillMaxWidth().padding(vertical = 12.dp).then(drag))
                            }
                            Spacer(Modifier.width(32.dp))
                            Column(
                                Modifier.weight(1f).fillMaxSize(),
                                verticalArrangement = Arrangement.Center,
                            ) {
                                controls()
                            }
                        }
                    } else {
                        Column(Modifier.fillMaxSize().padding(horizontal = 24.dp)) {
                            header()
                            artwork(
                                Modifier.weight(1f)
                                    .fillMaxWidth()
                                    .padding(vertical = 16.dp)
                                    .then(if (pane == Pane.ARTWORK) drag else Modifier)
                            )
                            controls()
                        }
                    }
                }
            }
        }
        if (queueOpen) QueueSheet(model, onDismiss = { queueOpen = false })
        if (sheets.sleeping) SleepTimerSheet(model, onDismiss = { sheets.sleeping = false })
    }
}

@Composable
private fun Cover(track: Track?, modifier: Modifier) {
    BoxWithConstraints(modifier, contentAlignment = Alignment.Center) {
        val side = minOf(maxWidth, maxHeight).coerceAtMost(COVER_AT_MOST)
        val edge = with(LocalDensity.current) { side.roundToPx() }
        Surface(
            shape = MaterialTheme.shapes.extraLarge,
            shadowElevation = 12.dp,
            modifier = Modifier.size(side),
        ) {
            Picture(
                rememberArtwork(track?.id, track?.artRevision ?: 0u, edge),
                Modifier.fillMaxSize(),
                shape = MaterialTheme.shapes.extraLarge,
            )
        }
    }
}

@Composable
private fun PlayerHeader(track: Track?, onClose: () -> Unit, modifier: Modifier) {
    val sheets = LocalSheets.current
    Row(
        modifier.fillMaxWidth().padding(top = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconButton(onClick = onClose) {
            Icon(Icons.Rounded.KeyboardArrowDown, contentDescription = "Close player")
        }
        Column(Modifier.weight(1f), horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                "Now playing",
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (!track?.album.isNullOrBlank()) {
                Text(
                    track.album,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    textAlign = TextAlign.Center,
                )
            }
        }
        IconButton(
            onClick = { track?.let { sheets.song(it, inPlayer = true) } },
            enabled = track != null,
        ) {
            Icon(Icons.Rounded.MoreVert, contentDescription = "Player options")
        }
    }
}

@Composable
private fun ColumnScope.Controls(
    model: PlayerViewModel,
    playing: Playing?,
    pane: Pane,
    onLyrics: () -> Unit,
    onQueue: () -> Unit,
) {
    val track = playing?.track
    val resonate = LocalResonate.current
    val navigator = LocalNavigator.current
    val sheets = LocalSheets.current
    val expansion = LocalPlayerExpansion.current
    val scope = rememberCoroutineScope()
    Row(verticalAlignment = Alignment.CenterVertically) {
        AnimatedContent(
            track,
            transitionSpec = { fadeIn() togetherWith fadeOut() },
            contentKey = { it?.id },
            modifier = Modifier.weight(1f),
            label = "player titles",
        ) { shown ->
            Column {
                Text(
                    shown?.shownTitle.orEmpty(),
                    style = MaterialTheme.typography.headlineSmall,
                    maxLines = 1,
                    modifier =
                        Modifier.clickable(
                                enabled = shown != null && shown.album.isNotBlank(),
                                onClickLabel = "Go to album",
                            ) {
                                shown?.let {
                                    expansion.collapse()
                                    navigator.openAlbumOf(it)
                                }
                            }
                            .basicMarquee(),
                )
                Text(
                    shown?.shownArtist.orEmpty(),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier =
                        Modifier.clickable(enabled = shown != null, onClickLabel = "Go to artist") {
                            shown?.let {
                                expansion.collapse()
                                navigator.openArtistOf(it)
                            }
                        },
                )
            }
        }
        val favourite = track?.favourite == true
        IconToggleButton(
            checked = favourite,
            onCheckedChange = { enabled ->
                track?.let { resonate.favourite(it, enabled, sheets, scope) }
            },
            enabled = track != null,
        ) {
            Icon(
                if (favourite) Icons.Rounded.Favorite else Icons.Rounded.FavoriteBorder,
                contentDescription = if (favourite) "Remove from favourites" else "Add to favourites",
                tint = if (favourite) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
    Spacer(Modifier.height(12.dp))
    Scrubber(model, playing)
    Spacer(Modifier.height(8.dp))
    Transport(model, playing)
    Spacer(Modifier.height(16.dp))
    Volume(model, playing)
    Spacer(Modifier.height(8.dp))
    PlayerActions(model, pane, playing, onLyrics, onQueue)
    Spacer(Modifier.height(8.dp))
}

@Composable
private fun Scrubber(model: PlayerViewModel, playing: Playing?) {
    val position by model.position.collectAsStateWithLifecycle()
    val duration = playing?.durationMs ?: 0L
    var draft by remember { mutableStateOf<Float?>(null) }
    val shown = (draft ?: position.toFloat()).coerceIn(0f, duration.coerceAtLeast(1L).toFloat())
    CommitSlider(
        value = shown,
        onCommit = { model.seek(it.toLong()) },
        onDraft = { draft = it },
        range = 0f..duration.coerceAtLeast(1L).toFloat(),
        enabled = duration > 0,
        modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Song progress" },
        trackHeight = 4.dp,
    )
    val times = MaterialTheme.typography.labelMedium.copy(fontFeatureSettings = "tnum")
    Row(verticalAlignment = Alignment.Top) {
        Text(clock(shown.toLong()), style = times, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Box(Modifier.weight(1f), contentAlignment = Alignment.TopCenter) {
            val line = playing?.waiting ?: playing?.track?.quality.orEmpty()
            if (line.isNotEmpty()) {
                Surface(
                    color = MaterialTheme.colorScheme.secondaryContainer,
                    shape = RoundedCornerShape(50),
                ) {
                    Row(
                        Modifier.padding(horizontal = 10.dp, vertical = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(
                            if (playing?.waiting != null) Icons.AutoMirrored.Rounded.VolumeOff else Icons.Rounded.GraphicEq,
                            null,
                            modifier = Modifier.size(14.dp),
                        )
                        Spacer(Modifier.width(6.dp))
                        Text(
                            line,
                            style = MaterialTheme.typography.labelSmall,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
        }
        Text(
            "-" + clock((duration - shown.toLong()).coerceAtLeast(0)),
            style = times,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun Transport(model: PlayerViewModel, playing: Playing?) {
    val active = playing?.active == true
    val corner by animateDpAsState(if (active) 20.dp else 32.dp, label = "play shape")
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        IconToggleButton(
            checked = playing?.shuffle == true,
            onCheckedChange = model::shuffle,
            colors = IconButtonDefaults.iconToggleButtonColors(checkedContentColor = MaterialTheme.colorScheme.primary),
        ) {
            Icon(Icons.Rounded.Shuffle, contentDescription = "Shuffle")
        }
        IconButton(onClick = model::previous, modifier = Modifier.size(56.dp)) {
            Icon(Icons.Rounded.SkipPrevious, contentDescription = "Previous", modifier = Modifier.size(32.dp))
        }
        FilledIconButton(
            onClick = model::toggle,
            shape = RoundedCornerShape(corner),
            modifier = Modifier.size(width = 72.dp, height = 64.dp).testTag("play-pause"),
        ) {
            Icon(
                if (active) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                contentDescription = if (active) "Pause" else "Play",
                modifier = Modifier.size(32.dp),
            )
        }
        IconButton(onClick = model::next, modifier = Modifier.size(56.dp)) {
            Icon(Icons.Rounded.SkipNext, contentDescription = "Next", modifier = Modifier.size(32.dp))
        }
        val repeat = playing?.repeat ?: Repeat.OFF
        IconToggleButton(
            checked = repeat != Repeat.OFF,
            onCheckedChange = {
                model.repeat(
                    when (repeat) {
                        Repeat.OFF -> Repeat.ALL
                        Repeat.ALL -> Repeat.ONE
                        Repeat.ONE -> Repeat.OFF
                    }
                )
            },
            colors = IconButtonDefaults.iconToggleButtonColors(checkedContentColor = MaterialTheme.colorScheme.primary),
        ) {
            Icon(
                if (repeat == Repeat.ONE) Icons.Rounded.RepeatOne else Icons.Rounded.Repeat,
                contentDescription =
                    when (repeat) {
                        Repeat.OFF -> "Repeat off"
                        Repeat.ALL -> "Repeat all"
                        Repeat.ONE -> "Repeat one"
                    },
            )
        }
    }
}

@Composable
private fun Volume(model: PlayerViewModel, playing: Playing?) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(
            Icons.AutoMirrored.Rounded.VolumeDown,
            null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(20.dp),
        )
        CommitSlider(
            value = playing?.volume ?: 1f,
            onCommit = model::volume,
            modifier = Modifier.weight(1f).padding(horizontal = 8.dp).semantics { contentDescription = "Volume" },
            trackHeight = 4.dp,
        )
        Icon(
            Icons.AutoMirrored.Rounded.VolumeUp,
            null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(20.dp),
        )
    }
}

@Composable
private fun PlayerActions(
    model: PlayerViewModel,
    pane: Pane,
    playing: Playing?,
    onLyrics: () -> Unit,
    onQueue: () -> Unit,
) {
    val sheets = LocalSheets.current
    val remaining by model.sleepRemaining.collectAsStateWithLifecycle()
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
    ) {
        ActionToggle(Icons.Rounded.Lyrics, "Lyrics", pane == Pane.LYRICS, onLyrics, Modifier.weight(1f))
        ActionToggle(Icons.AutoMirrored.Rounded.QueueMusic, "Queue", false, onQueue, Modifier.weight(1f))
        if (playing?.sleep != null) {
            ActionToggle(
                Icons.Rounded.Bedtime,
                remaining?.let { clock(it) } ?: "Sleep",
                true,
                { sheets.sleeping = true },
                Modifier.weight(1f),
                description = "Sleep timer",
            )
        }
    }
}

@Composable
private fun ActionToggle(
    icon: ImageVector,
    label: String,
    checked: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    description: String = label,
) {
    ToggleButton(
        checked = checked,
        onCheckedChange = { onClick() },
        colors = FilledTonalToggleButtonDefaults.colors(),
        modifier = modifier.widthIn(min = 48.dp).semantics { contentDescription = description },
    ) {
        Icon(icon, null, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(8.dp))
        Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.labelLarge.copy(fontFeatureSettings = "tnum"))
    }
}

private val COVER_AT_MOST: Dp = 520.dp

private const val BACK_PEEK = 0.2f
