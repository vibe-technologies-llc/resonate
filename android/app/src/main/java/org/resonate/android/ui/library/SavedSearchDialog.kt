package org.resonate.android.ui.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExposedDropdownMenu
import androidx.compose.material3.ExposedDropdownMenuAnchorType
import androidx.compose.material3.ExposedDropdownMenuBox
import androidx.compose.material3.ExposedDropdownMenuDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.SavedSearch
import org.resonate.android.nativecore.SearchOrder

data class SearchEditing(
    val title: String,
    val draft: SavedSearchDraft,
    val save: (String, SavedSearch) -> Unit,
)

@Composable
fun SavedSearchDialog(request: SearchEditing, dismiss: () -> Unit) {
    var name by rememberSaveable { mutableStateOf(request.draft.name) }
    var text by rememberSaveable { mutableStateOf(request.draft.text) }
    var order by rememberSaveable { mutableStateOf(request.draft.order) }
    var descending by rememberSaveable { mutableStateOf(request.draft.descending) }
    var cap by rememberSaveable { mutableStateOf(request.draft.cap) }
    val draft = SavedSearchDraft(name.trim(), text, order, descending, cap)
    val search = draft.search()
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text(request.title) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it.take(NAME_AT_MOST) },
                    label = { Text("Playlist name") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("saved-search-name"),
                )
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it },
                    label = { Text("Search") },
                    supportingText = { Text("Blank asks every song") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth().testTag("saved-search-query"),
                )
                OrderMenu(order) {
                    val moved = draft.ordered(it)
                    order = moved.order
                    descending = moved.descending
                }
                Row(
                    Modifier.fillMaxWidth()
                        .toggleable(value = descending, role = Role.Switch, onValueChange = { descending = it })
                        .testTag("saved-search-descending"),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween,
                ) {
                    Text("Descending", style = MaterialTheme.typography.bodyLarge)
                    Switch(checked = descending, onCheckedChange = null)
                }
                OutlinedTextField(
                    value = cap,
                    onValueChange = { cap = it.take(CAP_DIGITS_AT_MOST) },
                    label = { Text("Limit") },
                    supportingText = {
                        Text(if (search == null) "A whole number above zero, or blank for every row" else "Blank keeps every row")
                    },
                    isError = search == null,
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.fillMaxWidth().testTag("saved-search-cap"),
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = {
                    if (search != null && draft.name.isNotEmpty()) {
                        request.save(draft.name, search)
                        dismiss()
                    }
                },
                enabled = search != null && draft.name.isNotEmpty(),
                modifier = Modifier.testTag("saved-search-save"),
            ) {
                Text("Save")
            }
        },
        dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } },
    )
}

@Composable
private fun OrderMenu(order: SearchOrder, onChoose: (SearchOrder) -> Unit) {
    var open by remember { mutableStateOf(false) }
    ExposedDropdownMenuBox(expanded = open, onExpandedChange = { open = it }) {
        OutlinedTextField(
            value = order.label,
            onValueChange = {},
            readOnly = true,
            singleLine = true,
            label = { Text("Order") },
            trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(expanded = open) },
            modifier =
                Modifier.fillMaxWidth()
                    .menuAnchor(ExposedDropdownMenuAnchorType.PrimaryNotEditable)
                    .testTag("saved-search-order-menu"),
        )
        ExposedDropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            SearchOrder.entries.forEach { choice ->
                DropdownMenuItem(
                    text = { Text(choice.label) },
                    onClick = {
                        open = false
                        if (choice != order) onChoose(choice)
                    },
                    contentPadding = ExposedDropdownMenuDefaults.ItemContentPadding,
                )
            }
        }
    }
}

private const val NAME_AT_MOST = 200

private const val CAP_DIGITS_AT_MOST = 9
