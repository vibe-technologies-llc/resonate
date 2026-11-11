package org.resonate.android

import java.util.Collections
import java.util.IdentityHashMap
import kotlinx.coroutines.CancellationException

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

class EngineClosed : CancellationException()

class OpenEngine<T : Any>(private val release: (T) -> Unit = {}) {
    private var open: T? = null
    private val holders = IdentityHashMap<T, Int>()
    private val owed = Collections.newSetFromMap(IdentityHashMap<T, Boolean>())

    @Synchronized
    fun opened(engine: T) {
        open = engine
    }

    @Synchronized
    fun closing(engine: T) {
        if (open === engine) open = null
    }

    fun closed(engine: T) {
        val now =
            synchronized(this) {
                closing(engine)
                val held = (holders[engine] ?: 0) > 0
                if (held) owed.add(engine)
                !held
            }
        if (now) release(engine)
    }

    fun run(engine: T, action: (T) -> Unit): Boolean {
        if (!take(engine)) return false
        try {
            action(engine)
        } finally {
            give(engine)
        }
        return true
    }

    inline fun <R> holding(engine: T, action: (T) -> R): R {
        if (!take(engine)) throw EngineClosed()
        try {
            return action(engine)
        } finally {
            give(engine)
        }
    }

    @PublishedApi
    @Synchronized
    internal fun take(engine: T): Boolean {
        if (open !== engine) return false
        holders[engine] = (holders[engine] ?: 0) + 1
        return true
    }

    @PublishedApi
    internal fun give(engine: T) {
        val due =
            synchronized(this) {
                val left = (holders[engine] ?: 1) - 1
                if (left > 0) {
                    holders[engine] = left
                    false
                } else {
                    holders.remove(engine)
                    owed.remove(engine)
                }
            }
        if (due) release(engine)
    }
}
