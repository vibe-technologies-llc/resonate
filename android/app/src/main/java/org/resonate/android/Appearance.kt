package org.resonate.android

import android.content.SharedPreferences
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow

enum class ThemeMode(val label: String, val detail: String) {
    SYSTEM("System", "Follow the phone's light and dark setting"),
    LIGHT("Light", "Always light"),
    DARK("Dark", "Always dark"),
}

enum class Accent(val label: String, val hex: Int) {
    MAUVE("Mauve", 0xB98EF0),
    BLUE("Blue", 0x7CB6EA),
    TEAL("Teal", 0x5ECFC0),
    GREEN("Green", 0x5CCB92),
    AMBER("Amber", 0xE4A960),
    PEACH("Peach", 0xE8875F),
    RED("Red", 0xE36F6F),
}

data class Appearance(val mode: ThemeMode = ThemeMode.SYSTEM, val accent: Accent? = null) {
    val accentLabel: String
        get() = accent?.label ?: WALLPAPER

    companion object {
        const val WALLPAPER = "Wallpaper"
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

    private fun read() =
        Appearance(
            mode = ThemeMode.entries.find { it.name == storage.getString(MODE, null) }
                ?: ThemeMode.SYSTEM,
            accent = Accent.entries.find { it.name == storage.getString(ACCENT, null) },
        )

    private companion object {
        const val MODE = "theme"
        const val ACCENT = "accent"
    }
}
