package org.resonate.android.ui.player

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.launch
import org.resonate.android.R
import org.resonate.android.rowKey
import org.resonate.android.ui.components.Artwork
import org.resonate.android.ui.components.rememberTitleMaxLines
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.shownTitle

@Composable
fun MiniPlayer(model: PlayerViewModel, expansion: PlayerExpansion, modifier: Modifier = Modifier) {
    val playing by model.playing.collectAsStateWithLifecycle()
    val track = playing?.track
    AnimatedVisibility(
        track != null,
        modifier = modifier,
        enter = expandVertically() + fadeIn(),
        exit = shrinkVertically() + fadeOut(),
    ) {
        val scope = rememberCoroutineScope()
        val windowHeight = LocalConfiguration.current.screenHeightDp.dp
        val short = windowHeight < SHORT_WINDOW_BELOW
        val travel = with(LocalDensity.current) { windowHeight.toPx() }
        var dragged by remember { mutableFloatStateOf(0f) }
        val inset = if (short) SHORT_ARTWORK_INSET else ARTWORK_INSET
        val titleLines = rememberTitleMaxLines()
        val openPlayer = stringResource(R.string.player_open)
        Surface(
            color = MaterialTheme.colorScheme.surfaceContainerHighest,
            shape = RoundedCornerShape(CAPSULE_CORNER),
            modifier =
                Modifier.fillMaxWidth()
                    .padding(horizontal = 12.dp, vertical = if (short) 4.dp else 8.dp)
                    .testTag("mini-player")
                    .draggable(
                        orientation = Orientation.Vertical,
                        state =
                            rememberDraggableState { delta ->
                                dragged -= delta
                                scope.launch { expansion.follow(dragged / travel) }
                            },
                        onDragStarted = { dragged = 0f },
                        onDragStopped = { velocity -> expansion.settle(velocity, dragged) },
                    )
                    .clickable(
                        role = Role.Button,
                        onClickLabel = openPlayer,
                        onClick = expansion::expand,
                    ),
        ) {
            Column {
                Row(
                    Modifier.padding(
                        start = inset,
                        end = 4.dp,
                        top = inset,
                        bottom = if (short) 2.dp else 6.dp,
                    ),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Artwork(
                        track,
                        if (short) 36.dp else 48.dp,
                        shape = RoundedCornerShape(CAPSULE_CORNER - inset),
                    )
                    Spacer(Modifier.width(12.dp))
                    AnimatedContent(
                        track,
                        transitionSpec = { fadeIn() togetherWith fadeOut() },
                        contentKey = { it?.rowKey },
                        modifier = Modifier.weight(1f),
                        label = "miniTitles",
                    ) { shown ->
                        Column {
                            Text(
                                shown?.shownTitle.orEmpty(),
                                style = MaterialTheme.typography.titleSmall,
                                maxLines = titleLines,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Text(
                                shown?.shownArtist.orEmpty(),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = titleLines,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                    IconButton(onClick = model::previous) {
                        Icon(
                            Icons.Rounded.SkipPrevious,
                            contentDescription = stringResource(R.string.player_previous),
                        )
                    }
                    FilledIconButton(onClick = model::toggle) {
                        Icon(
                            if (playing?.active == true) Icons.Rounded.Pause
                            else Icons.Rounded.PlayArrow,
                            contentDescription =
                                stringResource(
                                    if (playing?.active == true) R.string.player_pause
                                    else R.string.player_play
                                ),
                        )
                    }
                    IconButton(onClick = model::next) {
                        Icon(
                            Icons.Rounded.SkipNext,
                            contentDescription = stringResource(R.string.player_next),
                        )
                    }
                }
                MiniProgress(model, playing?.durationMs ?: 0L)
            }
        }
    }
}

@Composable
private fun MiniProgress(model: PlayerViewModel, durationMs: Long) {
    val position by model.position.collectAsStateWithLifecycle()
    val fraction = if (durationMs > 0) (position.toFloat() / durationMs).coerceIn(0f, 1f) else 0f
    val progress = stringResource(R.string.player_song_progress)
    val percent =
        pluralStringResource(
            R.plurals.player_percent,
            (fraction * 100).toInt(),
            (fraction * 100).toInt(),
        )
    Box(
        Modifier.fillMaxWidth().padding(horizontal = 8.dp).padding(bottom = 8.dp).semantics {
            contentDescription = progress
            stateDescription = percent
            progressBarRangeInfo = ProgressBarRangeInfo(fraction, 0f..1f)
        }
    ) {
        LinearProgressIndicator(
            progress = { fraction },
            strokeCap = StrokeCap.Round,
            gapSize = 0.dp,
            trackColor = MaterialTheme.colorScheme.outline,
            drawStopIndicator = {},
            modifier = Modifier.fillMaxWidth().height(3.dp).clip(MaterialTheme.shapes.small),
        )
    }
}

private val SHORT_WINDOW_BELOW = 480.dp
private val CAPSULE_CORNER = 16.dp
private val ARTWORK_INSET = 8.dp
private val SHORT_ARTWORK_INSET = 4.dp
