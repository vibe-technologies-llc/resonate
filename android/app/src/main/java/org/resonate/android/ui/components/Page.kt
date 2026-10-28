package org.resonate.android.ui.components

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LargeFlexibleTopAppBar
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MediumFlexibleTopAppBar
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.TopAppBarScrollBehavior
import androidx.compose.runtime.Composable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.findRootCoordinates
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlin.math.roundToInt
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen

enum class TitleSize {
    LARGE,
    MEDIUM,
    SMALL,
}

val LocalPageBottomPadding = compositionLocalOf { 0.dp }

@Composable
fun Page(
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    size: TitleSize = TitleSize.MEDIUM,
    back: Boolean = true,
    actions: @Composable RowScope.() -> Unit = {},
    floatingActionButton: @Composable () -> Unit = {},
    selectionBar: (@Composable () -> Unit)? = null,
    content: @Composable (PaddingValues) -> Unit,
) {
    val navigator = LocalNavigator.current
    val scrollBehavior =
        when (size) {
            TitleSize.SMALL -> TopAppBarDefaults.pinnedScrollBehavior()
            else -> TopAppBarDefaults.exitUntilCollapsedScrollBehavior()
        }
    val navigation: @Composable () -> Unit = {
        if (back) {
            IconButton(onClick = navigator::back) {
                Icon(Icons.AutoMirrored.Rounded.ArrowBack, contentDescription = "Back")
            }
        }
    }
    val density = LocalDensity.current
    var belowPage by remember { mutableIntStateOf(0) }
    val keyboard = WindowInsets.ime.getBottom(density)
    val covered = with(density) { (keyboard - belowPage).coerceAtLeast(0).toDp() }
    Scaffold(
        modifier =
            modifier
                .onGloballyPositioned { page ->
                    val window = page.findRootCoordinates().size.height
                    belowPage = (window - page.boundsInRoot().bottom).roundToInt().coerceAtLeast(0)
                }
                .padding(bottom = covered)
                .nestedScroll(scrollBehavior.nestedScrollConnection),
        containerColor = MaterialTheme.colorScheme.surface,
        contentWindowInsets = WindowInsets.safeDrawing.only(WindowInsetsSides.Horizontal),
        floatingActionButton = floatingActionButton,
        bottomBar = {
            Spacer(Modifier.height((LocalPageBottomPadding.current - covered).coerceAtLeast(0.dp)))
        },
        topBar = {
            if (selectionBar != null) selectionBar()
            else PageBar(title, subtitle, size, scrollBehavior, navigation, actions)
        },
    ) { padding ->
        val direction = LocalLayoutDirection.current
        content(
            PaddingValues(
                start = padding.calculateStartPadding(direction),
                top = padding.calculateTopPadding(),
                end = padding.calculateEndPadding(direction),
                bottom = padding.calculateBottomPadding() + 16.dp,
            )
        )
    }
}

@Composable
private fun PageBar(
    title: String,
    subtitle: String?,
    size: TitleSize,
    scrollBehavior: TopAppBarScrollBehavior,
    navigation: @Composable () -> Unit,
    actions: @Composable RowScope.() -> Unit,
) {
    val titleText: @Composable () -> Unit = {
        Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
    val subtitleText: (@Composable () -> Unit)? =
        subtitle?.let { { Text(it, maxLines = 1, overflow = TextOverflow.Ellipsis) } }
    val surface = MaterialTheme.colorScheme.surface
    val colors =
        TopAppBarDefaults.topAppBarColors(containerColor = surface, scrolledContainerColor = surface)
    when (size) {
        TitleSize.LARGE ->
            LargeFlexibleTopAppBar(
                title = titleText,
                subtitle = subtitleText,
                navigationIcon = navigation,
                actions = actions,
                scrollBehavior = scrollBehavior,
                colors = colors,
            )
        TitleSize.MEDIUM ->
            MediumFlexibleTopAppBar(
                title = titleText,
                subtitle = subtitleText,
                navigationIcon = navigation,
                actions = actions,
                scrollBehavior = scrollBehavior,
                colors = colors,
            )
        TitleSize.SMALL ->
            TopAppBar(
                title = titleText,
                subtitle = subtitleText ?: {},
                navigationIcon = navigation,
                actions = actions,
                scrollBehavior = scrollBehavior,
                colors = colors,
            )
    }
}

@Composable
fun SettingsAction() {
    val navigator = LocalNavigator.current
    IconButton(onClick = { navigator.open(Screen.Settings) }) {
        Icon(Icons.Rounded.Settings, contentDescription = "Settings")
    }
}
