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

    fun write(preferences: PlaybackPreferences) {
        val edit =
            storage
                .edit()
                .putBoolean("equaliser-enabled", preferences.equaliserEnabled)
                .putString(CURVE, equaliserText(preferences.equaliser))
                .putString(CURVE_NAME, preferences.equaliser.name)
                .putString("replay-gain", preferences.replayGain.name)
                .putString("quality", preferences.quality.name)
                .putString("phase", preferences.phase.name)
                .putInt("buffer-ms", preferences.bufferMs.toInt())
                .putBoolean("dsd-like-pcm", preferences.dsdLikePcm)
                .putBoolean("true-peak", preferences.truePeak)
                .putString("restoration", preferences.restoration.name)
        if (!edit.commit()) throw IOException("Sound settings could not be saved")
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
    }
}
