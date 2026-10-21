package org.resonate.android.ui.navigation

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.lifecycle.viewmodel.navigation3.rememberViewModelStoreNavEntryDecorator
import androidx.navigation3.runtime.NavBackStack
import androidx.navigation3.runtime.NavEntry
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.rememberDecoratedNavEntries
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.runtime.rememberSaveableStateHolderNavEntryDecorator

@Stable
class Navigator(private val stacks: List<NavBackStack<NavKey>>, selected: Int) {
    var selected by mutableIntStateOf(selected)
        private set

    var beforeOpen: () -> Unit = {}

    val root: Screen
        get() = rootScreens[selected]

    private val current: NavBackStack<NavKey>
        get() = stacks[selected]

    val top: Screen
        get() = current.last() as Screen

    val atHome: Boolean
        get() = selected == 0 && current.size == 1

    fun select(screen: Screen) {
        val index = rootScreens.indexOf(screen).takeIf { it >= 0 } ?: return
        if (index == selected) {
            while (current.size > 1) current.removeAt(current.lastIndex)
        } else {
            selected = index
        }
    }

    fun open(screen: Screen) {
        beforeOpen()
        if (screen in rootScreens) {
            select(screen)
        } else if (current.lastOrNull() != screen) {
            current.add(screen)
        }
    }

    fun back() {
        when {
            current.size > 1 -> current.removeAt(current.lastIndex)
            selected != 0 -> selected = 0
        }
    }

    @Composable
    fun entries(content: @Composable (Screen) -> Unit): List<NavEntry<NavKey>> {
        val decorated =
            stacks.mapIndexed { index, stack ->
                rememberDecoratedNavEntries(
                    backStack = stack,
                    entryDecorators =
                        listOf(
                            rememberSaveableStateHolderNavEntryDecorator(),
                            rememberViewModelStoreNavEntryDecorator(),
                        ),
                    entryProvider = { key ->
                        NavEntry(key, contentKey = "$index/$key") { content(key as Screen) }
                    },
                )
            }
        return if (selected == 0) decorated[0] else decorated[0] + decorated[selected]
    }
}

@Composable
fun rememberNavigator(): Navigator {
    val stacks = rootScreens.map { rememberNavBackStack(it) }
    return rememberSaveable(saver = navigatorSaver(stacks)) { Navigator(stacks, 0) }
}

private fun navigatorSaver(stacks: List<NavBackStack<NavKey>>) =
    Saver<Navigator, Int>(
        save = { it.selected },
        restore = { Navigator(stacks, it) },
    )

val LocalNavigator = staticCompositionLocalOf<Navigator> { error("No navigator in composition") }
