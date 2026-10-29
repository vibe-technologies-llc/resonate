package org.resonate.android.ui.equaliser

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.FileOpen
import androidx.compose.material.icons.rounded.Insights
import androidx.compose.material.icons.rounded.Mic
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.ui.LocalResonate
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes

private const val ANY_DOCUMENT = "*/*"

@Composable
fun MeasurementSection(model: EqualiserViewModel) {
    val view by model.measurement.collectAsStateWithLifecycle()
    val resolver = LocalContext.current.contentResolver
    val measurementPicker =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) model.fitFrom(resolver, uri)
        }
    val calibrationPicker =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) model.calibrateFrom(resolver, uri)
        }
    val context = LocalContext.current
    val resonate = LocalResonate.current
    val targetPicker =
        rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) model.aimAtDocument(resolver, uri)
        }
    val offers = view.offers ?: return
    val aim = view.aim
    Column {
        SectionHeader("Measured correction")
        Text(
            "Fit a few bands to your own frequency-response measurement, as AutoEq's CSV or REW's exported text, toward a target.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp),
        )
        Text(
            "Fit toward",
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp),
        )
        FlowRow(
            Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp).testTag("measurement-targets"),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            offers.targets.forEach { target ->
                FilterChip(
                    selected = aim == Aim.BuiltIn(target.name),
                    onClick = { model.aimAt(Aim.BuiltIn(target.name)) },
                    enabled = !view.fitting,
                    label = { Text(target.label) },
                    modifier = Modifier.testTag("measurement-target-${target.name}"),
                )
            }
            FilterChip(
                selected = aim is Aim.Document,
                onClick = { targetPicker.launch(arrayOf(ANY_DOCUMENT)) },
                enabled = !view.fitting,
                leadingIcon = { Icon(Icons.Rounded.FileOpen, null, Modifier.size(ButtonDefaults.IconSize)) },
                label = { Text((aim as? Aim.Document)?.name ?: "A file…") },
                modifier = Modifier.testTag("measurement-target-file"),
            )
        }
        FlowRow(
            Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            FilledTonalButton(
                onClick = { measurementPicker.launch(arrayOf(ANY_DOCUMENT)) },
                enabled = !view.fitting && aim != null,
                contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                modifier = Modifier.testTag("measurement-fit"),
            ) {
                Icon(Icons.Rounded.Insights, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                Text(if (view.fitting) "Fitting…" else "Fit a measurement…")
            }
        }
        Text(
            "Measure your room",
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp),
        )
        Text(
            "Plays a sweep through the speaker, pausing the music, and fits the microphone's recording toward the target. The microphone is taken flat unless you give it a calibration file.",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp),
        )
        FlowRow(
            Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            FilledTonalButton(
                onClick = { model.measureRoom(context) },
                enabled = !view.fitting && aim != null,
                contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                modifier = Modifier.testTag("measurement-measure"),
            ) {
                Icon(Icons.Rounded.Mic, null, Modifier.size(ButtonDefaults.IconSize))
                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                Text(if (view.sounding) "Measuring…" else "Measure")
            }
            if (view.sounding) {
                FilledTonalButton(
                    onClick = model::stopMeasuring,
                    contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                    modifier = Modifier.testTag("measurement-stop"),
                ) {
                    Icon(Icons.Rounded.Stop, null, Modifier.size(ButtonDefaults.IconSize))
                    Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                    Text("Stop")
                }
            }
            FilterChip(
                selected = view.calibration != null,
                onClick = {
                    if (view.calibration == null) calibrationPicker.launch(arrayOf(ANY_DOCUMENT))
                    else model.forgetCalibration()
                },
                enabled = !view.fitting,
                leadingIcon = { Icon(Icons.Rounded.FileOpen, null, Modifier.size(ButtonDefaults.IconSize)) },
                label = { Text(view.calibration?.name ?: "Microphone calibration…") },
                modifier = Modifier.testTag("measurement-calibration"),
            )
        }
        if (view.denied) {
            Text(
                "Resonate needs the microphone to hear the sweep. Allow it in Settings.",
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp).testTag("measurement-denied"),
            )
            TextButton(
                onClick = resonate.host::openAppSettings,
                modifier = Modifier.padding(horizontal = Sizes.gutter),
            ) {
                Text("Open Settings")
            }
        }
        if (view.fitting) {
            LinearWavyProgressIndicator(
                Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp)
            )
        }
        view.said?.let { said ->
            Text(
                said,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp).testTag("measurement-said"),
            )
        }
    }
}
