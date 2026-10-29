package org.resonate.android.ui.equaliser

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.nativecore.TestArm
import org.resonate.android.nativecore.TestHear
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes

@Composable
fun ListeningTestSection(model: EqualiserViewModel) {
    val view by model.listening.collectAsStateWithLifecycle()
    if (view.lengths.isEmpty()) return
    val run = view.run
    Column {
        SectionHeader("Listening test")
        Text(
            LISTENING_TEST_HOW_IT_WORKS,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp),
        )
        if (run == null) {
            FlowRow(
                Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                view.lengths.forEach { trials ->
                    FilledTonalButton(
                        onClick = { model.beginTest(trials) },
                        enabled = !view.beginning,
                        contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                        modifier = Modifier.testTag("listening-begin-$trials"),
                    ) {
                        Icon(Icons.Rounded.PlayArrow, null, Modifier.size(ButtonDefaults.IconSize))
                        Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                        Text("Begin $trials trials")
                    }
                }
            }
            return@Column
        }
        val standing = run.standing
        Column(
            Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp)
                .semantics { liveRegion = LiveRegionMode.Polite }
                .testTag("listening-standing")
        ) {
            Text(standingWords(standing), style = MaterialTheme.typography.bodyLarge)
            Text(
                heardWords(standing),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (standing.verdicts.isNotEmpty()) {
                Text(
                    verdictsInALine(standing),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            lastWords(standing)?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (standing.finished) {
                Text(conclusionWords(standing), style = MaterialTheme.typography.bodyMedium)
            }
        }
        if (!standing.finished) {
            FlowRow(
                Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                HearButton("A", TestHear.A, model)
                HearButton("B", TestHear.B, model)
                HearButton("X", TestHear.X, model)
            }
            FlowRow(
                Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                SayButton("X is A", TestArm.A, model)
                SayButton("X is B", TestArm.B, model)
            }
        }
        OutlinedButton(
            onClick = model::endTest,
            contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
            modifier = Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp).testTag("listening-end"),
        ) {
            Icon(Icons.Rounded.Stop, null, Modifier.size(ButtonDefaults.IconSize))
            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
            Text(if (standing.finished) "Done" else "End the test")
        }
    }
}

@Composable
private fun HearButton(label: String, hear: TestHear, model: EqualiserViewModel) {
    FilledTonalButton(
        onClick = { model.hear(hear) },
        contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
        modifier = Modifier.testTag("listening-hear-$label"),
    ) {
        Icon(Icons.Rounded.PlayArrow, null, Modifier.size(ButtonDefaults.IconSize))
        Spacer(Modifier.width(ButtonDefaults.IconSpacing))
        Text("Hear $label")
    }
}

@Composable
private fun SayButton(label: String, arm: TestArm, model: EqualiserViewModel) {
    OutlinedButton(
        onClick = { model.say(arm) },
        contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
        modifier = Modifier.testTag("listening-say-${arm.name}"),
    ) {
        Icon(Icons.Rounded.Check, null, Modifier.size(ButtonDefaults.IconSize))
        Spacer(Modifier.width(ButtonDefaults.IconSpacing))
        Text(label)
    }
}
