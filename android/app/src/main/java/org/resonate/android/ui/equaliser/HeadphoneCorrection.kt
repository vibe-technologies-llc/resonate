package org.resonate.android.ui.equaliser

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.CloudOff
import androidx.compose.material.icons.rounded.Equalizer
import androidx.compose.material.icons.rounded.Headphones
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
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
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.AutoEqDevice
import org.resonate.android.nativecore.PlaybackPreferences
import org.resonate.android.nativecore.parseEqProfile
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.settings.ButtonRow
import org.resonate.android.ui.settings.ConfirmDialog
import org.resonate.android.ui.settings.Field
import org.resonate.android.ui.settings.Note
import org.resonate.android.ui.settings.NoticeCard
import org.resonate.android.ui.settings.SettingRow
import org.resonate.android.ui.settings.SettingsList

data class CurrentCurve(val name: String, val enabled: Boolean)

fun currentOf(preferences: PlaybackPreferences?): CurrentCurve? =
    preferences?.let { CurrentCurve(shownName(it.equaliser), it.equaliserEnabled) }

private const val TYPING_SETTLES_MS = 350L

private const val PROFILE_EXAMPLE = "Preamp: -6 dB\nFilter 1: ON PK Fc 100 Hz Gain 3 dB Q 0.7"

@OptIn(FlowPreview::class)
class HeadphoneCorrectionViewModel(private val resonate: Resonate) : ViewModel() {
    private val features = resonate.features
    private val library = resonate.library
    private var searchedFor: String? = null
    var query by mutableStateOf("")
        private set

    var profileName by mutableStateOf("")

    var profileText by mutableStateOf("")

    val searched = MutableStateFlow(false)
    val results: StateFlow<List<AutoEqDevice>> = features.corrections
    val busy: StateFlow<Boolean> =
        combine(features.correctionBusy, features.correctionApplying) { searching, applying ->
                searching || applying
            }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), false)
    val online: StateFlow<Boolean> = features.online

    val current: StateFlow<CurrentCurve?> =
        library.preferences
            .map(::currentOf)
            .stateIn(
                viewModelScope,
                SharingStarted.WhileSubscribed(5_000),
                currentOf(library.preferences.value),
            )

    init {
        viewModelScope.launch {
            snapshotFlow { query }
                .drop(1)
                .debounce(TYPING_SETTLES_MS)
                .map { it.trim() }
                .distinctUntilChanged()
                .collect { text -> searchNow(text) }
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

    fun apply(device: AutoEqDevice) =
        features.applyCorrection(device.id, measured(device.name).headphone)

    fun importProfile() {
        val name = profileName.trim()
        val text = profileText
        resonate.attempt(viewModelScope) { _ ->
            val found =
                withContext(Dispatchers.Default) {
                    parseEqProfile(text.toByteArray(Charsets.UTF_8), name)
                }
            library.updatePreferences { it.withCurve(found.curve) }
            library.notice(importSentence(found))
            profileName = ""
            profileText = ""
        }
    }
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
        SettingsList(padding, Modifier.testTag("correction-list")) {
            item(key = "current") {
                val curve = current
                SettingRow(
                    title = curve?.name ?: "Reading the curve…",
                    icon = Icons.Rounded.Equalizer,
                    supporting =
                        when {
                            curve == null -> null
                            curve.enabled -> "The curve in use · Equaliser on"
                            else -> "The curve in use · Equaliser off"
                        },
                    modifier = Modifier.testTag("correction-current"),
                )
            }
            item(key = "autoeq") {
                SectionHeader("AutoEq")
                Note(
                    "Measured corrections for thousands of headphones and earphones. Applying one replaces the curve."
                )
                Column(Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp)) {
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
                            KeyboardActions(
                                onSearch = {
                                    focus.clearFocus()
                                    model.submit()
                                }
                            ),
                        modifier = Modifier.fillMaxWidth().testTag("correction-search"),
                    )
                    Box(Modifier.fillMaxWidth().height(12.dp).padding(top = 8.dp)) {
                        if (busy) LinearWavyProgressIndicator(Modifier.fillMaxWidth())
                    }
                }
            }
            when {
                !online ->
                    item(key = "offline") {
                        NoticeCard(
                            Icons.Rounded.CloudOff,
                            "Online features are off. Turn them on in Settings to search AutoEq.",
                        )
                    }
                results.isEmpty() && !busy && searched ->
                    item(key = "empty") {
                        Note(
                            "No headphones match. Try the brand alone, or fewer words of the model."
                        )
                    }
                else ->
                    items(results, key = { it.id }) { device ->
                        val named = measured(device.name)
                        SettingRow(
                            title = named.headphone,
                            icon = Icons.Rounded.Headphones,
                            supporting = named.source?.let { "Measured by $it" },
                            onClick = { confirming = device.id to device.name },
                            modifier = Modifier.testTag("correction-${device.id}"),
                        )
                    }
            }
            item(key = "import") {
                SectionHeader("Import a profile")
                Note(
                    "Paste EqualizerAPO or GraphicEQ text. Parametric filters load as bands; a GraphicEQ line is fitted onto bands."
                )
                Field(
                    value = model.profileName,
                    onValueChange = { model.profileName = it },
                    label = "Profile name",
                    modifier = Modifier.testTag("import-name"),
                )
                OutlinedTextField(
                    value = model.profileText,
                    onValueChange = { model.profileText = it },
                    label = { Text("Profile text") },
                    placeholder = { Text(PROFILE_EXAMPLE) },
                    minLines = 4,
                    maxLines = 10,
                    shape = MaterialTheme.shapes.large,
                    modifier =
                        Modifier.fillMaxWidth()
                            .padding(horizontal = Sizes.gutter, vertical = 4.dp)
                            .testTag("import-text"),
                )
                ButtonRow {
                    Button(
                        onClick = model::importProfile,
                        enabled = model.profileText.isNotBlank(),
                        modifier = Modifier.testTag("import-apply"),
                    ) {
                        Text("Apply profile")
                    }
                }
            }
        }
    }
    confirming?.let { (id, name) ->
        ConfirmDialog(
            title = "Replace the curve?",
            text =
                "Apply ${measured(name).headphone}? It replaces the current curve and turns the equaliser on.",
            confirm = "Apply",
            onConfirm = { model.apply(AutoEqDevice(id, name)) },
            onDismiss = { confirming = null },
            icon = Icons.Rounded.Headphones,
        )
    }
}
