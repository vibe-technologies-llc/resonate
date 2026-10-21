package org.resonate.android.ui.library

import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.hideFromAccessibility
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import kotlinx.coroutines.launch

fun scrolledShare(first: Int, total: Int, visible: Int): Float {
    val scrollable = total - visible
    return if (scrollable <= 0) 0f else (first.toFloat() / scrollable).coerceIn(0f, 1f)
}

fun itemAtShare(share: Float, total: Int): Int =
    if (total <= 0) 0 else (share.coerceIn(0f, 1f) * (total - 1)).roundToInt()

@Composable
fun FastScroller(list: LazyListState, rows: Int, modifier: Modifier = Modifier) {
    if (rows < FAST_SCROLL_FROM) return
    val scope = rememberCoroutineScope()
    var dragging by remember { mutableStateOf(false) }
    var dragShare by remember { mutableFloatStateOf(0f) }
    val shown = list.isScrollInProgress || dragging
    val alpha by
        animateFloatAsState(
            if (shown) 1f else 0f,
            tween(durationMillis = if (shown) 150 else 500, delayMillis = if (shown) 0 else 900),
            label = "scroller",
        )
    val width by animateDpAsState(if (dragging) 8.dp else 5.dp, label = "thumb")
    val share by remember(list) {
        derivedStateOf {
            val info = list.layoutInfo
            scrolledShare(list.firstVisibleItemIndex, info.totalItemsCount, info.visibleItemsInfo.size)
        }
    }
    val color = MaterialTheme.colorScheme.primary
    BoxWithConstraints(modifier.fillMaxHeight().width(28.dp).semantics { hideFromAccessibility() }) {
        val density = LocalDensity.current
        val thumb = with(density) { THUMB.toPx() }
        val travel = (constraints.maxHeight - thumb).coerceAtLeast(1f)
        val at = if (dragging) dragShare else share
        Box(
            Modifier.offset { IntOffset(0, (travel * at).roundToInt()) }
                .align(Alignment.TopEnd)
                .size(28.dp, THUMB)
                .graphicsLayer { this.alpha = alpha }
                .draggable(
                    orientation = Orientation.Vertical,
                    enabled = alpha > 0f,
                    state =
                        rememberDraggableState { delta ->
                            dragShare = (dragShare + delta / travel).coerceIn(0f, 1f)
                            val total = list.layoutInfo.totalItemsCount
                            scope.launch { list.scrollToItem(itemAtShare(dragShare, total)) }
                        },
                    onDragStarted = {
                        dragShare = share
                        dragging = true
                    },
                    onDragStopped = { dragging = false },
                )
        ) {
            Box(
                Modifier.align(Alignment.CenterEnd)
                    .offset(x = (-4).dp)
                    .width(width)
                    .height(THUMB)
                    .background(color, CircleShape)
            )
        }
    }
}

private val THUMB = 52.dp
private const val FAST_SCROLL_FROM = 60
