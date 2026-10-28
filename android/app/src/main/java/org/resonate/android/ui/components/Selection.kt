package org.resonate.android.ui.components

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue

@Stable
class Selection<K : Any> {
    var chosen by mutableStateOf<Set<K>>(emptySet())
        private set

    val active: Boolean
        get() = chosen.isNotEmpty()

    val count: Int
        get() = chosen.size

    operator fun contains(key: K): Boolean = key in chosen

    fun begin(key: K) {
        chosen = chosen + key
    }

    fun toggle(key: K) {
        chosen = if (key in chosen) chosen - key else chosen + key
    }

    fun selectAll(keys: Collection<K>) {
        chosen = keys.toSet()
    }

    fun clear() {
        if (chosen.isNotEmpty()) chosen = emptySet()
    }

    fun retain(keys: Collection<K>) {
        val kept = chosen.intersect(keys.toSet())
        if (kept.size != chosen.size) chosen = kept
    }

    fun everyOf(keys: Collection<K>): Boolean = keys.isNotEmpty() && chosen.containsAll(keys)

    fun <T> picked(items: List<T>, keyOf: (T) -> K): List<T> = items.filter { keyOf(it) in chosen }

    fun row(key: K): RowSelection =
        RowSelection(
            selecting = active,
            selected = key in chosen,
            toggle = { toggle(key) },
            begin = { begin(key) },
        )
}

@Stable
class RowSelection(
    val selecting: Boolean,
    val selected: Boolean,
    val toggle: () -> Unit,
    val begin: () -> Unit,
)

@Composable fun <K : Any> rememberSelection(): Selection<K> = remember { Selection() }
