package org.resonate.android.ui.equaliser

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.Headphones
import androidx.compose.material.icons.rounded.HeadsetOff
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import org.resonate.android.nativecore.AutoEqDevice
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.EmptyState
import org.resonate.android.ui.components.IconTile
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel

data class CurrentCurve(val name: String, val enabled: Boolean)

private const val TYPING_SETTLES_MS = 350L

@OptIn(FlowPreview::class)
class HeadphoneCorrectionViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features
    private var searchedFor: String? = null
    var query by mutableStateOf("")
        private set
    val searched = MutableStateFlow(false)
    val results: StateFlow<List<AutoEqDevice>> = features.corrections
    val busy: StateFlow<Boolean> =
        combine(features.correctionBusy, features.correctionApplying) { searching, applying -> searching || applying }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), false)
    val online: StateFlow<Boolean> = features.online

    val current: StateFlow<CurrentCurve?> =
        resonate.library.preferences
            .map { it?.let { kept -> CurrentCurve(shownName(kept.equaliser), kept.equaliserEnabled) } }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)

    init {
        viewModelScope.launch {
            snapshotFlow { query }.drop(1).debounce(TYPING_SETTLES_MS).map { it.trim() }.distinctUntilChanged().collect { text ->
                searchNow(text)
            }
        }
    }

    fun type(text: String) {
        query = text
        if (text.isBlank()) clear()
    }

    fun submit() {
        searchedFor = null
        searchNow(query.trim())
    }

    private fun searchNow(text: String) {
        if (text.isEmpty()) {
            clear()
            return
        }
        if (text == searchedFor) return
        searchedFor = text
        searched.value = true
        features.searchCorrections(text)
    }

    private fun clear() {
        searchedFor = null
        searched.value = false
        features.clearCorrections()
    }

    fun apply(device: AutoEqDevice) = features.applyCorrection(device.id, measured(device.name).headphone)
}

@Composable
fun HeadphoneCorrectionScreen() {
    val model = resonateViewModel { HeadphoneCorrectionViewModel(it) }
    val query = model.query
    val results by model.results.collectAsStateWithLifecycle()
    val busy by model.busy.collectAsStateWithLifecycle()
    val online by model.online.collectAsStateWithLifecycle()
    val searched by model.searched.collectAsStateWithLifecycle()
    val current by model.current.collectAsStateWithLifecycle()
    var confirming by rememberSaveable { mutableStateOf<Pair<String, String>?>(null) }
    val focus = LocalFocusManager.current
    Page("Headphone correction") { padding ->
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
            LazyColumn(
                contentPadding = padding,
                modifier = Modifier.widthIn(max = 840.dp).fillMaxWidth().testTag("correction-list"),
            ) {
                item(key = "search") {
                    Column(Modifier.padding(horizontal = Sizes.gutter, vertical = 8.dp)) {
                        OutlinedTextField(
                            value = query,
                            onValueChange = model::type,
                            label = { Text("Search AutoEq") },
                            placeholder = { Text("Headphone brand or model") },
                            leadingIcon = { Icon(Icons.Rounded.Search, null) },
                            trailingIcon = {
                                if (query.isNotEmpty()) {
                                    IconButton(onClick = { model.type("") }) {
                                        Icon(Icons.Rounded.Close, contentDescription = "Clear search")
                                    }
                                }
                            },
                            singleLine = true,
                            enabled = online,
                            shape = CircleShape,
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                            keyboardActions =
                                KeyboardActions(onSearch = {
                                    focus.clearFocus()
                                    model.submit()
                                }),
                            colors = OutlinedTextFieldDefaults.colors(),
                            modifier = Modifier.fillMaxWidth().testTag("correction-search"),
                        )
                        Box(Modifier.fillMaxWidth().height(12.dp).padding(top = 8.dp)) {
                            if (busy) LinearWavyProgressIndicator(Modifier.fillMaxWidth())
                        }
                    }
                }
                current?.let { curve ->
                    item(key = "current") {
                        ListItem(
                            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                            leadingContent = { IconTile(Icons.Rounded.Equalizer) },
                            overlineContent = { Text("Current curve") },
                            supportingContent = { Text(if (curve.enabled) "Equaliser on" else "Equaliser off") },
                        ) { Text(curve.name, maxLines = 2, overflow = TextOverflow.Ellipsis) }
                    }
                }
                when {
                    !online ->
                        item(key = "offline") {
                            EmptyState(
                                Icons.Rounded.CloudOff,
                                "Online features are off",
                                detail = "Turn them on in Settings to search AutoEq's measured corrections.",
                                modifier = Modifier.heightIn(min = 360.dp),
                            )
                        }
                    results.isEmpty() && !busy ->
                        item(key = "empty") {
                            if (searched) {
                                EmptyState(
                                    Icons.Rounded.HeadsetOff,
                                    "No headphones match",
                                    detail = "Try the brand alone, or fewer words of the model.",
                                    modifier = Modifier.heightIn(min = 360.dp),
                                )
                            } else {
                                EmptyState(
                                    Icons.Rounded.Headphones,
                                    "Find your headphones",
                                    detail = "AutoEq has measured corrections for thousands of headphones and earphones. Applying one replaces the equaliser curve.",
                                    modifier = Modifier.heightIn(min = 360.dp),
                                )
                            }
                        }
                    else -> {
                        item(key = "results") { SectionHeader("Results") }
                        items(results, key = { it.id }) { device ->
                            val named = measured(device.name)
                            ListItem(
                                onClick = { confirming = device.id to device.name },
                                modifier = Modifier.testTag("correction-${device.id}"),
                                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                                leadingContent = { IconTile(Icons.Rounded.Headphones) },
                                supportingContent =
                                    named.source?.let { source -> { Text("Measured by $source", maxLines = 2, overflow = TextOverflow.Ellipsis) } },
                            ) { Text(named.headphone, maxLines = 2, overflow = TextOverflow.Ellipsis) }
                        }
                    }
                }
            }
        }
    }
    confirming?.let { (id, name) ->
        AlertDialog(
            onDismissRequest = { confirming = null },
            icon = { Icon(Icons.Rounded.Headphones, null) },
            title = { Text("Replace equaliser curve?") },
            text = { Text("Apply ${measured(name).headphone}? It replaces the current curve and turns the equaliser on.") },
            confirmButton = {
                TextButton(onClick = {
                    confirming = null
                    model.apply(AutoEqDevice(id, name))
                }) { Text("Apply") }
            },
            dismissButton = { TextButton(onClick = { confirming = null }) { Text("Cancel") } },
        )
    }
}
