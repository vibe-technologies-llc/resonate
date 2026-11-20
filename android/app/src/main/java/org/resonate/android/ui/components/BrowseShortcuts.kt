package org.resonate.android.ui.components

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.TrendingUp
import androidx.compose.material.icons.outlined.BookmarkBorder
import androidx.compose.material.icons.rounded.Explore
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.HighQuality
import androidx.compose.material.icons.rounded.Schedule
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import org.resonate.android.R
import org.resonate.android.ui.Mix
import org.resonate.android.ui.navigation.Screen

private class BrowseTile(@StringRes val title: Int, val icon: ImageVector, val screen: Screen)

private val browseTiles =
    listOf(
        BrowseTile(R.string.browse_favourites, Icons.Rounded.Favorite, Screen.Favourites),
        BrowseTile(
            Mix.RECENTLY_ADDED.title,
            Icons.Rounded.Schedule,
            Screen.Mix(Mix.RECENTLY_ADDED),
        ),
        BrowseTile(
            Mix.MOST_PLAYED.title,
            Icons.AutoMirrored.Rounded.TrendingUp,
            Screen.Mix(Mix.MOST_PLAYED),
        ),
        BrowseTile(Mix.NEVER_HEARD.title, Icons.Rounded.Explore, Screen.Mix(Mix.NEVER_HEARD)),
        BrowseTile(
            Mix.HIGH_RESOLUTION.title,
            Icons.Rounded.HighQuality,
            Screen.Mix(Mix.HIGH_RESOLUTION),
        ),
        BrowseTile(R.string.settings_wanted_title, Icons.Outlined.BookmarkBorder, Screen.Wanted),
    )

private val tileSpacing = 8.dp
private val tileWidth = 146.dp
private val gridWidth = 560.dp

@Composable
fun BrowseShortcuts(open: (Screen) -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        SectionHeader(stringResource(R.string.search_browse))
        BoxWithConstraints(
            Modifier.padding(horizontal = Sizes.gutter).widthIn(max = gridWidth).fillMaxWidth()
        ) {
            val columns =
                if (rememberLargeText()) 1
                else ((maxWidth + tileSpacing) / (tileWidth + tileSpacing)).toInt().coerceIn(2, 3)
            Column(
                modifier = Modifier.padding(top = 4.dp, bottom = 8.dp),
                verticalArrangement = Arrangement.spacedBy(tileSpacing),
            ) {
                browseTiles.chunked(columns).forEach { tiles ->
                    Row(
                        Modifier.fillMaxWidth().height(IntrinsicSize.Min),
                        horizontalArrangement = Arrangement.spacedBy(tileSpacing),
                    ) {
                        tiles.forEach { tile ->
                            BrowseCard(tile, Modifier.weight(1f).fillMaxHeight()) {
                                open(tile.screen)
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun BrowseCard(tile: BrowseTile, modifier: Modifier, onClick: () -> Unit) {
    Card(
        onClick = onClick,
        modifier = modifier.testTag("browse-tile"),
        shape = MaterialTheme.shapes.medium,
        colors =
            CardDefaults.cardColors(
                containerColor = MaterialTheme.colorScheme.surfaceContainerHigh
            ),
    ) {
        Row(
            Modifier.fillMaxWidth()
                .heightIn(min = 48.dp)
                .padding(horizontal = 12.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(tileSpacing),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                tile.icon,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.primary,
                modifier = Modifier.size(20.dp),
            )
            Text(stringResource(tile.title), style = MaterialTheme.typography.titleSmall)
        }
    }
}
