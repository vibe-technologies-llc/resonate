package org.resonate.android

import android.content.SharedPreferences
import android.util.Log
import java.io.IOException
import org.resonate.android.nativecore.EqualiserCurve
import org.resonate.android.nativecore.GainMode
import org.resonate.android.nativecore.LossyRestoration
import org.resonate.android.nativecore.PlaybackPreferences
import org.resonate.android.nativecore.ResamplingPhase
import org.resonate.android.nativecore.ResamplingQuality
import org.resonate.android.nativecore.defaultPlaybackPreferences
import org.resonate.android.nativecore.equaliserText
import org.resonate.android.nativecore.parseEqProfile

class PlaybackPreferencesStore(private val storage: SharedPreferences) {
    fun read(): PlaybackPreferences {
        val defaults = defaultPlaybackPreferences()
        return defaults.copy(
            equaliserEnabled = storage.getBoolean("equaliser-enabled", defaults.equaliserEnabled),
            equaliser = storedCurve() ?: defaults.equaliser,
            replayGain =
                GainMode.entries.find { it.name == storage.getString("replay-gain", null) }
                    ?: defaults.replayGain,
            quality =
                ResamplingQuality.entries.find { it.name == storage.getString("quality", null) }
                    ?: defaults.quality,
            phase =
                ResamplingPhase.entries.find { it.name == storage.getString("phase", null) }
                    ?: defaults.phase,
            bufferMs =
                storage.getInt("buffer-ms", defaults.bufferMs.toInt()).coerceIn(50, 2000).toUInt(),
            dsdLikePcm = storage.getBoolean("dsd-like-pcm", defaults.dsdLikePcm),
            truePeak = storage.getBoolean("true-peak", defaults.truePeak),
            restoration =
                LossyRestoration.entries.find { it.name == storage.getString("restoration", null) }
                    ?: defaults.restoration,
        )
    }

    fun read(route: OutputRoute?): PlaybackPreferences {
        val preferences = read()
        val bound = route?.let(::boundCurve) ?: return preferences
        return preferences.copy(equaliser = bound)
    }

    fun write(preferences: PlaybackPreferences) {
        val edit = choices(storage.edit(), preferences)
        edit.putString(CURVE, equaliserText(preferences.equaliser))
        edit.putString(CURVE_NAME, preferences.equaliser.name)
        if (!edit.commit()) throw IOException("Sound settings could not be saved")
    }

    fun write(preferences: PlaybackPreferences, route: OutputRoute?) {
        if (route == null || !isBound(route)) return write(preferences)
        val edit = choices(storage.edit(), preferences)
        keepCurve(edit, route, preferences.equaliser)
        if (!edit.commit()) throw IOException("Sound settings could not be saved")
    }

    fun followsSongRate(): Boolean = storage.getBoolean(FOLLOW_SONG_RATE, false)

    fun keepFollowsSongRate(follows: Boolean) {
        if (!storage.edit().putBoolean(FOLLOW_SONG_RATE, follows).commit())
            throw IOException("Whether to match the song's sample rate could not be saved")
    }

    fun boundRoutes(): List<OutputRoute> =
        storage.all
            .mapNotNull { (key, value) ->
                if (key.startsWith(ROUTE_NAMES) && value is String)
                    OutputRoute(key.removePrefix(ROUTE_NAMES), value)
                else null
            }
            .sortedBy { it.name }

    fun isBound(route: OutputRoute): Boolean = storage.contains(ROUTE_CURVES + route.id)

    fun bind(route: OutputRoute, curve: EqualiserCurve) {
        val edit = storage.edit().putString(ROUTE_NAMES + route.id, route.name)
        keepCurve(edit, route, curve)
        if (!edit.commit()) throw IOException("The curve could not be kept for this output")
    }

    fun unbind(route: OutputRoute) {
        val edit =
            storage
                .edit()
                .remove(ROUTE_CURVES + route.id)
                .remove(ROUTE_CURVE_NAMES + route.id)
                .remove(ROUTE_NAMES + route.id)
        if (!edit.commit()) throw IOException("The curve could not be forgotten for this output")
    }

    private fun choices(
        edit: SharedPreferences.Editor,
        preferences: PlaybackPreferences,
    ): SharedPreferences.Editor =
        edit
            .putBoolean("equaliser-enabled", preferences.equaliserEnabled)
            .putString("replay-gain", preferences.replayGain.name)
            .putString("quality", preferences.quality.name)
            .putString("phase", preferences.phase.name)
            .putInt("buffer-ms", preferences.bufferMs.toInt())
            .putBoolean("dsd-like-pcm", preferences.dsdLikePcm)
            .putBoolean("true-peak", preferences.truePeak)
            .putString("restoration", preferences.restoration.name)

    private fun keepCurve(
        edit: SharedPreferences.Editor,
        route: OutputRoute,
        curve: EqualiserCurve,
    ) {
        edit.putString(ROUTE_CURVES + route.id, equaliserText(curve))
        edit.putString(ROUTE_CURVE_NAMES + route.id, curve.name)
    }

    private fun boundCurve(route: OutputRoute): EqualiserCurve? {
        val text = storage.getString(ROUTE_CURVES + route.id, null) ?: return null
        val name = storage.getString(ROUTE_CURVE_NAMES + route.id, "").orEmpty()
        return parsedCurve(text, name)
    }

    private fun storedCurve(): EqualiserCurve? {
        val kept = storage.getString(CURVE, null) ?: return null
        val name = storage.getString(CURVE_NAME, "").orEmpty()
        return parsedCurve(kept, name)
    }

    private fun parsedCurve(text: String, name: String): EqualiserCurve? =
        try {
            parseEqProfile(text.toByteArray(Charsets.UTF_8), name).curve
        } catch (failure: Exception) {
            Log.w("Resonate", "A kept equaliser curve could not be read", failure)
            null
        }

    private companion object {
        const val CURVE = "equaliser-curve"
        const val CURVE_NAME = "equaliser-curve-name"
        const val ROUTE_CURVES = "equaliser-curve-for."
        const val ROUTE_CURVE_NAMES = "equaliser-curve-name-for."
        const val ROUTE_NAMES = "equaliser-route-names."
        const val FOLLOW_SONG_RATE = "follow-song-rate"
    }
}
