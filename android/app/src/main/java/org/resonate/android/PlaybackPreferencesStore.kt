package org.resonate.android

import android.content.SharedPreferences
import java.io.IOException
import org.resonate.android.nativecore.GainMode
import org.resonate.android.nativecore.LossyRestoration
import org.resonate.android.nativecore.PlaybackPreferences
import org.resonate.android.nativecore.ResamplingPhase
import org.resonate.android.nativecore.ResamplingQuality
import org.resonate.android.nativecore.defaultPlaybackPreferences

class PlaybackPreferencesStore(private val storage: SharedPreferences) {
    fun read(): PlaybackPreferences {
        val defaults = defaultPlaybackPreferences()
        return defaults.copy(
            equaliserEnabled = storage.getBoolean("equaliser-enabled", defaults.equaliserEnabled),
            preampDb = finiteLevel("equaliser-preamp", defaults.preampDb, -24f..12f),
            gainsDb =
                defaults.gainsDb.mapIndexed { index, gain ->
                    finiteLevel("equaliser-gain-$index", gain, -12f..12f)
                },
            replayGain =
                GainMode.entries.find { it.name == storage.getString("replay-gain", null) }
                    ?: defaults.replayGain,
            quality =
                ResamplingQuality.entries.find { it.name == storage.getString("quality", null) }
                    ?: defaults.quality,
            phase =
                ResamplingPhase.entries.find { it.name == storage.getString("phase", null) }
                    ?: defaults.phase,
            profileName = storage.getString("equaliser-profile-name", "").orEmpty(),
            profileText = storage.getString("equaliser-profile-text", "").orEmpty(),
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
                .putFloat("equaliser-preamp", preferences.preampDb)
                .putString("replay-gain", preferences.replayGain.name)
                .putString("quality", preferences.quality.name)
                .putString("phase", preferences.phase.name)
                .putString("equaliser-profile-name", preferences.profileName)
                .putString("equaliser-profile-text", preferences.profileText)
                .putInt("buffer-ms", preferences.bufferMs.toInt())
                .putBoolean("dsd-like-pcm", preferences.dsdLikePcm)
                .putBoolean("true-peak", preferences.truePeak)
                .putString("restoration", preferences.restoration.name)
        preferences.gainsDb.forEachIndexed { index, gain ->
            edit.putFloat("equaliser-gain-$index", gain)
        }
        if (!edit.commit()) throw IOException("Sound settings could not be saved")
    }

    private fun finiteLevel(
        key: String,
        default: Float,
        range: ClosedFloatingPointRange<Float>,
    ): Float = storage.getFloat(key, default).takeIf { it.isFinite() }?.coerceIn(range) ?: default
}
