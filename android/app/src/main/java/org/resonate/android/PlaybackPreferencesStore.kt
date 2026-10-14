package org.resonate.android

import android.content.SharedPreferences
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
import org.resonate.android.nativecore.tenBandCurve

class PlaybackPreferencesStore(private val storage: SharedPreferences) {
    fun read(): PlaybackPreferences {
        val defaults = defaultPlaybackPreferences()
        return defaults.copy(
            equaliserEnabled = storage.getBoolean("equaliser-enabled", defaults.equaliserEnabled),
            equaliser = storedCurve() ?: tenBandMigration() ?: defaults.equaliser,
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
        (TEN_BAND_KEYS + TEN_BAND_GAINS).forEach(edit::remove)
        if (!edit.commit()) throw IOException("Sound settings could not be saved")
    }

    private fun storedCurve(): EqualiserCurve? {
        val kept = storage.getString(CURVE, null) ?: return null
        val name = storage.getString(CURVE_NAME, "").orEmpty()
        return runCatching { parseEqProfile(kept.toByteArray(Charsets.UTF_8), name).curve }
            .getOrNull()
    }

    private fun tenBandMigration(): EqualiserCurve? {
        val profile = storage.getString(TEN_BAND_PROFILE_TEXT, "").orEmpty()
        if (profile.isNotEmpty()) {
            val name = storage.getString(TEN_BAND_PROFILE_NAME, "").orEmpty()
            val migrated =
                runCatching { parseEqProfile(profile.toByteArray(Charsets.UTF_8), name).curve }
                    .getOrNull()
            if (migrated != null) return migrated
        }
        if ((listOf(TEN_BAND_PREAMP) + TEN_BAND_GAINS).none(storage::contains)) return null
        return tenBandCurve(
            storage.getFloat(TEN_BAND_PREAMP, 0f),
            TEN_BAND_GAINS.map { storage.getFloat(it, 0f) },
        )
    }

    private companion object {
        const val CURVE = "equaliser-curve"
        const val CURVE_NAME = "equaliser-curve-name"
        const val TEN_BAND_PREAMP = "equaliser-preamp"
        const val TEN_BAND_PROFILE_NAME = "equaliser-profile-name"
        const val TEN_BAND_PROFILE_TEXT = "equaliser-profile-text"
        val TEN_BAND_KEYS = listOf(TEN_BAND_PREAMP, TEN_BAND_PROFILE_NAME, TEN_BAND_PROFILE_TEXT)
        const val TEN_BANDS = 10
        val TEN_BAND_GAINS = (0 until TEN_BANDS).map { "equaliser-gain-$it" }
    }
}
