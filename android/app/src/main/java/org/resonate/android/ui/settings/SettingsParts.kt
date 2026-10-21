package org.resonate.android.ui.settings

import android.content.ClipData
import android.content.ClipDescription
import android.os.PersistableBundle
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.Surface
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import kotlinx.coroutines.launch
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Visibility
import androidx.compose.material.icons.rounded.VisibilityOff
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemColors
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import org.resonate.android.ui.components.Sizes

val PageWidth = 840.dp

val FAB_ROOM = 88.dp

@Composable
fun PaddingValues.withBottom(extra: Dp): PaddingValues {
    val direction = LocalLayoutDirection.current
    return PaddingValues(
        start = calculateStartPadding(direction),
        top = calculateTopPadding(),
        end = calculateEndPadding(direction),
        bottom = calculateBottomPadding() + extra,
    )
}

@Composable
fun SettingsList(
    padding: PaddingValues,
    modifier: Modifier = Modifier,
    content: LazyListScope.() -> Unit,
) {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
        LazyColumn(
            modifier = modifier.widthIn(max = PageWidth).fillMaxWidth().fillMaxHeight(),
            contentPadding = padding,
            content = content,
        )
    }
}

@Composable
fun Centered(padding: PaddingValues, content: @Composable () -> Unit) {
    Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
        content()
    }
}

@Composable
fun Loading(padding: PaddingValues) {
    Centered(padding) { LoadingIndicator(Modifier.testTag("loading")) }
}

@Composable
fun ListLoading() {
    Box(Modifier.fillMaxWidth().padding(vertical = 48.dp), contentAlignment = Alignment.Center) {
        LoadingIndicator(Modifier.testTag("loading"))
    }
}

@Composable
fun SettingIcon(icon: ImageVector) {
    Icon(icon, contentDescription = null)
}

@Composable
private fun rowColors(): ListItemColors {
    val scheme = MaterialTheme.colorScheme
    return ListItemDefaults.colors(
        containerColor = Color.Transparent,
        selectedContainerColor = Color.Transparent,
        selectedContentColor = scheme.onSurface,
        selectedLeadingContentColor = scheme.onSurfaceVariant,
        selectedTrailingContentColor = scheme.onSurfaceVariant,
        selectedSupportingContentColor = scheme.onSurfaceVariant,
        selectedOverlineContentColor = scheme.onSurfaceVariant,
    )
}

@Composable
fun SettingRow(
    title: String,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    supporting: String? = null,
    enabled: Boolean = true,
    supportingColor: Color? = null,
    trailing: (@Composable () -> Unit)? = null,
    leadingContent: (@Composable () -> Unit)? = null,
    onClick: (() -> Unit)? = null,
) {
    val leading: (@Composable () -> Unit)? = leadingContent ?: icon?.let { { SettingIcon(it) } }
    val support: (@Composable () -> Unit)? =
        supporting?.let {
            {
                if (supportingColor == null) Text(it)
                else Text(it, color = supportingColor)
            }
        }
    if (onClick == null) {
        ListItem(
            modifier = modifier,
            enabled = enabled,
            leadingContent = leading,
            supportingContent = support,
            trailingContent = trailing,
            colors = rowColors(),
        ) {
            Text(title)
        }
    } else {
        ListItem(
            onClick = onClick,
            modifier = modifier,
            enabled = enabled,
            leadingContent = leading,
            supportingContent = support,
            trailingContent = trailing,
            colors = rowColors(),
        ) {
            Text(title)
        }
    }
}

@Composable
fun SwitchRow(
    title: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    supporting: String? = null,
    enabled: Boolean = true,
) {
    ListItem(
        checked = checked,
        onCheckedChange = onCheckedChange,
        modifier = modifier,
        enabled = enabled,
        leadingContent = icon?.let { { SettingIcon(it) } },
        supportingContent = supporting?.let { { Text(it) } },
        trailingContent = { Switch(checked = checked, onCheckedChange = null, enabled = enabled) },
        colors = rowColors(),
    ) {
        Text(title)
    }
}

@Composable
fun RadioRow(
    title: String,
    selected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    supporting: String? = null,
    enabled: Boolean = true,
) {
    ListItem(
        selected = selected,
        onClick = onClick,
        modifier = modifier,
        enabled = enabled,
        leadingContent = icon?.let { { SettingIcon(it) } },
        supportingContent = supporting?.let { { Text(it) } },
        trailingContent = { RadioButton(selected = selected, onClick = null, enabled = enabled) },
        colors = rowColors(),
    ) {
        Text(title)
    }
}

data class Choice<T>(val value: T, val label: String, val detail: String? = null)

@Composable
fun <T> ChoiceDialog(
    title: String,
    choices: List<Choice<T>>,
    selected: T,
    onChoose: (T) -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column(Modifier.selectableGroup().verticalScroll(rememberScrollState())) {
                choices.forEach { choice ->
                    ListItem(
                        selected = choice.value == selected,
                        onClick = {
                            onChoose(choice.value)
                            onDismiss()
                        },
                        leadingContent = {
                            RadioButton(selected = choice.value == selected, onClick = null)
                        },
                        supportingContent = choice.detail?.let { { Text(it) } },
                        colors = rowColors(),
                        contentPadding = PaddingValues(vertical = 8.dp),
                    ) {
                        Text(choice.label)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
fun <T> ChoiceRow(
    title: String,
    choices: List<Choice<T>>,
    selected: T,
    onChoose: (T) -> Unit,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    enabled: Boolean = true,
) {
    var open by rememberSaveable { mutableStateOf(false) }
    SettingRow(
        title = title,
        modifier = modifier,
        icon = icon,
        supporting = choices.firstOrNull { it.value == selected }?.label,
        enabled = enabled,
        onClick = { open = true },
    )
    if (open) {
        ChoiceDialog(title, choices, selected, onChoose) { open = false }
    }
}

@Composable
fun ConfirmDialog(
    title: String,
    text: String,
    confirm: String,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
    icon: ImageVector? = null,
    destructive: Boolean = false,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = icon?.let { { Icon(it, contentDescription = null) } },
        title = { Text(title) },
        text = { Text(text) },
        confirmButton = {
            TextButton(
                onClick = {
                    onConfirm()
                    onDismiss()
                },
                colors =
                    if (destructive)
                        ButtonDefaults.textButtonColors(
                            contentColor = MaterialTheme.colorScheme.error
                        )
                    else ButtonDefaults.textButtonColors(),
                modifier = Modifier.testTag("confirm"),
            ) {
                Text(confirm)
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
fun Note(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        style = MaterialTheme.typography.bodyMedium,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
    )
}

@Composable
fun Footnote(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 12.dp),
    )
}

@Composable
fun NoticeCard(
    icon: ImageVector,
    text: String,
    modifier: Modifier = Modifier,
    error: Boolean = false,
    action: String? = null,
    onAction: (() -> Unit)? = null,
) {
    val scheme = MaterialTheme.colorScheme
    Card(
        modifier = modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
        shape = MaterialTheme.shapes.large,
        colors =
            CardDefaults.cardColors(
                containerColor = if (error) scheme.errorContainer else scheme.secondaryContainer,
                contentColor = if (error) scheme.onErrorContainer else scheme.onSecondaryContainer,
            ),
    ) {
        Row(
            Modifier.padding(start = 16.dp, end = 8.dp, top = 12.dp, bottom = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(icon, contentDescription = null)
            Spacer(Modifier.width(16.dp))
            Text(text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
            if (action != null && onAction != null) {
                TextButton(onClick = onAction) { Text(action) }
            }
        }
    }
}

@Composable
fun Field(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    supporting: String? = null,
    secret: Boolean = false,
    enabled: Boolean = true,
    keyboardType: KeyboardType = KeyboardType.Text,
) {
    var revealed by rememberSaveable { mutableStateOf(false) }
    OutlinedTextField(
        value = value,
        onValueChange = onValueChange,
        label = { Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        supportingText = supporting?.let { { Text(it) } },
        singleLine = true,
        enabled = enabled,
        visualTransformation =
            if (secret && !revealed) PasswordVisualTransformation() else VisualTransformation.None,
        keyboardOptions =
            KeyboardOptions(
                capitalization = KeyboardCapitalization.None,
                autoCorrectEnabled = !secret && keyboardType == KeyboardType.Text,
                keyboardType = if (secret) KeyboardType.Password else keyboardType,
            ),
        trailingIcon =
            if (secret) {
                {
                    IconButton(onClick = { revealed = !revealed }) {
                        Icon(
                            if (revealed) Icons.Rounded.VisibilityOff else Icons.Rounded.Visibility,
                            contentDescription = if (revealed) "Hide $label" else "Show $label",
                        )
                    }
                }
            } else null,
        shape = MaterialTheme.shapes.large,
        modifier =
            modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp),
    )
}

@Composable
fun ButtonRow(modifier: Modifier = Modifier, start: Boolean = false, content: @Composable () -> Unit) {
    androidx.compose.foundation.layout.FlowRow(
        modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp, if (start) Alignment.Start else Alignment.End),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        content()
    }
}

@Composable
fun SmallLoading(modifier: Modifier = Modifier) {
    LoadingIndicator(modifier.size(24.dp))
}

@Composable
fun CodeText(code: String, modifier: Modifier = Modifier) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerHighest,
        shape = MaterialTheme.shapes.large,
        modifier = modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
    ) {
        SelectionContainer {
            Text(
                code,
                style =
                    MaterialTheme.typography.titleLarge.copy(
                        fontFamily = FontFamily.Monospace,
                        fontWeight = FontWeight.Bold,
                    ),
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 16.dp),
            )
        }
    }
}

@Composable
fun rememberCopy(): (label: String, text: String) -> Unit {
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    return { label, text ->
        val data = ClipData.newPlainText(label, text)
        data.description.extras =
            PersistableBundle().apply { putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true) }
        scope.launch { clipboard.setClipEntry(ClipEntry(data)) }
    }
}
