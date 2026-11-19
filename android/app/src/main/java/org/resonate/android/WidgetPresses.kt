package org.resonate.android

enum class WidgetPress {
    CONTROLLED,
    STARTED,
    UNHEARD,
}

object WidgetPresses {
    const val TOGGLE = "org.resonate.android.widget.TOGGLE"
    const val PREVIOUS = "org.resonate.android.widget.PREVIOUS"
    const val NEXT = "org.resonate.android.widget.NEXT"
    val actions = setOf(TOGGLE, PREVIOUS, NEXT)

    fun playStartsTheService(playing: Boolean): Boolean = !playing

    fun pressed(action: String, serviceRunning: Boolean): WidgetPress =
        when {
            serviceRunning -> WidgetPress.CONTROLLED
            action == TOGGLE -> WidgetPress.STARTED
            else -> WidgetPress.UNHEARD
        }
}
