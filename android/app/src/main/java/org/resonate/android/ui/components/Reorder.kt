package org.resonate.android.ui.components

import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.DragHandle
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.zIndex
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.flow.collectLatest

@Stable
class Reorder<T>(
    private val list: LazyListState,
    private val keyOf: (T) -> Any,
    private val firstIndex: () -> Int,
    private val anchor: () -> Int,
    private val onMove: (from: Int, to: Int) -> Unit,
) {
    val shown = mutableStateListOf<T>()
    var dragged by mutableStateOf<Any?>(null)
        private set

    var offset by mutableFloatStateOf(0f)
        private set

    private var startedAt = -1
    private var anchoredAt = 0
    private var latest = emptyList<T>()
    private var followedAtStart = emptyList<T>()

    fun follow(items: List<T>) {
        latest = items
        if (dragged != null) return
        shown.clear()
        shown.addAll(items)
    }

    fun start(key: Any) {
        startedAt = shown.indexOfFirst { keyOf(it) == key }
        if (startedAt < 0) return
        anchoredAt = anchor()
        followedAtStart = latest
        dragged = key
        offset = 0f
    }

    fun drag(delta: Float) {
        val key = dragged ?: return
        offset += delta
        val visible = list.layoutInfo.visibleItemsInfo
        val current = visible.firstOrNull { it.key == key } ?: return
        val centre = current.offset + offset + current.size / 2f
        val base = firstIndex()
        val target =
            visible.firstOrNull {
                it.key != key &&
                    it.index - base in shown.indices &&
                    centre > it.offset &&
                    centre < it.offset + it.size
            } ?: return
        val from = current.index - base
        val to = target.index - base
        if (from !in shown.indices || to !in shown.indices) return
        shown.add(to, shown.removeAt(from))
        offset += (current.offset - target.offset).toFloat()
    }

    fun edgeSpeed(edge: Float): Float {
        val key = dragged ?: return 0f
        val info = list.layoutInfo
        val current = info.visibleItemsInfo.firstOrNull { it.key == key } ?: return 0f
        val top = current.offset + offset
        val bottom = top + current.size
        val start = edge
        val end = info.viewportEndOffset - info.afterContentPadding - edge
        val past =
            when {
                bottom > end -> bottom - end
                top < start -> top - start
                else -> 0f
            }
        return (past / edge).coerceIn(-1f, 1f) * edge * EDGE_SPEED_PER_FRAME
    }

    fun scrolled(by: Float) {
        if (dragged == null || by == 0f) return
        offset += by
        drag(0f)
    }

    fun end() {
        val key = dragged ?: return
        val finalAt = shown.indexOfFirst { keyOf(it) == key }
        val moved = startedAt >= 0 && finalAt >= 0 && finalAt != startedAt
        dragged = null
        offset = 0f
        if (moved) onMove(anchoredAt + startedAt, anchoredAt + finalAt)
        if (!moved || latest !== followedAtStart) follow(latest)
        startedAt = -1
    }
}

@Composable
fun <T> rememberReorder(
    items: List<T>,
    list: LazyListState,
    keyOf: (T) -> Any,
    firstIndex: () -> Int,
    anchor: () -> Int = { 0 },
    onMove: (from: Int, to: Int) -> Unit,
): Reorder<T> {
    val move by rememberUpdatedState(onMove)
    val first by rememberUpdatedState(firstIndex)
    val anchored by rememberUpdatedState(anchor)
    val reorder =
        remember(list) {
            Reorder(list, keyOf, { first() }, { anchored() }) { from, to -> move(from, to) }
                .also { it.follow(items) }
        }
    LaunchedEffect(items) { reorder.follow(items) }
    val edge = with(LocalDensity.current) { SCROLLING_EDGE.toPx() }
    LaunchedEffect(reorder, edge) {
        snapshotFlow { reorder.dragged != null }
            .collectLatest { dragging ->
                while (dragging) {
                    withFrameNanos {}
                    val speed = reorder.edgeSpeed(edge)
                    if (speed != 0f) reorder.scrolled(list.scrollBy(speed))
                }
            }
    }
    return reorder
}

private val SCROLLING_EDGE = 56.dp
private const val EDGE_SPEED_PER_FRAME = 0.3f

@Composable
fun LazyItemScope.reorderable(reorder: Reorder<*>, key: Any): Modifier {
    val dragging = reorder.dragged == key
    return if (dragging)
        Modifier.zIndex(1f).graphicsLayer {
            translationY = reorder.offset
            shadowElevation = 8.dp.toPx()
        }
    else Modifier.animateItem()
}

@Composable
fun DragHandle(reorder: Reorder<*>, key: Any) {
    Icon(
        Icons.Rounded.DragHandle,
        contentDescription = "Reorder",
        tint = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier =
            Modifier.padding(12.dp)
                .draggable(
                    orientation = Orientation.Vertical,
                    state = rememberDraggableState { reorder.drag(it) },
                    onDragStarted = { reorder.start(key) },
                    onDragStopped = { reorder.end() },
                ),
    )
}
