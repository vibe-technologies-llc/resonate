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
import androidx.compose.ui.res.stringResource
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
import org.resonate.android.R
import org.resonate.android.Words
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

fun currentOf(words: Words, preferences: PlaybackPreferences?): CurrentCurve? =
    preferences?.let { CurrentCurve(shownName(words, it.equaliser), it.equaliserEnabled) }

private const val TYPING_SETTLES_MS = 350L

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
            .map { currentOf(library.words, it) }
            .stateIn(
                viewModelScope,
                SharingStarted.WhileSubscribed(5_000),
                currentOf(library.words, library.preferences.value),
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
            library.notice(importSentence(library.words, found))
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
    Page(stringResource(R.string.equaliser_headphone_correction)) { padding ->
        SettingsList(padding, Modifier.testTag("correction-list")) {
            item(key = "current") {
                val curve = current
                SettingRow(
                    title = curve?.name ?: stringResource(R.string.equaliser_reading_curve),
                    icon = Icons.Rounded.Equalizer,
                    supporting =
                        when {
                            curve == null -> null
                            curve.enabled -> stringResource(R.string.equaliser_curve_in_use_on)
                            else -> stringResource(R.string.equaliser_curve_in_use_off)
                        },
                    modifier = Modifier.testTag("correction-current"),
                )
            }
            item(key = "autoeq") {
                SectionHeader("AutoEq")
                Note(stringResource(R.string.equaliser_autoeq_note))
                Column(Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp)) {
                    OutlinedTextField(
                        value = query,
                        onValueChange = model::type,
                        label = { Text(stringResource(R.string.equaliser_autoeq_search)) },
                        placeholder = {
                            Text(stringResource(R.string.equaliser_autoeq_placeholder))
                        },
                        leadingIcon = { Icon(Icons.Rounded.Search, null) },
                        trailingIcon = {
                            if (query.isNotEmpty()) {
                                IconButton(onClick = { model.type("") }) {
                                    Icon(
                                        Icons.Rounded.Close,
                                        contentDescription =
                                            stringResource(R.string.equaliser_clear_search),
                                    )
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
                            stringResource(R.string.equaliser_autoeq_offline),
                        )
                    }
                results.isEmpty() && !busy && searched ->
                    item(key = "empty") {
                        Note(stringResource(R.string.equaliser_autoeq_none))
                    }
                else ->
                    items(results, key = { it.id }) { device ->
                        val named = measured(device.name)
                        val by =
                            named.source?.let {
                                stringResource(R.string.equaliser_measured_by, it)
                            }
                        SettingRow(
                            title = named.headphone,
                            icon = Icons.Rounded.Headphones,
                            supporting = by,
                            onClick = { confirming = device.id to device.name },
                            modifier = Modifier.testTag("correction-${device.id}"),
                        )
                    }
            }
            item(key = "import") {
                SectionHeader(stringResource(R.string.equaliser_import))
                Note(stringResource(R.string.equaliser_import_note))
                Field(
                    value = model.profileName,
                    onValueChange = { model.profileName = it },
                    label = stringResource(R.string.equaliser_import_name),
                    modifier = Modifier.testTag("import-name"),
                )
                OutlinedTextField(
                    value = model.profileText,
                    onValueChange = { model.profileText = it },
                    label = { Text(stringResource(R.string.equaliser_import_text)) },
                    placeholder = { Text(stringResource(R.string.equaliser_import_example)) },
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
                        Text(stringResource(R.string.equaliser_import_apply))
                    }
                }
            }
        }
    }
    confirming?.let { (id, name) ->
        ConfirmDialog(
            title = stringResource(R.string.equaliser_replace_title),
            text = stringResource(R.string.equaliser_replace_text, measured(name).headphone),
            confirm = stringResource(R.string.equaliser_apply),
            onConfirm = { model.apply(AutoEqDevice(id, name)) },
            onDismiss = { confirming = null },
            icon = Icons.Rounded.Headphones,
        )
    }
}
