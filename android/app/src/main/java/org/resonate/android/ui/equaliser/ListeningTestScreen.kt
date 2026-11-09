package org.resonate.android.ui.equaliser

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.ListeningTest
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.TestArm
import org.resonate.android.nativecore.TestHear
import org.resonate.android.nativecore.TestedSetting
import org.resonate.android.nativecore.listeningTestLengths
import org.resonate.android.nativecore.settingTestByDefault
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.settings.ButtonRow
import org.resonate.android.ui.settings.Choice
import org.resonate.android.ui.settings.Note
import org.resonate.android.ui.settings.SettingsList
import org.resonate.android.ui.settings.phaseChoices
import org.resonate.android.ui.settings.qualityChoices
import org.resonate.android.ui.settings.restorationChoices

class ListeningTestViewModel(private val resonate: Resonate) : ViewModel() {
    private val library = resonate.library
    private val testing = MutableStateFlow(ListeningTestView.unread)

    val listening: StateFlow<ListeningTestView> = testing.asStateFlow()

    init {
        viewModelScope.launch {
            val lengths = withContext(Dispatchers.Default) { listeningTestLengths() }
            testing.update { it.copy(lengths = lengths) }
        }
    }

    fun chooseTesting(tested: Testing) {
        val view = testing.value
        if (view.run != null || view.beginning) return
        val kind = tested.kind
        val kept = library.preferences.value
        val against = if (kind == null || kept == null) null else settingTestByDefault(kind, kept)
        testing.update { it.testing(tested, against) }
    }

    fun against(setting: TestedSetting) {
        testing.update { it.against(setting) }
    }

    fun beginTest(trials: UInt) {
        val view = testing.value
        if (view.run != null || view.beginning) return
        val tested = view.testing
        val curve = library.preferences.value?.equaliser
        val setting = view.against
        val start: (MobilePlayer) -> ListeningTest =
            when {
                tested == Testing.Equaliser && curve != null -> { native ->
                        native.beginListeningTest(curve, trials)
                    }
                tested != Testing.Equaliser && setting != null -> { native ->
                        native.beginSettingTest(setting, trials)
                    }
                else -> return
            }
        testing.update { it.beginning() }
        resonate.attempt(viewModelScope) { native ->
            try {
                val test = withContext(Dispatchers.Default) { start(native) }
                testing.update { it.begun(test, tested) }
            } finally {
                testing.update { if (it.run == null) it.failedToBegin() else it }
            }
        }
    }

    fun hear(hear: TestHear) {
        val test = testing.value.run?.test ?: return
        resonate.attempt(viewModelScope) { native ->
            native.hearInListeningTest(test, hear)
            testing.update { it.stood(test.standing()) }
        }
    }

    fun say(arm: TestArm) {
        val test = testing.value.run?.test ?: return
        resonate.attempt(viewModelScope) { _ ->
            test.answer(arm)
            testing.update { it.stood(test.standing()) }
        }
    }

    fun endTest() {
        val run = testing.value.run ?: return
        testing.update { it.ended() }
        library.request { native -> native.endListeningTest() }
        run.test.close()
    }

    override fun onCleared() {
        endTest()
    }
}

@Composable
fun ListeningTestScreen() {
    val model = resonateViewModel { ListeningTestViewModel(it) }
    val view by model.listening.collectAsStateWithLifecycle()
    val run = view.run
    Page("Listening test") { padding ->
        SettingsList(padding, Modifier.testTag("listening-list")) {
            if (run == null) {
                item(key = "choose") {
                    SectionHeader("What is tested")
                    TestingChoices(view, model)
                    Note(view.testing.how)
                }
                val against = view.against?.takeIf { view.testing != Testing.Equaliser }
                if (against != null) {
                    item(key = "against") { Against(against, model) }
                }
                item(key = "begin") {
                    SectionHeader("Trials")
                    ButtonRow(start = true) {
                        if (view.lengths.isEmpty()) {
                            Spacer(Modifier.height(ButtonDefaults.MinHeight))
                        }
                        view.lengths.forEach { trials ->
                            FilledTonalButton(
                                onClick = { model.beginTest(trials) },
                                enabled = !view.beginning,
                                contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                                modifier = Modifier.testTag("listening-begin-$trials"),
                            ) {
                                Icon(
                                    Icons.Rounded.PlayArrow,
                                    null,
                                    Modifier.size(ButtonDefaults.IconSize),
                                )
                                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                                Text(
                                    if (view.beginning) beginningWords(view.testing)
                                    else "Begin $trials trials"
                                )
                            }
                        }
                    }
                }
            } else {
                item(key = "run") { Running(run, model) }
            }
        }
    }
}

@Composable
private fun Running(run: ListeningRun, model: ListeningTestViewModel) {
    val standing = run.standing
    SectionHeader(run.testing.label)
    Column(
        Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp)
            .semantics { liveRegion = LiveRegionMode.Polite }
            .testTag("listening-standing")
    ) {
        Text(standingWords(standing), style = MaterialTheme.typography.titleMedium)
        Text(
            heardWords(standing),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        if (standing.verdicts.isNotEmpty()) {
            Text(
                verdictsInALine(standing),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        lastWords(standing)?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (standing.finished) {
            Text(conclusionWords(standing), style = MaterialTheme.typography.bodyLarge)
        }
    }
    if (!standing.finished) {
        ButtonRow(start = true) {
            HearButton("A", TestHear.A, model)
            HearButton("B", TestHear.B, model)
            HearButton("X", TestHear.X, model)
        }
        ButtonRow(start = true) {
            SayButton("X is A", TestArm.A, model)
            SayButton("X is B", TestArm.B, model)
        }
    }
    ButtonRow(start = true) {
        OutlinedButton(
            onClick = model::endTest,
            contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
            modifier = Modifier.testTag("listening-end"),
        ) {
            Icon(Icons.Rounded.Stop, null, Modifier.size(ButtonDefaults.IconSize))
            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
            Text(if (standing.finished) "Done" else "End the test")
        }
    }
    Note(run.testing.how)
}

@Composable
private fun TestingChoices(view: ListeningTestView, model: ListeningTestViewModel) {
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Testing.entries.forEach { testing ->
            FilterChip(
                selected = view.testing == testing,
                onClick = { model.chooseTesting(testing) },
                enabled = !view.beginning,
                label = { Text(testing.label) },
                modifier = Modifier.testTag("listening-testing-${testing.name}"),
            )
        }
    }
}

@Composable
private fun Against(setting: TestedSetting, model: ListeningTestViewModel) {
    SectionHeader("Against")
    Note(playedAsWords(setting), Modifier.testTag("listening-a"))
    when (setting) {
        is TestedSetting.Restoration ->
            AgainstRow(
                "B is",
                restorationChoices.filter { it.value != setting.a },
                setting.b,
                "restoration",
            ) {
                model.against(setting.copy(b = it))
            }
        is TestedSetting.Resampling -> {
            AgainstRow("B's quality", qualityChoices, setting.bQuality, "quality") {
                model.against(setting.copy(bQuality = it))
            }
            AgainstRow("B's phase", phaseChoices, setting.bPhase, "phase") {
                model.against(setting.copy(bPhase = it))
            }
        }
        is TestedSetting.Dither ->
            AgainstRow(
                "B is",
                ditherChoices.filter { it.value != setting.a },
                setting.b,
                "dither",
            ) {
                model.against(setting.copy(b = it))
            }
    }
}

@Composable
private fun <T> AgainstRow(
    caption: String,
    choices: List<Choice<T>>,
    selected: T,
    tag: String,
    onChoose: (T) -> Unit,
) {
    Text(
        caption,
        style = MaterialTheme.typography.labelLarge,
        modifier =
            Modifier.padding(horizontal = Sizes.gutter, vertical = 4.dp).semantics { heading() },
    )
    FlowRow(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 4.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        choices.forEach { choice ->
            FilterChip(
                selected = selected == choice.value,
                onClick = { onChoose(choice.value) },
                label = { Text(choice.label) },
                modifier = Modifier.testTag("listening-against-$tag-${choice.label}"),
            )
        }
    }
}

@Composable
private fun HearButton(label: String, hear: TestHear, model: ListeningTestViewModel) {
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
private fun SayButton(label: String, arm: TestArm, model: ListeningTestViewModel) {
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
