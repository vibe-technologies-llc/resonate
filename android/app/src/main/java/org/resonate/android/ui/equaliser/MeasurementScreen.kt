package org.resonate.android.ui.equaliser

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.automirrored.rounded.ShowChart
import androidx.compose.material.icons.rounded.Mic
import androidx.compose.material.icons.rounded.MicOff
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material.icons.rounded.TrackChanges
import androidx.compose.material.icons.rounded.Tune
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.R
import org.resonate.android.SweepRecorder
import org.resonate.android.Words
import org.resonate.android.nativecore.BuiltInTarget
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.fitMeasurement
import org.resonate.android.nativecore.fitRecordedSweep
import org.resonate.android.nativecore.measurementOffers
import org.resonate.android.nativecore.measurementRateFor
import org.resonate.android.nativecore.measurementSweep
import org.resonate.android.readUpTo
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.settings.ButtonRow
import org.resonate.android.ui.settings.Choice
import org.resonate.android.ui.settings.ChoiceDialog
import org.resonate.android.ui.settings.Note
import org.resonate.android.ui.settings.NoticeCard
import org.resonate.android.ui.settings.SettingRow
import org.resonate.android.ui.settings.SettingsList
import org.resonate.android.words

private const val ANY_DOCUMENT = "*/*"

sealed interface TargetChoice {
    data class BuiltIn(val name: String) : TargetChoice

    data object File : TargetChoice
}

fun targetChoices(words: Words, targets: List<BuiltInTarget>): List<Choice<TargetChoice?>> =
    targets.map { Choice<TargetChoice?>(TargetChoice.BuiltIn(it.name), it.label) } +
        Choice<TargetChoice?>(
            TargetChoice.File,
            words.string(R.string.equaliser_measure_own_file),
        )

fun Aim?.choice(): TargetChoice? =
    when (this) {
        is Aim.BuiltIn -> TargetChoice.BuiltIn(name)
        is Aim.Document -> TargetChoice.File
        null -> null
    }

class MeasurementViewModel(private val resonate: Resonate) : ViewModel() {
    private val library = resonate.library
    private val measuring = MutableStateFlow(MeasurementView.unread)
    private var sweeping: Job? = null

    val measurement: StateFlow<MeasurementView> = measuring.asStateFlow()

    init {
        viewModelScope.launch {
            val offers = withContext(Dispatchers.Default) { measurementOffers() }
            measuring.update { if (it.offers == null) MeasurementView.of(offers) else it }
        }
    }

    private fun commit(curve: EqualiserCurve) = library.updatePreferences { it.withCurve(curve) }

    fun aimAt(aim: Aim) = measuring.update { it.aimed(aim) }

    fun aimAtDocument(resolver: ContentResolver, uri: Uri) {
        val offers = measuring.value.offers ?: return
        resonate.attempt(viewModelScope) { _ ->
            val read = withContext(Dispatchers.IO) { resolver.readUpTo(uri, offers.bytesAtMost) }
            measuring.update { it.aimed(Aim.Document(read.name, read.bytes)) }
        }
    }

    fun fitFrom(resolver: ContentResolver, uri: Uri) {
        val view = measuring.value
        val offers = view.offers ?: return
        val aim = view.aim ?: return
        if (view.fitting) return
        measuring.update { it.begun() }
        resonate.attempt(viewModelScope) { _ ->
            try {
                val read =
                    withContext(Dispatchers.IO) { resolver.readUpTo(uri, offers.bytesAtMost) }
                val fit =
                    withContext(Dispatchers.Default) {
                        fitMeasurement(read.bytes, aim.target(), profileNameFor(read.name))
                    }
                commit(fit.imported.curve)
                measuring.update {
                    it.finished(fitSentence(library.words, fit, aim.label(offers.targets)))
                }
            } finally {
                measuring.update { it.finished(null) }
            }
        }
    }

    fun calibrateFrom(resolver: ContentResolver, uri: Uri) {
        val offers = measuring.value.offers ?: return
        resonate.attempt(viewModelScope) { _ ->
            val read = withContext(Dispatchers.IO) { resolver.readUpTo(uri, offers.bytesAtMost) }
            measuring.update { it.calibratedBy(Calibration(read.name, read.bytes)) }
        }
    }

    fun forgetCalibration() = measuring.update { it.calibratedBy(null) }

    fun measure(context: Context) {
        val view = measuring.value
        val offers = view.offers ?: return
        val aim = view.aim ?: return
        if (view.fitting) return
        measuring.update { it.sounded() }
        sweeping =
            resonate.attempt(viewModelScope) { _ ->
                try {
                    if (!resonate.host.allowMicrophone()) {
                        measuring.update { it.turnedAway() }
                        return@attempt
                    }
                    library.pauseForRecording()
                    val rate =
                        withContext(Dispatchers.Default) {
                            measurementRateFor(SweepRecorder.outputRate())
                        }
                    val sweep = withContext(Dispatchers.Default) { measurementSweep(rate) }
                    val recorded = SweepRecorder.measure(context, sweep)
                    measuring.update { it.heard() }
                    val fit =
                        withContext(Dispatchers.Default) {
                            fitRecordedSweep(
                                recorded.toList(),
                                sweep.rate,
                                view.calibration?.bytes,
                                aim.target(),
                                library.words.string(R.string.equaliser_measured_profile),
                            )
                        }
                    commit(fit.imported.curve)
                    measuring.update {
                        it.finished(fitSentence(library.words, fit, aim.label(offers.targets)))
                    }
                } finally {
                    measuring.update { it.finished(null) }
                }
            }
    }

    fun stop() {
        sweeping?.cancel()
    }

    override fun onCleared() {
        sweeping?.cancel()
    }
}

@Composable
fun MeasurementScreen() {
    val model = resonateViewModel { MeasurementViewModel(it) }
    val view by model.measurement.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val resolver = context.contentResolver
    val resonate = LocalResonate.current
    var choosing by rememberSaveable { mutableStateOf(false) }
    val measurementPicker =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) model.fitFrom(resolver, uri)
        }
    val calibrationPicker =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) model.calibrateFrom(resolver, uri)
        }
    val targetPicker =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) model.aimAtDocument(resolver, uri)
        }
    val offers = view.offers
    val aim = view.aim
    val ready = offers != null && aim != null && !view.fitting
    Page(stringResource(R.string.equaliser_measure)) { padding ->
        SettingsList(padding, Modifier.testTag("measurement-list")) {
            item(key = "target") {
                SectionHeader(stringResource(R.string.equaliser_measure_target))
                SettingRow(
                    title = stringResource(R.string.equaliser_measure_fit_toward),
                    icon = Icons.Rounded.TrackChanges,
                    supporting =
                        if (offers == null) {
                            stringResource(R.string.equaliser_measure_reading_targets)
                        } else {
                            aim?.label(offers.targets)
                                ?: stringResource(R.string.equaliser_measure_choose_target)
                        },
                    enabled = offers != null && !view.fitting,
                    onClick = { choosing = true },
                    modifier = Modifier.testTag("measurement-target"),
                )
                Note(stringResource(R.string.equaliser_measure_target_note))
            }
            item(key = "file") {
                SectionHeader(stringResource(R.string.equaliser_measure_file))
                Note(stringResource(R.string.equaliser_measure_file_note))
                ButtonRow(start = true) {
                    FilledTonalButton(
                        onClick = { measurementPicker.launch(arrayOf(ANY_DOCUMENT)) },
                        enabled = ready,
                        contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                        modifier = Modifier.testTag("measurement-fit"),
                    ) {
                        Icon(Icons.AutoMirrored.Rounded.ShowChart, null, Modifier.size(ButtonDefaults.IconSize))
                        Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                        Text(
                            stringResource(
                                if (view.fitting && !view.sounding) {
                                    R.string.equaliser_measure_fitting
                                } else R.string.equaliser_measure_fit_file
                            )
                        )
                    }
                }
            }
            item(key = "microphone") {
                SectionHeader(stringResource(R.string.equaliser_measure_microphone))
                Note(stringResource(R.string.equaliser_measure_microphone_note))
                ButtonRow(start = true) {
                    Button(
                        onClick = { model.measure(context) },
                        enabled = ready,
                        contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                        modifier = Modifier.testTag("measurement-measure"),
                    ) {
                        Icon(Icons.Rounded.Mic, null, Modifier.size(ButtonDefaults.IconSize))
                        Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                        Text(
                            stringResource(
                                if (view.sounding) R.string.equaliser_measure_measuring
                                else R.string.equaliser_measure_start
                            )
                        )
                    }
                    if (view.sounding) {
                        OutlinedButton(
                            onClick = model::stop,
                            contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                            modifier = Modifier.testTag("measurement-stop"),
                        ) {
                            Icon(Icons.Rounded.Stop, null, Modifier.size(ButtonDefaults.IconSize))
                            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                            Text(stringResource(R.string.equaliser_measure_stop))
                        }
                    }
                }
                val calibration = view.calibration
                SettingRow(
                    title = stringResource(R.string.equaliser_measure_calibration),
                    icon = Icons.Rounded.Tune,
                    supporting =
                        calibration?.name
                            ?: stringResource(R.string.equaliser_measure_calibration_none),
                    enabled = offers != null && !view.fitting,
                    onClick = { calibrationPicker.launch(arrayOf(ANY_DOCUMENT)) },
                    trailing =
                        if (calibration == null) null
                        else {
                            {
                                IconButton(
                                    onClick = model::forgetCalibration,
                                    enabled = !view.fitting,
                                ) {
                                    Icon(
                                        Icons.Rounded.Close,
                                        stringResource(
                                            R.string.equaliser_measure_calibration_forget
                                        ),
                                    )
                                }
                            }
                        },
                    modifier = Modifier.testTag("measurement-calibration"),
                )
            }
            if (view.denied) {
                item(key = "denied") {
                    NoticeCard(
                        Icons.Rounded.MicOff,
                        stringResource(R.string.equaliser_measure_denied),
                        action = stringResource(R.string.equaliser_open_settings),
                        onAction = resonate.host::openAppSettings,
                        modifier = Modifier.testTag("measurement-denied"),
                    )
                }
            }
            item(key = "status") {
                Column(Modifier.semantics { liveRegion = LiveRegionMode.Polite }) {
                    Box(
                        Modifier.fillMaxWidth()
                            .height(12.dp)
                            .padding(horizontal = Sizes.gutter)
                    ) {
                        if (view.fitting) LinearWavyProgressIndicator(Modifier.fillMaxWidth())
                    }
                    view.said?.let { Note(it, Modifier.testTag("measurement-said")) }
                }
            }
        }
    }
    if (choosing && offers != null) {
        ChoiceDialog(
            title = stringResource(R.string.equaliser_measure_fit_toward),
            choices = targetChoices(words(), offers.targets),
            selected = aim.choice(),
            onChoose = { chosen ->
                when (chosen) {
                    is TargetChoice.BuiltIn -> model.aimAt(Aim.BuiltIn(chosen.name))
                    TargetChoice.File -> targetPicker.launch(arrayOf(ANY_DOCUMENT))
                    null -> Unit
                }
            },
            onDismiss = { choosing = false },
        )
    }
}
