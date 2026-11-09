package org.resonate.android.ui.equaliser

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Exposure
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.unit.dp
import org.resonate.android.R
import org.resonate.android.ui.components.CommitSlider
import org.resonate.android.ui.components.Sizes

val SLIDER_TRACK = 4.dp

private val VALUE_PADDING = PaddingValues(horizontal = 8.dp)

private val CONTROL_OVERLAP = 12.dp

@Composable
fun EqualiserCard(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Card(
        modifier = modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp),
        shape = MaterialTheme.shapes.extraLarge,
        colors =
            CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
    ) {
        content()
    }
}

@Composable
fun MeasureControl(
    measure: Measure,
    value: Float,
    description: String,
    onCommit: (Float) -> Unit,
    tag: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    unused: String = stringResource(R.string.equaliser_not_used),
    action: (@Composable () -> Unit)? = null,
) {
    var draft by remember { mutableStateOf<Float?>(null) }
    var entering by rememberSaveable { mutableStateOf(false) }
    val scale =
        remember(measure.range, measure.logarithmic) {
            if (measure.logarithmic) LogScale(measure.range.start, measure.range.endInclusive)
            else null
        }
    val actual: (Float) -> Float = { raw -> scale?.actual(raw) ?: raw }
    val shown = if (enabled) measure.shown(draft ?: value) else unused
    val typeAValue = stringResource(R.string.equaliser_type_a_value)
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(-CONTROL_OVERLAP)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                measure.label,
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.weight(1f),
            )
            TextButton(
                onClick = { entering = true },
                enabled = enabled,
                contentPadding = VALUE_PADDING,
                modifier =
                    Modifier.semantics {
                            contentDescription = "$description, $shown"
                            onClick(label = typeAValue) {
                                entering = true
                                true
                            }
                        }
                        .testTag("$tag-value"),
            ) {
                Text(
                    shown,
                    style = MaterialTheme.typography.titleMedium.copy(fontFeatureSettings = "tnum"),
                )
            }
            action?.invoke()
        }
        CommitSlider(
            value = scale?.fraction(value.coerceIn(measure.range)) ?: value.coerceIn(measure.range),
            range = if (scale == null) measure.range else 0f..1f,
            onCommit = { raw -> onCommit(actual(raw)) },
            onDraft = { raw -> draft = raw?.let(actual) },
            enabled = enabled,
            trackHeight = SLIDER_TRACK,
            centred = measure.centred,
            stateText = { raw -> if (enabled) measure.shown(actual(raw)) else unused },
            modifier =
                Modifier.fillMaxWidth().semantics { contentDescription = description }.testTag(tag),
        )
    }
    if (entering) {
        ExactEntry(
            measure = measure,
            value = value,
            title = description,
            onDone = {
                entering = false
                onCommit(it)
            },
            onDismiss = { entering = false },
        )
    }
}

@Composable
fun ExactEntry(
    measure: Measure,
    value: Float,
    title: String,
    onDone: (Float) -> Unit,
    onDismiss: () -> Unit,
) {
    val first = measure.written(value)
    var text by
        rememberSaveable(stateSaver = TextFieldValue.Saver) {
            mutableStateOf(TextFieldValue(first, TextRange(0, first.length)))
        }
    val focus = remember { FocusRequester() }
    val accepted = measure.accepted(text.text)
    val finish = { accepted?.let(onDone) }
    LaunchedEffect(Unit) { focus.requestFocus() }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                label = { Text(measure.label) },
                suffix = measure.unit.takeIf { it.isNotEmpty() }?.let { unit -> { Text(unit) } },
                supportingText = { Text(measure.reach) },
                isError = accepted == null,
                singleLine = true,
                keyboardOptions =
                    KeyboardOptions(
                        keyboardType = KeyboardType.Decimal,
                        imeAction = ImeAction.Done,
                    ),
                keyboardActions = KeyboardActions(onDone = { finish() }),
                trailingIcon =
                    if (measure.signed) {
                        {
                            IconButton(
                                onClick = {
                                    val flipped = flippedSign(text.text)
                                    text = TextFieldValue(flipped, TextRange(flipped.length))
                                }
                            ) {
                                Icon(
                                    Icons.Rounded.Exposure,
                                    contentDescription =
                                        stringResource(R.string.equaliser_change_sign),
                                )
                            }
                        }
                    } else null,
                modifier = Modifier.fillMaxWidth().focusRequester(focus).testTag("exact-entry"),
            )
        },
        confirmButton = {
            TextButton(
                onClick = { finish() },
                enabled = accepted != null,
                modifier = Modifier.testTag("exact-entry-set"),
            ) {
                Text(stringResource(R.string.equaliser_set))
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss) { Text(stringResource(R.string.equaliser_cancel)) }
        },
    )
}
