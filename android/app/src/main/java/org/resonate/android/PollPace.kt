package org.resonate.android

object PollPace {
    const val BUSY_MS = 250L
    const val IDLE_MS = 1_000L
    const val BACKGROUND_MS = 2_000L
    const val BUSY_AFTER_ASKING_MS = 2_000L

    fun intervalMs(foreground: Boolean, moving: Boolean, working: Boolean, asked: Boolean): Long =
        when {
            asked -> BUSY_MS
            !foreground -> BACKGROUND_MS
            moving || working -> BUSY_MS
            else -> IDLE_MS
        }
}

class OpenEngine<T : Any> {
    @Volatile private var open: T? = null

    fun opened(engine: T) {
        open = engine
    }

    fun closing(engine: T) {
        if (open === engine) open = null
    }

    fun run(engine: T, action: (T) -> Unit): Boolean {
        if (open !== engine) return false
        action(engine)
        return true
    }
}
