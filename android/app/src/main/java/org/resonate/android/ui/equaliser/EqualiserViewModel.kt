package org.resonate.android.ui.equaliser

import android.content.ContentResolver
import android.net.Uri
import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.conflate
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.mapLatest
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.OutputRoute
import org.resonate.android.RoomView
import org.resonate.android.readUpTo
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.EqualiserPreset
import org.resonate.android.nativecore.PlaybackPreferences
import org.resonate.android.nativecore.equaliserFrequencies
import org.resonate.android.nativecore.equaliserLimits
import org.resonate.android.nativecore.equaliserPresets
import org.resonate.android.nativecore.fitMeasurement
import org.resonate.android.nativecore.fittedPreamp
import org.resonate.android.nativecore.measurementOffers
import org.resonate.android.nativecore.parseEqProfile
import org.resonate.android.nativecore.responseCurve
import org.resonate.android.ui.Resonate

data class EqualiserInfo(
    val bounds: EqualiserBounds,
    val frequencies: List<Float>,
    val presets: List<EqualiserPreset>,
)

private const val RESPONSE_POINTS = 256u

@OptIn(ExperimentalCoroutinesApi::class)
class EqualiserViewModel(private val resonate: Resonate) : ViewModel() {
    private val library = resonate.library
    private val draft = MutableStateFlow<EqualiserCurve?>(null)
    private var holding = false
    private val mutableInfo = MutableStateFlow<EqualiserInfo?>(null)

    val info: StateFlow<EqualiserInfo?> = mutableInfo.asStateFlow()

    val preferences: StateFlow<PlaybackPreferences?> = library.preferences

    private val stored = library.preferences.map { it?.equaliser }.distinctUntilChanged()

    val shown: StateFlow<EqualiserCurve?> =
        combine(stored, draft) { kept, edited -> edited ?: kept }
            .distinctUntilChanged()
            .stateIn(viewModelScope, SharingStarted.Eagerly, null)

    val response: StateFlow<List<Float>> =
        shown
            .filterNotNull()
            .conflate()
            .mapLatest { curve -> withContext(Dispatchers.Default) { responseOf(curve) } }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    val route: StateFlow<OutputRoute?> = library.route

    val boundRoutes: StateFlow<List<OutputRoute>> = library.boundRoutes

    val room: StateFlow<RoomView> = resonate.features.room.view

    private val measuring = MutableStateFlow(MeasurementView.unread)

    val measurement: StateFlow<MeasurementView> = measuring.asStateFlow()

    init {
        viewModelScope.launch {
            mutableInfo.value =
                withContext(Dispatchers.Default) {
                    EqualiserInfo(
                        EqualiserBounds.of(equaliserLimits()),
                        equaliserFrequencies().map { it.toFloat() },
                        equaliserPresets(),
                    )
                }
        }
        viewModelScope.launch {
            val offers = withContext(Dispatchers.Default) { measurementOffers() }
            measuring.update { if (it.offers == null) MeasurementView.of(offers) else it }
        }
        viewModelScope.launch { stored.collect { if (!holding) draft.value = null } }
    }

    private val outputRate: UInt?
        get() = library.snapshot.value?.outputRate?.takeIf { it != 0u }

    private fun responseOf(curve: EqualiserCurve): List<Float> =
        runCatching { responseCurve(curve, RESPONSE_POINTS, outputRate) }
            .recoverCatching { responseCurve(curve, RESPONSE_POINTS, null) }
            .onFailure { Log.w("Resonate", "The equaliser response could not be drawn", it) }
            .getOrDefault(emptyList())

    fun enable(enabled: Boolean) {
        library.updatePreferences { it.copy(equaliserEnabled = enabled) }
    }

    fun choose(preset: EqualiserPreset) {
        holding = false
        draft.value = preset.curve
        library.updatePreferences { it.withPreset(preset) }
        settle(preset.curve)
    }

    fun hold(curve: EqualiserCurve) {
        holding = true
        draft.value = curve
    }

    fun release() {
        holding = false
        draft.value = null
    }

    fun commit(curve: EqualiserCurve) {
        holding = false
        draft.value = curve
        library.updatePreferences { it.withCurve(curve) }
        settle(curve)
    }

    fun reset() {
        holding = false
        draft.value = flatCurve
        library.updatePreferences { it.reset() }
        settle(flatCurve)
    }

    private fun settle(curve: EqualiserCurve) {
        library.request { draft.compareAndSet(curve, null) }
    }

    fun fitPreamp(curve: EqualiserCurve) {
        val bounds = info.value?.bounds ?: EqualiserBounds.unread
        val rate = outputRate
        resonate.attempt(viewModelScope) { native ->
            val fitted = withContext(Dispatchers.Default) { fittedPreamp(curve, rate) }
            val modelled = curve.withFittedPreamp(fitted, bounds)
            commit(modelled)
            val measured =
                withContext(Dispatchers.Default) { native.fittedPreampToTheQueue(curve) }
            if (measured != null && shown.value == modelled) {
                commit(curve.withFittedPreamp(measured, bounds))
            }
        }
    }

    fun importProfile(name: String, text: String, onApplied: () -> Unit) {
        resonate.attempt(viewModelScope) { _ ->
            val found =
                withContext(Dispatchers.Default) {
                    parseEqProfile(text.toByteArray(Charsets.UTF_8), name.trim())
                }
            commit(found.curve)
            onApplied()
        }
    }

    fun aimAt(aim: Aim) = measuring.update { it.aimed(aim) }

    fun aimAtDocument(resolver: ContentResolver, uri: Uri) {
        val offers = measuring.value.offers ?: return
        resonate.attempt(viewModelScope) { _ ->
            val read = withContext(Dispatchers.IO) { resolver.readUpTo(uri, offers.bytesAtMost) }
            measuring.update { it.aimed(Aim.Document(read.name, read.bytes)) }
        }
    }

    fun fitFrom(resolver: ContentResolver, uri: Uri) {
        val view = measuring.value
        val offers = view.offers ?: return
        val aim = view.aim ?: return
        if (view.fitting) return
        measuring.update { it.begun() }
        resonate.attempt(viewModelScope) { _ ->
            try {
                val read = withContext(Dispatchers.IO) { resolver.readUpTo(uri, offers.bytesAtMost) }
                val fit =
                    withContext(Dispatchers.Default) {
                        fitMeasurement(read.bytes, aim.target(), profileNameFor(read.name))
                    }
                commit(fit.imported.curve)
                measuring.update { it.finished(fitSentence(fit, aim.label(offers.targets))) }
            } finally {
                measuring.update { it.finished(null) }
            }
        }
    }

    fun bind(bound: Boolean) = library.bindEqualiser(bound)

    fun forget(route: OutputRoute) = library.forgetEqualiser(route)

    fun chooseRoomResponse() = resonate.host.chooseRoomResponse()

    fun turnRoom(on: Boolean) = resonate.features.room.turn(on)

    fun forgetRoom() = resonate.features.room.forget()
}
