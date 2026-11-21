package org.resonate.android.ui.wrapped

import android.graphics.Bitmap
import androidx.compose.animation.core.snap
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.PagerDefaults
import androidx.compose.foundation.pager.PagerState
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.ArrowForward
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.Share
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import androidx.core.view.WindowCompat
import java.io.File
import java.io.FileOutputStream
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.R
import org.resonate.android.nativecore.WrappedBar
import org.resonate.android.nativecore.WrappedRow
import org.resonate.android.nativecore.WrappedSlide
import org.resonate.android.nativecore.WrappedYear
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.rememberCalm
import org.resonate.android.ui.settings.rememberCopy
import org.resonate.android.ui.theme.LocalDarkTheme
import org.resonate.android.words

private val CARD_WIDTH = 360.dp
private val CARD_HEIGHT = 450.dp
private const val CARD_PIXELS_PER_DP = 3f
private const val CARD_ROWS_AT_MOST = 5
private const val CARD_FOLDER = "shared"
private const val PNG_QUALITY = 100
private val CHART_HEIGHT = 140.dp
private val CARD_CHART_HEIGHT = 100.dp
private const val SMALLEST_BAR = 0.02f
private val DECK_WIDTH = 560.dp
private val COVER_BARS = listOf(0.38f, 0.66f, 1f, 0.78f, 0.48f)

@Composable
fun WrappedDeck(year: WrappedYear, onYear: (Long) -> Unit, onClose: () -> Unit) {
    val pager = rememberPagerState { year.slides.size }
    var yearTurnedThrough by rememberSaveable { mutableLongStateOf(year.year) }
    LaunchedEffect(year.year) {
        if (yearTurnedThrough != year.year) {
            yearTurnedThrough = year.year
            pager.scrollToPage(0)
        }
    }
    Dialog(
        onDismissRequest = onClose,
        properties =
            DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        Deck(year, pager, onYear, onClose)
    }
}

@Composable
private fun Deck(
    year: WrappedYear,
    pager: PagerState,
    onYear: (Long) -> Unit,
    onClose: () -> Unit,
) {
    BarsInkFollowsTheTheme()
    val slides = year.slides
    val calm = rememberCalm()
    val scope = rememberCoroutineScope()
    val focus = remember { FocusRequester() }
    val card = rememberGraphicsLayer()
    val shown = slides.getOrNull(pager.currentPage)
    val turn: (Int) -> Unit = { to ->
        if (to in slides.indices) {
            scope.launch {
                if (calm.reduced) pager.scrollToPage(to) else pager.animateScrollToPage(to)
            }
        }
    }
    LaunchedEffect(Unit) { focus.requestFocus() }
    val entry = WrappedWords.entry(words(), year.year)
    Surface(
        color = MaterialTheme.colorScheme.surface,
        modifier =
            Modifier.fillMaxSize()
                .semantics {
                    testTagsAsResourceId = true
                    paneTitle = entry
                }
                .testTag("wrapped-deck")
                .focusRequester(focus)
                .focusable()
                .onPreviewKeyEvent { event ->
                    when {
                        event.type != KeyEventType.KeyDown -> false
                        event.key == Key.DirectionRight -> {
                            turn(pager.currentPage + 1)
                            true
                        }
                        event.key == Key.DirectionLeft -> {
                            turn(pager.currentPage - 1)
                            true
                        }
                        else -> false
                    }
                },
    ) {
        Column(
            Modifier.fillMaxSize().safeDrawingPadding(),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Row(
                Modifier.widthIn(max = DECK_WIDTH).fillMaxWidth().padding(horizontal = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconButton(onClick = onClose, modifier = Modifier.testTag("wrapped-close")) {
                    Icon(
                        Icons.Rounded.Close,
                        contentDescription = stringResource(R.string.wrapped_close),
                    )
                }
                Box(Modifier.weight(1f), contentAlignment = Alignment.Center) {
                    Years(year, onYear)
                }
                WrappedActions(year, pager.currentPage, card, shown)
            }
            HorizontalPager(
                state = pager,
                flingBehavior =
                    if (calm.reduced) PagerDefaults.flingBehavior(pager, snapAnimationSpec = snap())
                    else PagerDefaults.flingBehavior(pager),
                modifier =
                    Modifier.weight(1f)
                        .widthIn(max = DECK_WIDTH)
                        .fillMaxWidth()
                        .testTag("wrapped-slides"),
            ) { page ->
                Slide(slides[page])
            }
            SlideNavigation(pager.currentPage, slides.size, turn)
        }
    }
    if (shown != null) CardBeingDrawn(year.year, shown, card)
}

@Composable
private fun BarsInkFollowsTheTheme() {
    val window = (LocalView.current.parent as? DialogWindowProvider)?.window ?: return
    val view = LocalView.current
    val dark = LocalDarkTheme.current
    SideEffect {
        WindowCompat.getInsetsController(window, view).apply {
            isAppearanceLightStatusBars = !dark
            isAppearanceLightNavigationBars = !dark
        }
    }
}

@Composable
private fun Years(year: WrappedYear, onYear: (Long) -> Unit) {
    if (year.years.size <= 1) return
    Row(
        Modifier.padding(start = 8.dp, end = 8.dp).horizontalScroll(rememberScrollState()),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        year.years.forEach { heard ->
            FilterChip(
                selected = heard == year.year,
                onClick = { onYear(heard) },
                label = { Text(heard.toString()) },
            )
        }
    }
}

@Composable
private fun WrappedActions(
    year: WrappedYear,
    page: Int,
    card: GraphicsLayer,
    shown: WrappedSlide?,
) {
    val resonate = LocalResonate.current
    val context = LocalContext.current
    val copy = rememberCopy(sensitive = false)
    val scope = rememberCoroutineScope()
    var copied by remember(year.year) { mutableStateOf(false) }
    val words = words()
    val entry = WrappedWords.entry(words, year.year)
    val notDrawn = stringResource(R.string.wrapped_not_drawn)
    IconButton(
        onClick = {
            copy(entry, year.summary)
            copied = true
        },
        modifier = Modifier.testTag("wrapped-copy"),
    ) {
        Icon(
            if (copied) Icons.Rounded.Check else Icons.Rounded.ContentCopy,
            contentDescription =
                stringResource(if (copied) R.string.wrapped_copied else R.string.wrapped_copy),
        )
    }
    IconButton(
        onClick = {
            scope.launch {
                resonate
                    .read {
                        val drawn = card.toImageBitmap().asAndroidBitmap()
                        val file =
                            File(
                                File(context.cacheDir, CARD_FOLDER),
                                WrappedWords.cardFileName(year.year, page),
                            )
                        withContext(Dispatchers.IO) { writePng(drawn, file) }
                        file
                    }
                    .onSuccess { resonate.host.shareImage(entry, it) }
                    .onFailure {
                        android.util.Log.w("Resonate", "The card was not drawn", it)
                        resonate.library.notice(notDrawn)
                    }
            }
        },
        enabled = shown != null,
        modifier = Modifier.testTag("wrapped-share"),
    ) {
        Icon(Icons.Rounded.Share, contentDescription = stringResource(R.string.wrapped_share))
    }
}

@Composable
private fun SlideNavigation(page: Int, total: Int, turn: (Int) -> Unit) {
    if (total == 0) return
    Row(
        Modifier.widthIn(max = DECK_WIDTH)
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        FilledTonalIconButton(
            onClick = { turn(page - 1) },
            enabled = page > 0,
            modifier = Modifier.testTag("wrapped-before"),
        ) {
            Icon(
                Icons.AutoMirrored.Rounded.ArrowBack,
                contentDescription = stringResource(R.string.wrapped_before),
            )
        }
        Column(
            Modifier.weight(1f),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(
                WrappedWords.place(words(), page, total),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.testTag("wrapped-place"),
            )
            LinearProgressIndicator(
                progress = { (page + 1).toFloat() / total },
                modifier = Modifier.fillMaxWidth().clearAndSetSemantics {},
                drawStopIndicator = {},
            )
        }
        FilledTonalIconButton(
            onClick = { turn(page + 1) },
            enabled = page < total - 1,
            modifier = Modifier.testTag("wrapped-next"),
        ) {
            Icon(
                Icons.AutoMirrored.Rounded.ArrowForward,
                contentDescription = stringResource(R.string.wrapped_next),
            )
        }
    }
}

private fun writePng(drawn: Bitmap, file: File) {
    file.parentFile?.mkdirs()
    val readable =
        if (drawn.config == Bitmap.Config.HARDWARE) drawn.copy(Bitmap.Config.ARGB_8888, false)
        else drawn
    FileOutputStream(file).use { readable.compress(Bitmap.CompressFormat.PNG, PNG_QUALITY, it) }
}

@Composable
private fun Slide(slide: WrappedSlide) {
    val scheme = MaterialTheme.colorScheme
    val cover = slide.rows.isEmpty() && slide.bars.isEmpty()
    val gradient =
        if (cover) {
            Brush.verticalGradient(listOf(scheme.primaryContainer, scheme.surface))
        } else {
            Brush.verticalGradient(listOf(scheme.surfaceContainerHigh, scheme.surface))
        }
    Surface(
        shape = MaterialTheme.shapes.extraLarge,
        color = scheme.surface,
        contentColor = scheme.onSurface,
        border = BorderStroke(1.dp, scheme.outlineVariant),
        modifier = Modifier.fillMaxSize().padding(horizontal = 16.dp, vertical = 4.dp),
    ) {
        BoxWithConstraints(Modifier.background(gradient)) {
            Column(
                Modifier.fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .heightIn(min = maxHeight)
                    .padding(24.dp),
                verticalArrangement =
                    Arrangement.spacedBy(
                        24.dp,
                        if (cover) Alignment.CenterVertically else Alignment.Top,
                    ),
                horizontalAlignment = if (cover) Alignment.CenterHorizontally else Alignment.Start,
            ) {
                if (cover) CoverMark()
                SlideHeading(slide, cover)
                if (slide.rows.isNotEmpty()) {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        slide.rows.forEachIndexed { at, row -> RowOfASlide(at, row) }
                    }
                }
                if (slide.bars.isNotEmpty()) Chart(slide.bars, CHART_HEIGHT)
            }
        }
    }
}

@Composable
private fun SlideHeading(slide: WrappedSlide, cover: Boolean) {
    val scheme = MaterialTheme.colorScheme
    val align = if (cover) TextAlign.Center else TextAlign.Start
    Column(
        Modifier.fillMaxWidth(),
        horizontalAlignment = if (cover) Alignment.CenterHorizontally else Alignment.Start,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            slide.eyebrow,
            style = MaterialTheme.typography.labelLarge,
            color = scheme.onSurfaceVariant,
            textAlign = align,
        )
        Text(
            slide.headline,
            style =
                if (cover) MaterialTheme.typography.displaySmall
                else MaterialTheme.typography.headlineLarge,
            fontWeight = FontWeight.Bold,
            textAlign = align,
            modifier = Modifier.semantics { heading() },
        )
        slide.detail?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodyLarge,
                color = scheme.onSurfaceVariant,
                textAlign = align,
            )
        }
    }
}

@Composable
private fun CoverMark() {
    val scheme = MaterialTheme.colorScheme
    val colors =
        listOf(scheme.tertiary, scheme.secondary, scheme.primary, scheme.secondary, scheme.tertiary)
    Row(
        Modifier.widthIn(max = 192.dp).fillMaxWidth().height(96.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        COVER_BARS.forEachIndexed { index, height ->
            Box(
                Modifier.weight(1f)
                    .fillMaxHeight(height)
                    .background(colors[index], MaterialTheme.shapes.extraLarge)
            )
        }
    }
}

@Composable
private fun RowOfASlide(at: Int, row: WrappedRow) {
    val scheme = MaterialTheme.colorScheme
    Row(
        Modifier.fillMaxWidth().semantics(mergeDescendants = true) {},
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            "${at + 1}",
            style = MaterialTheme.typography.titleMedium,
            color = scheme.primary,
            modifier = Modifier.width(24.dp),
        )
        Column(Modifier.weight(1f)) {
            Text(
                row.name,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            row.by?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodyMedium,
                    color = scheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        Text(
            row.figure,
            style = MaterialTheme.typography.labelLarge,
            color = scheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun Chart(bars: List<WrappedBar>, height: androidx.compose.ui.unit.Dp) {
    val scheme = MaterialTheme.colorScheme
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(2.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        bars.forEachIndexed { at, bar ->
            Column(
                Modifier.weight(1f).clearAndSetSemantics {
                    contentDescription = bar.said
                },
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Box(
                    Modifier.height(height).fillMaxWidth(),
                    contentAlignment = Alignment.BottomCenter,
                ) {
                    Box(
                        Modifier.fillMaxWidth()
                            .fillMaxHeight(
                                WrappedWords.barFraction(bar.sharePermille)
                                    .coerceAtLeast(SMALLEST_BAR)
                            )
                            .background(
                                if (bar.peak) scheme.primary else scheme.secondaryContainer,
                                MaterialTheme.shapes.extraSmall,
                            )
                    )
                }
                Box(
                    Modifier.height(20.dp).wrapContentSize(unbounded = true),
                    contentAlignment = Alignment.Center,
                ) {
                    if (WrappedWords.labelsBar(at, bars.size)) {
                        Text(
                            bar.label,
                            style = MaterialTheme.typography.labelSmall,
                            color = scheme.onSurfaceVariant,
                            maxLines = 1,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun CardBeingDrawn(year: Long, slide: WrappedSlide, layer: GraphicsLayer) {
    Box(
        Modifier.size(0.dp)
            .wrapContentSize(align = Alignment.TopStart, unbounded = true)
            .clearAndSetSemantics {}
    ) {
        CompositionLocalProvider(
            androidx.compose.ui.platform.LocalDensity provides
                Density(CARD_PIXELS_PER_DP, fontScale = 1f)
        ) {
            Box(
                Modifier.requiredSize(CARD_WIDTH, CARD_HEIGHT).drawWithContent {
                    layer.record { this@drawWithContent.drawContent() }
                }
            ) {
                ShareCard(year, slide)
            }
        }
    }
}

@Composable
private fun ShareCard(year: Long, slide: WrappedSlide) {
    val scheme = MaterialTheme.colorScheme
    Column(
        Modifier.fillMaxSize().background(scheme.primaryContainer).padding(28.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(
            stringResource(R.string.wrapped_card_brand, year),
            style = MaterialTheme.typography.labelLarge,
            color = scheme.onPrimaryContainer,
        )
        Text(
            slide.eyebrow,
            style = MaterialTheme.typography.labelLarge,
            color = scheme.onPrimaryContainer,
        )
        Text(
            slide.headline,
            style = MaterialTheme.typography.headlineMedium,
            fontWeight = FontWeight.SemiBold,
            color = scheme.onPrimaryContainer,
            maxLines = 4,
            overflow = TextOverflow.Ellipsis,
        )
        slide.detail?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodyLarge,
                color = scheme.onPrimaryContainer,
                maxLines = 3,
                overflow = TextOverflow.Ellipsis,
            )
        }
        slide.rows.take(CARD_ROWS_AT_MOST).forEachIndexed { at, row ->
            Text(
                "${at + 1}  ${row.name}",
                style = MaterialTheme.typography.titleSmall,
                color = scheme.onPrimaryContainer,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (slide.bars.isNotEmpty()) {
            MaterialTheme(
                colorScheme =
                    scheme.copy(
                        onSurfaceVariant = scheme.onPrimaryContainer,
                        secondaryContainer = scheme.surface,
                    )
            ) {
                Chart(slide.bars, CARD_CHART_HEIGHT)
            }
        }
    }
}
