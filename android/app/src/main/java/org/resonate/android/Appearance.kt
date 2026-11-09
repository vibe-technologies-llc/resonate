package org.resonate.android

import android.content.SharedPreferences
import androidx.annotation.StringRes
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

enum class ThemeMode(@StringRes val label: Int, @StringRes val detail: Int) {
    SYSTEM(R.string.appearance_theme_system, R.string.appearance_theme_system_detail),
    LIGHT(R.string.appearance_theme_light, R.string.appearance_theme_light_detail),
    DARK(R.string.appearance_theme_dark, R.string.appearance_theme_dark_detail),
}

enum class Accent(@StringRes val label: Int, val hex: Int) {
    MAUVE(R.string.appearance_accent_mauve, 0xB98EF0),
    BLUE(R.string.appearance_accent_blue, 0x7CB6EA),
    TEAL(R.string.appearance_accent_teal, 0x5ECFC0),
    GREEN(R.string.appearance_accent_green, 0x5CCB92),
    AMBER(R.string.appearance_accent_amber, 0xE4A960),
    PEACH(R.string.appearance_accent_peach, 0xE8875F),
    RED(R.string.appearance_accent_red, 0xE36F6F),
}

data class Appearance(
    val mode: ThemeMode = ThemeMode.SYSTEM,
    val accent: Accent? = null,
    val pitchBlack: Boolean = false,
    val highContrast: Boolean = false,
    val backgroundStrength: Float = Appearance.STRENGTH_DEFAULT,
    val nowPlayingIntensity: Float = Appearance.INTENSITY_DEFAULT,
) {
    @get:StringRes
    val accentLabel: Int
        get() = accent?.label ?: WALLPAPER

    companion object {
        val WALLPAPER = R.string.appearance_accent_wallpaper
        const val STRENGTH_DEFAULT = 0.7f
        const val INTENSITY_DEFAULT = 0.5f
    }
}

class AppearanceStore(private val storage: SharedPreferences) {
    private val mutable = MutableStateFlow(read())
    val appearance: StateFlow<Appearance> = mutable

    fun chooseMode(mode: ThemeMode) {
        mutable.value = mutable.value.copy(mode = mode)
        storage.edit().putString(MODE, mode.name).apply()
    }

    fun chooseAccent(accent: Accent?) {
        mutable.value = mutable.value.copy(accent = accent)
        val edit = storage.edit()
        if (accent == null) edit.remove(ACCENT) else edit.putString(ACCENT, accent.name)
        edit.apply()
    }

    fun choosePitchBlack(pitchBlack: Boolean) {
        mutable.value = mutable.value.copy(pitchBlack = pitchBlack)
        storage.edit().putBoolean(PITCH_BLACK, pitchBlack).apply()
    }

    fun chooseHighContrast(highContrast: Boolean) {
        mutable.value = mutable.value.copy(highContrast = highContrast)
        storage.edit().putBoolean(HIGH_CONTRAST, highContrast).apply()
    }

    fun chooseBackgroundStrength(strength: Float) {
        val kept = strength.coerceIn(0f, 1f)
        mutable.value = mutable.value.copy(backgroundStrength = kept)
        storage.edit().putFloat(STRENGTH, kept).apply()
    }

    fun chooseNowPlayingIntensity(intensity: Float) {
        val kept = intensity.coerceIn(0f, 1f)
        mutable.value = mutable.value.copy(nowPlayingIntensity = kept)
        storage.edit().putFloat(INTENSITY, kept).apply()
    }

    private fun read() =
        Appearance(
            mode = ThemeMode.entries.find { it.name == storage.getString(MODE, null) }
                ?: ThemeMode.SYSTEM,
            accent = Accent.entries.find { it.name == storage.getString(ACCENT, null) },
            pitchBlack = storage.getBoolean(PITCH_BLACK, false),
            highContrast = storage.getBoolean(HIGH_CONTRAST, false),
            backgroundStrength =
                storage.getFloat(STRENGTH, Appearance.STRENGTH_DEFAULT).coerceIn(0f, 1f),
            nowPlayingIntensity =
                storage.getFloat(INTENSITY, Appearance.INTENSITY_DEFAULT).coerceIn(0f, 1f),
        )

    private companion object {
        const val MODE = "theme"
        const val ACCENT = "accent"
        const val PITCH_BLACK = "pitch-black"
        const val HIGH_CONTRAST = "high-contrast"
        const val STRENGTH = "background-strength"
        const val INTENSITY = "now-playing-intensity"
    }
}
