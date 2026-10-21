package org.resonate.android.ui.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.calculateEndPadding
import androidx.compose.foundation.layout.calculateStartPadding
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.TrendingUp
import androidx.compose.material.icons.rounded.Explore
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.HighQuality
import androidx.compose.material.icons.rounded.NewReleases
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import org.resonate.android.ui.Mix
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.navigation.LocalNavigator
import org.resonate.android.ui.navigation.Screen

private val CARD_LEAST = 168.dp

private class Suggestion(
    val key: String,
    val title: String,
    val subtitle: String,
    val icon: ImageVector,
    val screen: Screen,
)

private val Mix.icon: ImageVector
    get() =
        when (this) {
            Mix.NEVER_HEARD -> Icons.Rounded.Explore
            Mix.MOST_PLAYED -> Icons.AutoMirrored.Rounded.TrendingUp
            Mix.RECENTLY_ADDED -> Icons.Rounded.NewReleases
            Mix.HIGH_RESOLUTION -> Icons.Rounded.HighQuality
        }

private val suggestions =
    Mix.entries.map { Suggestion(it.name, it.title, it.subtitle, it.icon, Screen.Mix(it)) } +
        Suggestion(
            "favourites",
            "Your favourites",
            "The ones you come back to.",
            Icons.Rounded.Favorite,
            Screen.Favourites,
        )

@Composable
fun SuggestionsScreen() {
    val navigator = LocalNavigator.current
    val scheme = MaterialTheme.colorScheme
    val palettes =
        listOf(
            scheme.primaryContainer to scheme.onPrimaryContainer,
            scheme.secondaryContainer to scheme.onSecondaryContainer,
            scheme.surfaceContainerHighest to scheme.onSurface,
        )
    Page("Made for you") { padding ->
        val direction = LocalLayoutDirection.current
        val fontScale = LocalDensity.current.fontScale
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = CARD_LEAST * fontScale.coerceIn(1f, 1.6f)),
                modifier =
                    Modifier.widthIn(max = PageWidth)
                        .fillMaxWidth()
                        .fillMaxHeight()
                        .testTag("suggestions"),
                contentPadding =
                    PaddingValues(
                        start = padding.calculateStartPadding(direction) + Sizes.gutter,
                        end = padding.calculateEndPadding(direction) + Sizes.gutter,
                        top = padding.calculateTopPadding() + 8.dp,
                        bottom = padding.calculateBottomPadding(),
                    ),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                item(span = { GridItemSpan(maxLineSpan) }) {
                    Text(
                        "Mixes drawn from your music and your listening.",
                        style = MaterialTheme.typography.bodyMedium,
                        color = scheme.onSurfaceVariant,
                        modifier = Modifier.padding(bottom = 4.dp),
                    )
                }
                items(
                    suggestions.size,
                    key = { suggestions[it].key },
                    span = {
                        if (it == suggestions.lastIndex && suggestions.size % maxLineSpan != 0)
                            GridItemSpan(maxLineSpan)
                        else GridItemSpan(1)
                    },
                ) { index ->
                    val suggestion = suggestions[index]
                    val (container, content) = palettes[index % palettes.size]
                    SuggestionCard(suggestion, container, content) {
                        navigator.open(suggestion.screen)
                    }
                }
            }
        }
    }
}

@Composable
private fun SuggestionCard(
    suggestion: Suggestion,
    container: Color,
    content: Color,
    onClick: () -> Unit,
) {
    Card(
        onClick = onClick,
        shape = MaterialTheme.shapes.extraLarge,
        colors = CardDefaults.cardColors(containerColor = container, contentColor = content),
        modifier =
            Modifier.fillMaxWidth()
                .heightIn(min = 176.dp)
                .testTag("mix-${suggestion.key.lowercase()}"),
    ) {
        Column(Modifier.padding(16.dp)) {
            Surface(
                color = content,
                contentColor = container,
                shape = MaterialTheme.shapes.large,
                modifier = Modifier.size(48.dp),
            ) {
                Box(contentAlignment = Alignment.Center) {
                    Icon(suggestion.icon, contentDescription = null)
                }
            }
            Spacer(Modifier.height(20.dp))
            Text(suggestion.title, style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(4.dp))
            Text(suggestion.subtitle, style = MaterialTheme.typography.bodyMedium)
        }
    }
}
