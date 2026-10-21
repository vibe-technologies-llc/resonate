package org.resonate.android.ui.library

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.TitleSize

@Composable
fun <T> CollectionPage(
    title: String,
    loaded: Loaded<T>,
    onRetry: () -> Unit,
    tag: String,
    list: LazyListState = rememberLazyListState(),
    actions: @Composable RowScope.() -> Unit = {},
    floatingActionButton: @Composable () -> Unit = {},
    content: LazyListScope.(T) -> Unit,
) {
    val past = rememberPastHeader(list)
    Page(
        if (past || loaded !is Loaded.Ready) title else "",
        size = TitleSize.SMALL,
        actions = actions,
        floatingActionButton = floatingActionButton,
    ) { padding ->
        LoadedContent(loaded, onRetry, Modifier.padding(padding)) { value ->
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
                LazyColumn(
                    state = list,
                    contentPadding = padding,
                    modifier =
                        Modifier.widthIn(max = Shelves.pageWidest)
                            .fillMaxWidth()
                            .fillMaxHeight()
                            .testTag(tag),
                ) {
                    content(value)
                }
            }
        }
    }
}
