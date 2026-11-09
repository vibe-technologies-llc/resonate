package org.resonate.android.ui.tools

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.DragInteraction
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Lyrics
import androidx.compose.material.icons.rounded.MyLocation
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.BlurredEdgeTreatment
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import org.resonate.android.R
import org.resonate.android.nativecore.LyricSheet
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.rememberCalm

private const val FOLLOW_PAUSE_MS = 5_000L
private const val FOLLOWED_AT = 0.35f
const val QUIET_ALPHA = 0.85f
private const val RESTING_SCALE = 0.94f

@Composable
fun LyricsPane(track: Track, modifier: Modifier = Modifier) {
    LyricsView(track, MaterialTheme.typography.headlineSmall, modifier)
}

@Composable
fun LyricsView(track: Track, style: TextStyle, modifier: Modifier = Modifier) {
    val resonate = LocalResonate.current
    val features = resonate.features
    val sheet by features.lyrics.collectAsStateWithLifecycle()
    val loading by features.lyricsLoading.collectAsStateWithLifecycle()
    val unread by features.lyricsUnread.collectAsStateWithLifecycle()
    val online by features.online.collectAsStateWithLifecycle()
    var held by remember(track.id) { mutableStateOf<LyricSheet?>(null) }
    val current = sheet?.takeIf { it.trackId == track.id }
    val mine = current ?: held
    LaunchedEffect(current) {
        if (current != null) held = current
    }
    LaunchedEffect(track.id, sheet?.trackId, loading, unread) {
        if (!loading && mine == null && unread != track.id && features.lyrics.value?.trackId != track.id)
            features.loadLyrics(track.id)
    }
    DisposableEffect(Unit) {
        features.watchLyrics(true)
        onDispose { features.watchLyrics(false) }
    }
    val shown = mine?.takeIf { it.lines.isNotEmpty() }
    Box(modifier.testTag("lyrics"), contentAlignment = Alignment.Center) {
        when {
            loading && shown == null -> LoadingIndicator()
            shown == null && unread == track.id ->
                NoLyrics(
                    stringResource(R.string.lyrics_unread),
                    detail = stringResource(R.string.lyrics_unread_detail),
                    action = stringResource(R.string.lyrics_try_again),
                    onAction = { features.loadLyrics(track.id, refresh = true) },
                )
            shown == null ->
                NoLyrics(
                    stringResource(R.string.lyrics_none),
                    detail =
                        stringResource(
                            if (online) R.string.lyrics_none_online
                            else R.string.lyrics_none_offline
                        ),
                    action = if (online) stringResource(R.string.lyrics_search_again) else null,
                    onAction = { features.loadLyrics(track.id, refresh = true) },
                )
            else -> LyricLines(track, shown, style)
        }
    }
}

@Composable
private fun NoLyrics(title: String, detail: String, action: String?, onAction: () -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.Center,
    ) {
        EmptyState(
            Icons.Rounded.Lyrics,
            title,
            detail = detail,
            action = action,
            onAction = onAction,
            verticalPadding = 0.dp,
        )
    }
}

@Composable
private fun LyricLines(track: Track, sheet: LyricSheet, style: TextStyle) {
    val resonate = LocalResonate.current
    val snapshot by resonate.library.snapshot.collectAsStateWithLifecycle()
    val frame by resonate.features.lyricFrame.collectAsStateWithLifecycle()
    val playing = snapshot?.current?.id == track.id
    val calm = rememberCalm()
    val listState = rememberLazyListState()
    var touching by remember(track.id) { mutableStateOf(false) }
    LaunchedEffect(listState) {
        listState.interactionSource.interactions.collectLatest { interaction ->
            when (interaction) {
                is DragInteraction.Start -> touching = true
                is DragInteraction.Stop,
                is DragInteraction.Cancel -> {
                    delay(FOLLOW_PAUSE_MS)
                    touching = false
                }
            }
        }
    }
    val reading = FrameReading(sheet, frame.takeIf { playing }, following = playing)
    val followed = reading.followed
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val height = constraints.maxHeight
        val density = LocalDensity.current
        val lead = with(density) { (height * FOLLOWED_AT).toDp() }
        LaunchedEffect(followed, touching, height) {
            if (followed != null && !touching && height > 0) {
                if (calm.reduced) listState.scrollToItem(followed)
                else listState.animateScrollToItem(followed)
            }
        }
        LazyColumn(
            state = listState,
            contentPadding =
                PaddingValues(
                    top = if (reading.unsynced || !playing) 24.dp else lead,
                    bottom = with(density) { (height * 0.5f).toDp() },
                ),
            modifier = Modifier.fillMaxSize().fadedEdges().testTag("lyric-lines"),
        ) {
            items(sheet.lines.size, key = { it }) { index ->
                val row = sheet.lines[index]
                val look = reading.look(index)
                val seekAt = reading.seekAt(index)
                LyricLine(
                    text = row.text,
                    blank = row.blank,
                    right = rightVoice(row.voice),
                    look = look,
                    style = style,
                    blurred = !calm.reduced && !calm.spoken,
                    reduced = calm.reduced,
                    onSeek = seekAt?.let { at -> { resonate.playback.seek(at) } },
                )
            }
            item(key = "origin") {
                Text(
                    sheet.origin,
                    style = MaterialTheme.typography.bodySmall,
                    color = LocalContentColor.current.copy(alpha = 0.6f),
                    modifier = Modifier.padding(top = 32.dp, bottom = 16.dp),
                )
            }
        }
        AnimatedVisibility(
            visible = touching && followed != null,
            enter = fadeIn() + scaleIn(),
            exit = fadeOut() + scaleOut(),
            modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 16.dp),
        ) {
            FilledTonalButton(
                onClick = { touching = false },
                contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                modifier = Modifier.testTag("follow-lyrics"),
            ) {
                Icon(Icons.Rounded.MyLocation, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                Text(stringResource(R.string.lyrics_follow))
            }
        }
    }
}

@Composable
private fun LyricLine(
    text: String,
    blank: Boolean,
    right: Boolean,
    look: LineLook,
    style: TextStyle,
    blurred: Boolean,
    reduced: Boolean,
    onSeek: (() -> Unit)?,
) {
    val lit = look.tone == LineTone.LIT
    val plain = look.tone == LineTone.PLAIN
    val spec = if (reduced) snap<Float>() else tween<Float>(350)
    val alpha by
        animateFloatAsState(if (lit || plain) 1f else QUIET_ALPHA, spec, label = "lineAlpha")
    val scale by
        animateFloatAsState(if (lit || plain) 1f else RESTING_SCALE, spec, label = "lineScale")

    val playFromHere = stringResource(R.string.lyrics_play_from_line)
    val currentLine = stringResource(R.string.lyrics_current_line)
    val ink = LocalContentColor.current
    val clickable =
        if (onSeek != null)
            Modifier.clickable(
                role = Role.Button,
                onClickLabel = playFromHere,
                onClick = onSeek,
            )
        else Modifier
    Box(
        Modifier.fillMaxWidth()
            .clip(MaterialTheme.shapes.large)
            .then(clickable)
            .semantics(mergeDescendants = true) { if (lit) stateDescription = currentLine }

            .padding(vertical = if (blank) 4.dp else 10.dp, horizontal = 8.dp)
    ) {
        Column(
            Modifier.fillMaxWidth(),
            horizontalAlignment = if (right) Alignment.End else Alignment.Start,
        ) {
            look.breath?.let { Breath(it, ink) }
            if (!blank) {
                Text(
                    wiped(text, look.spans, ink),
                    style = style.copy(fontWeight = if (lit || plain) FontWeight.Bold else FontWeight.Medium),
                    color = ink,
                    textAlign = if (right) TextAlign.End else TextAlign.Start,
                    modifier =
                        Modifier.graphicsLayer {
                                this.alpha = alpha
                                scaleX = scale
                                scaleY = scale
                                transformOrigin = TransformOrigin(if (right) 1f else 0f, 0.5f)
                            }
                            .then(
                                if (blurred && look.tone == LineTone.DISTANT)
                                    Modifier.blur(1.5.dp, BlurredEdgeTreatment.Unbounded)
                                else Modifier
                            ),
                )
            }
        }
    }
}

@Composable
private fun Breath(through: Float, ink: Color) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.padding(bottom = 8.dp),
    ) {
        repeat(3) { dot ->
            val filled = ((through * 3) - dot).coerceIn(0f, 1f)
            Surface(
                color = ink.copy(alpha = QUIET_ALPHA + (1 - QUIET_ALPHA) * filled),
                shape = CircleShape,
                modifier = Modifier.size(10.dp + 2.dp * filled),
            ) {}
        }
    }
}

fun wiped(text: String, spans: WipeSpans?, ink: Color): AnnotatedString {
    if (spans == null) return AnnotatedString(text)
    val dim = ink.copy(alpha = QUIET_ALPHA)
    return buildAnnotatedString {
        withStyle(SpanStyle(color = ink)) { append(text.substring(0, spans.sung)) }
        withStyle(SpanStyle(color = dim)) { append(text.substring(spans.sung, spans.blendStart)) }
        withStyle(SpanStyle(color = lerp(dim, ink, spans.share))) {
            append(text.substring(spans.blendStart, spans.blendEnd))
        }
        withStyle(SpanStyle(color = dim)) { append(text.substring(spans.blendEnd)) }
    }
}

private fun Modifier.fadedEdges(): Modifier =
    graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
        .drawWithContent {
            drawContent()
            drawRect(
                Brush.verticalGradient(
                    0f to Color.Transparent,
                    0.08f to Color.Black,
                    0.88f to Color.Black,
                    1f to Color.Transparent,
                ),
                blendMode = BlendMode.DstIn,
            )
        }
