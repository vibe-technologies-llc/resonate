package org.resonate.android.ui.components

import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.only
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
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen

enum class TitleSize {
    LARGE,
    MEDIUM,
    SMALL,
}

@Composable
fun Page(
    title: String,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    size: TitleSize = TitleSize.MEDIUM,
    back: Boolean = true,
    actions: @Composable RowScope.() -> Unit = {},
    floatingActionButton: @Composable () -> Unit = {},
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
    Scaffold(
        modifier = modifier.nestedScroll(scrollBehavior.nestedScrollConnection),
        containerColor = MaterialTheme.colorScheme.surface,
        contentWindowInsets = WindowInsets.safeDrawing.only(WindowInsetsSides.Horizontal),
        floatingActionButton = floatingActionButton,
        topBar = {
            PageBar(title, subtitle, size, scrollBehavior, navigation, actions)
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
    when (size) {
        TitleSize.LARGE ->
            LargeFlexibleTopAppBar(
                title = titleText,
                subtitle = subtitleText,
                navigationIcon = navigation,
                actions = actions,
                scrollBehavior = scrollBehavior,
            )
        TitleSize.MEDIUM ->
            MediumFlexibleTopAppBar(
                title = titleText,
                subtitle = subtitleText,
                navigationIcon = navigation,
                actions = actions,
                scrollBehavior = scrollBehavior,
            )
        TitleSize.SMALL ->
            TopAppBar(
                title = titleText,
                subtitle = subtitleText ?: {},
                navigationIcon = navigation,
                actions = actions,
                scrollBehavior = scrollBehavior,
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
