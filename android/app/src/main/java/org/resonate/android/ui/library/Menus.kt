package org.resonate.android.ui.library

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material3.DropdownMenuGroup
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.DropdownMenuPopup
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.unit.dp
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Switch

private val CHOICE_ROW_HEIGHT = 40.dp
private val CHOICE_ROW_PADDING = PaddingValues(horizontal = 12.dp, vertical = 4.dp)

sealed interface MenuEntry {
    val label: String

    data class Action(
        override val label: String,
        val icon: ImageVector?,
        val destructive: Boolean = false,
        val supporting: String? = null,
        val run: () -> Unit,
    ) : MenuEntry

    data class Choice(
        override val label: String,
        val selected: Boolean,
        val run: () -> Unit,
    ) : MenuEntry

    data class Toggle(
        override val label: String,
        val icon: ImageVector?,
        val checked: Boolean,
        val change: (Boolean) -> Unit,
    ) : MenuEntry
}

@Composable
fun GroupedMenu(expanded: Boolean, onDismiss: () -> Unit, groups: List<List<MenuEntry>>) {
    val shown = groups.filter { it.isNotEmpty() }
    DropdownMenuPopup(expanded = expanded, onDismissRequest = onDismiss) {
        shown.forEachIndexed { groupIndex, group ->
            if (groupIndex > 0) Spacer(Modifier.height(MenuDefaults.GroupSpacing))
            DropdownMenuGroup(shapes = MenuDefaults.groupShape(groupIndex, shown.size)) {
                group.forEachIndexed { index, entry ->
                    MenuRow(entry, MenuDefaults.itemShape(index, group.size), onDismiss)
                }
            }
        }
    }
}

@Composable
private fun MenuRow(
    entry: MenuEntry,
    shapes: androidx.compose.material3.MenuItemShapes,
    onDismiss: () -> Unit,
) {
    when (entry) {
        is MenuEntry.Action -> {
            val tint =
                if (entry.destructive) MaterialTheme.colorScheme.error
                else MaterialTheme.colorScheme.onSurfaceVariant
            DropdownMenuItem(
                onClick = {
                    onDismiss()
                    entry.run()
                },
                text = {
                    Text(
                        entry.label,
                        color =
                            if (entry.destructive) MaterialTheme.colorScheme.error
                            else MaterialTheme.colorScheme.onSurface,
                    )
                },
                supportingText = entry.supporting?.let { { Text(it) } },
                shape = shapes.shape,
                leadingIcon = entry.icon?.let { { Icon(it, contentDescription = null, tint = tint) } },
            )
        }
        is MenuEntry.Choice ->
            DropdownMenuItem(
                onClick = {
                    onDismiss()
                    entry.run()
                },
                text = { Text(entry.label) },
                shape = shapes.shape,
                leadingIcon = { MarkIcon(entry.selected) },
                contentPadding = CHOICE_ROW_PADDING,
                modifier = Modifier.height(CHOICE_ROW_HEIGHT).semantics { selected = entry.selected },
            )
        is MenuEntry.Toggle ->
            DropdownMenuItem(
                onClick = { entry.change(!entry.checked) },
                text = { Text(entry.label) },
                shape = shapes.shape,
                leadingIcon = entry.icon?.let { { Icon(it, contentDescription = null) } },
                trailingContent = { Switch(checked = entry.checked, onCheckedChange = null) },
                modifier = Modifier.semantics { toggleableState = ToggleableState(entry.checked) },
            )
    }
}

@Composable
private fun MarkIcon(marked: Boolean) {
    if (marked) Icon(Icons.Rounded.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
    else Spacer(Modifier.size(24.dp))
}

@Composable
fun OverflowMenu(
    groups: List<List<MenuEntry>>,
    modifier: Modifier = Modifier,
    description: String = "More options",
    icon: ImageVector = Icons.Rounded.MoreVert,
    onOpen: () -> Unit = {},
) {
    var open by remember { mutableStateOf(false) }
    Box(modifier) {
        IconButton(
            onClick = {
                onOpen()
                open = true
            },
            modifier = Modifier.testTag("overflow"),
        ) {
            Icon(icon, contentDescription = description)
        }
        GroupedMenu(open, { open = false }, groups)
    }
}
