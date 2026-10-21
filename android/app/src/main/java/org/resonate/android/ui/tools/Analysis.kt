package org.resonate.android.ui.tools

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.automirrored.rounded.HelpOutline
import androidx.compose.material.icons.rounded.Hearing
import androidx.compose.material.icons.rounded.Insights
import androidx.compose.material.icons.rounded.Verified
import androidx.compose.material.icons.rounded.WarningAmber
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlin.math.roundToInt
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.nativecore.AnalysisReport
import org.resonate.android.nativecore.RecognitionCandidate
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.byline
import org.resonate.android.ui.components.Artwork
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.shownTitle

class AnalysisViewModel(resonate: Resonate, private val id: ULong) : ViewModel() {
    private val features = resonate.features

    val report: StateFlow<AnalysisReport?> =
        features.analysis
            .map { it?.takeIf { found -> found.trackId == id } }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)

    val busy: StateFlow<Boolean> = features.analysisBusy
    val progress: StateFlow<Float?> = features.analysisProgress
    val online: StateFlow<Boolean> = features.online
    val key: StateFlow<String> = features.recognitionKey
    val listening: StateFlow<Boolean> = features.recognitionBusy

    val candidates: StateFlow<List<RecognitionCandidate>> =
        combine(features.recognition, features.recognitionTrack) { found, track ->
                if (track == id) found else emptyList()
            }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    fun analyse() = features.analyse(id)

    fun cancel() = features.cancelAnalysis()

    fun recognise() = features.recognise(id)

    fun saveKey(value: String) = features.saveRecognitionKey(value.trim())
}

@Composable
fun AnalysisScreen(track: ULong) {
    val song = rememberSong(track)
    val model = resonateViewModel(key = "analysis-$track") { AnalysisViewModel(it, track) }
    val report by model.report.collectAsStateWithLifecycle()
    val busy by model.busy.collectAsStateWithLifecycle()
    Page("Analysis") { padding ->
        if (song == null) {
            MissingSong(padding)
            return@Page
        }
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
            LazyColumn(
                contentPadding = padding,
                modifier = Modifier.widthIn(max = 840.dp).fillMaxWidth().testTag("analysis-list"),
            ) {
                item(key = "song") { SongLine(song) }
                item(key = "run") { AnalyseControl(model, busy, report != null) }
                report?.let { found -> reportItems(found) }
                item(key = "recognise") { Recognition(model) }
            }
        }
    }
}

@Composable
private fun SongLine(track: Track) {
    ListItem(
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        leadingContent = { Artwork(track, Sizes.rowArt, shape = MaterialTheme.shapes.medium) },
        supportingContent = { Text(track.byline, maxLines = 2, overflow = TextOverflow.Ellipsis) },
    ) { Text(track.shownTitle, maxLines = 2, overflow = TextOverflow.Ellipsis) }
}

@Composable
private fun AnalyseControl(model: AnalysisViewModel, busy: Boolean, done: Boolean) {
    Column(Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp)) {
        if (busy) {
            val progress by model.progress.collectAsStateWithLifecycle()
            Card(
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerHigh),
                shape = MaterialTheme.shapes.extraLarge,
            ) {
                Column(Modifier.padding(20.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Column(Modifier.weight(1f)) {
                            Text("Reading the whole song", style = MaterialTheme.typography.titleMedium)
                            Text(
                                progress?.let { "${(it * 100).roundToInt()}% decoded" } ?: "Starting…",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        TextButton(onClick = model::cancel, modifier = Modifier.testTag("cancel-analysis")) {
                            Icon(Icons.Rounded.Close, null, Modifier.size(ButtonDefaults.IconSize))
                            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                            Text("Cancel")
                        }
                    }
                    Spacer(Modifier.height(16.dp))
                    val fraction = progress
                    if (fraction == null) LinearWavyProgressIndicator(Modifier.fillMaxWidth())
                    else LinearWavyProgressIndicator(progress = { fraction.coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth())
                }
            }
        } else {
            Button(
                onClick = model::analyse,
                contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                modifier = Modifier.fillMaxWidth().height(56.dp).testTag("analyse"),
            ) {
                Icon(Icons.Rounded.Insights, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                Text(if (done) "Analyse again" else "Analyse original audio")
            }
            if (!done) {
                Text(
                    "Decodes the whole file once for its waveform, spectrum, loudness and a lossless check.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 8.dp, start = 4.dp, end = 4.dp),
                )
            }
        }
    }
}

private fun LazyListScope.reportItems(report: AnalysisReport) {
    item(key = "verdict") { VerdictCard(verdictOf(report.verdict)) }
    item(key = "waveform") {
        SectionHeader("Waveform")
        Plot { WaveformPlot(report.waveform) }
    }
    item(key = "spectrum") {
        SectionHeader("Spectrum")
        Plot { SpectrumPlot(report.spectrum, report.spectrumStepHz) }
    }
    item(key = "metrics") {
        SectionHeader("Levels")
        MetricGrid(metricsOf(report))
    }
    if (report.findings.isNotEmpty()) {
        item(key = "findings") { SectionHeader("Findings") }
        items(report.findings, key = { "finding-$it" }) { finding ->
            ListItem(
                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                leadingContent = { Icon(Icons.Outlined.Info, null) },
            ) { Text(finding, style = MaterialTheme.typography.bodyLarge) }
        }
    }
}

@Composable
private fun VerdictCard(verdict: Verdict) {
    val scheme = MaterialTheme.colorScheme
    val (container, content, icon) =
        when (verdict.tone) {
            Tone.GOOD -> Triple(scheme.primaryContainer, scheme.onPrimaryContainer, Icons.Rounded.Verified)
            Tone.WARNING -> Triple(scheme.tertiaryContainer, scheme.onTertiaryContainer, Icons.Rounded.WarningAmber)
            Tone.BAD -> Triple(scheme.errorContainer, scheme.onErrorContainer, Icons.Rounded.ErrorOutline)
            Tone.NEUTRAL -> Triple(scheme.surfaceContainerHighest, scheme.onSurface, Icons.AutoMirrored.Rounded.HelpOutline)
        }
    Surface(
        color = container,
        contentColor = content,
        shape = MaterialTheme.shapes.extraLarge,
        modifier = Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp).testTag("verdict"),
    ) {
        Row(Modifier.padding(20.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, Modifier.size(40.dp))
            Spacer(Modifier.width(16.dp))
            Column {
                Text(verdict.title, style = MaterialTheme.typography.titleLarge)
                Text(verdict.detail, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

@Composable
private fun Plot(content: @Composable () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
        shape = MaterialTheme.shapes.extraLarge,
        modifier = Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp),
    ) {
        Box(Modifier.padding(16.dp)) { content() }
    }
}

@Composable
private fun WaveformPlot(values: List<Float>) {
    val primary = MaterialTheme.colorScheme.primary
    val tertiary = MaterialTheme.colorScheme.tertiary
    Canvas(Modifier.fillMaxWidth().height(120.dp).semantics { contentDescription = "Waveform of the whole song" }) {
        if (values.isEmpty()) return@Canvas
        val step = size.width / values.size
        val middle = size.height / 2
        val bar = (step * 0.7f).coerceAtLeast(1f)
        val brush = Brush.verticalGradient(listOf(tertiary, primary, tertiary))
        waveformHeights(values).forEachIndexed { index, value ->
            val reach = (value.coerceIn(0f, 1f) * middle).coerceAtLeast(1f)
            drawRoundRect(
                brush,
                topLeft = Offset(index * step + (step - bar) / 2, middle - reach),
                size = Size(bar, reach * 2),
                cornerRadius = CornerRadius(bar / 2),
            )
        }
    }
}

@Composable
private fun SpectrumPlot(levels: List<Float>, stepHz: Float) {
    val scheme = MaterialTheme.colorScheme
    val measurer = rememberTextMeasurer()
    val labels = MaterialTheme.typography.labelSmall.copy(color = scheme.onSurfaceVariant)
    val heights = remember(levels) { spectrumHeights(levels) }
    val marks = remember(levels, stepHz) { spectrumMarks(levels.size, stepHz) }
    Canvas(Modifier.fillMaxWidth().height(160.dp).semantics { contentDescription = "Average spectrum of the whole song" }) {
        if (heights.size < 2) return@Canvas
        val texts = marks.map { (share, label) -> share to measurer.measure(label, labels) }
        val strip = (texts.maxOfOrNull { it.second.size.height } ?: 0) + 4.dp.toPx()
        val floor = size.height - strip
        texts.forEach { (share, text) ->
            val x = share * size.width
            drawLine(scheme.outlineVariant, Offset(x, 0f), Offset(x, floor), 1.dp.toPx() / 2)
            drawText(text, topLeft = Offset(x - text.size.width / 2f, size.height - text.size.height))
        }
        val trace = Path()
        heights.forEachIndexed { index, height ->
            val x = index.toFloat() / (heights.size - 1) * size.width
            val y = floor - height * floor
            if (index == 0) trace.moveTo(x, y) else trace.lineTo(x, y)
        }
        val fill =
            Path().apply {
                addPath(trace)
                lineTo(size.width, floor)
                lineTo(0f, floor)
                close()
            }
        drawPath(fill, Brush.verticalGradient(listOf(scheme.primary.copy(alpha = 0.45f), scheme.primary.copy(alpha = 0.04f)), endY = floor))
        drawPath(trace, scheme.primary, style = Stroke(1.5.dp.toPx()))
    }
}

@Composable
private fun MetricGrid(metrics: List<Metric>) {
    BoxWithConstraints(Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter)) {
        val scale = LocalDensity.current.fontScale
        val columns =
            when {
                maxWidth >= 600.dp * scale -> 3
                maxWidth >= 300.dp * scale -> 2
                else -> 1
            }
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            metrics.chunked(columns).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    row.forEach { metric -> MetricTile(metric, Modifier.weight(1f)) }
                    repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                }
            }
        }
    }
}

@Composable
private fun MetricTile(metric: Metric, modifier: Modifier) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shape = MaterialTheme.shapes.large,
        modifier = modifier,
    ) {
        Column(Modifier.padding(16.dp)) {
            Text(metric.label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.height(4.dp))
            Text(metric.value, style = MaterialTheme.typography.titleLarge.copy(fontFeatureSettings = "tnum"))
            Text(metric.detail, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun Recognition(model: AnalysisViewModel) {
    val key by model.key.collectAsStateWithLifecycle()
    val listening by model.listening.collectAsStateWithLifecycle()
    val candidates by model.candidates.collectAsStateWithLifecycle()
    val online by model.online.collectAsStateWithLifecycle()
    var typed by rememberSaveable(key) { mutableStateOf(key) }
    val focus = LocalFocusManager.current
    Column {
        SectionHeader("Recognise this recording")
        Column(Modifier.padding(horizontal = Sizes.gutter)) {
            OutlinedTextField(
                value = typed,
                onValueChange = { typed = it },
                label = { Text("AcoustID key (optional)") },
                supportingText = {
                    Text(
                        if (key.isBlank()) "Without a key the recording is recognised by ear."
                        else "Recognition asks AcoustID with your key."
                    )
                },
                singleLine = true,
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Done),
                keyboardActions =
                    KeyboardActions(onDone = {
                        focus.clearFocus()
                        model.saveKey(typed)
                    }),
                trailingIcon = {
                    if (typed != key) TextButton(onClick = { model.saveKey(typed) }) { Text("Save") }
                },
                modifier = Modifier.fillMaxWidth(),
            )
            Spacer(Modifier.height(8.dp))
            FilledTonalButton(
                onClick = model::recognise,
                enabled = online && !listening,
                contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                modifier = Modifier.testTag("recognise"),
            ) {
                if (listening) LoadingIndicator(Modifier.size(ButtonDefaults.IconSize))
                else Icon(Icons.Rounded.Hearing, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                Text(if (listening) "Listening…" else "Recognise recording")
            }
            if (!online) {
                Text(
                    "Turn on online features to recognise recordings.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
        }
        if (candidates.isNotEmpty()) {
            Spacer(Modifier.height(8.dp))
            candidates.forEach { candidate -> CandidateRow(candidate) }
            Text(
                "Possible matches only. Nothing is written to your files.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 8.dp),
            )
        }
    }
}

@Composable
private fun CandidateRow(candidate: RecognitionCandidate) {
    val song = candidate.song
    ListItem(
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        leadingContent = { ScoreBadge(candidate.score) },
        supportingContent = {
            Text(
                listOf(song.artist, song.album).filter { it.isNotBlank() }.joinToString(" · "),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        },
    ) { Text(song.title.ifBlank { "Untitled recording" }, maxLines = 2, overflow = TextOverflow.Ellipsis) }
}

@Composable
private fun ScoreBadge(score: UInt) {
    Surface(
        color = MaterialTheme.colorScheme.secondaryContainer,
        shape = MaterialTheme.shapes.medium,
        modifier =
            Modifier.heightIn(min = Sizes.rowArt).widthIn(min = Sizes.rowArt).semantics {
                contentDescription = scoreOf(score)
            },
    ) {
        Box(Modifier.padding(8.dp), contentAlignment = Alignment.Center) {
            Text(
                "${score.coerceAtMost(100u)}%",
                style = MaterialTheme.typography.titleSmall,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
            )
        }
    }
}
