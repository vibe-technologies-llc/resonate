package org.resonate.android

enum class WidgetPress {
    CONTROLLED,
    STARTED,
}

enum class ColdSkip {
    PREVIOUS,
    NEXT,
}

object WidgetPresses {
    const val TOGGLE = "org.resonate.android.widget.TOGGLE"
    const val PREVIOUS = "org.resonate.android.widget.PREVIOUS"
    const val NEXT = "org.resonate.android.widget.NEXT"
    val actions = setOf(TOGGLE, PREVIOUS, NEXT)

    fun startsTheService(action: String, playing: Boolean, serviceRunning: Boolean): Boolean =
        if (action == TOGGLE) !playing else !serviceRunning

    fun pressed(serviceRunning: Boolean): WidgetPress =
        if (serviceRunning) WidgetPress.CONTROLLED else WidgetPress.STARTED

    fun coldSkip(action: String?): ColdSkip? =
        when (action) {
            PREVIOUS -> ColdSkip.PREVIOUS
            NEXT -> ColdSkip.NEXT
            else -> null
        }

    fun resumesBeforeSkipping(holdsARow: Boolean, resumedAlready: Boolean): Boolean =
        !holdsARow && !resumedAlready
}
