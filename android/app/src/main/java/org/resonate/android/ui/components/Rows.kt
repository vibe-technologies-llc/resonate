package org.resonate.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.CheckCircle
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.GraphicEq
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.byline
import org.resonate.android.ui.shownTitle

object Sizes {
    val rowArt = 56.dp
    val carouselCover = 156.dp
    val artistCircle = 120.dp
    val headerCover = 240.dp
    val gutter = 16.dp
}

@Composable
fun TrackRow(
    track: Track,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    playing: Boolean = false,
    number: String? = null,
    supporting: String = track.byline,
    title: AnnotatedString? = null,
    subtitle: AnnotatedString? = null,
    onMore: (() -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
    selection: RowSelection? = null,
) {
    val scheme = MaterialTheme.colorScheme
    val haptics = LocalHapticFeedback.current
    val selecting = selection?.selecting == true
    val selected = selection?.selected == true
    val label = title?.text ?: track.shownTitle
    ListItem(
        onClick = if (selection != null && selecting) selection.toggle else onClick,
        onLongClick =
            selection?.let { held ->
                {
                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                    if (held.selecting) held.toggle() else held.begin()
                }
            },
        onLongClickLabel = selection?.let { "Select" },
        modifier =
            modifier
                .then(if (selected) Modifier.background(scheme.secondaryContainer) else Modifier)
                .then(
                    if (selection == null) Modifier
                    else
                        Modifier.semantics {
                            this.selected = selected
                            customActions =
                                if (selecting) emptyList()
                                else
                                    listOf(
                                        CustomAccessibilityAction("Select $label") {
                                            selection.begin()
                                            true
                                        }
                                    )
                        }
                ),
        colors =
            ListItemDefaults.colors(
                containerColor = Color.Transparent,
                contentColor = if (playing) scheme.primary else scheme.onSurface,
            ),
        leadingContent =
            if (number != null) {
                {
                    Box(Modifier.width(32.dp), contentAlignment = Alignment.Center) {
                        if (selected) {
                            Icon(Icons.Rounded.CheckCircle, null, tint = scheme.primary)
                        } else if (playing) {
                            Icon(Icons.Rounded.GraphicEq, null, tint = scheme.primary)
                        } else {
                            Text(
                                number,
                                style = MaterialTheme.typography.bodyMedium,
                                color = scheme.onSurfaceVariant,
                                textAlign = TextAlign.Center,
                            )
                        }
                    }
                }
            } else {
                {
                    Box {
                        Artwork(track, Sizes.rowArt, shape = MaterialTheme.shapes.medium)
                        if (selected) {
                            Surface(
                                color = scheme.primary,
                                shape = CircleShape,
                                modifier = Modifier.align(Alignment.Center).size(28.dp),
                            ) {
                                Icon(
                                    Icons.Rounded.Check,
                                    null,
                                    tint = scheme.onPrimary,
                                    modifier = Modifier.padding(4.dp),
                                )
                            }
                        } else if (playing) {
                            Surface(
                                color = scheme.primary,
                                shape = CircleShape,
                                modifier = Modifier.align(Alignment.BottomEnd).size(20.dp),
                            ) {
                                Icon(
                                    Icons.Rounded.GraphicEq,
                                    null,
                                    tint = scheme.onPrimary,
                                    modifier = Modifier.padding(3.dp),
                                )
                            }
                        }
                    }
                }
            },
        supportingContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (track.favourite) {
                    Icon(
                        Icons.Rounded.Favorite,
                        contentDescription = "Favourite",
                        tint = scheme.primary,
                        modifier = Modifier.size(14.dp),
                    )
                    Spacer(Modifier.width(4.dp))
                }
                Text(
                    subtitle ?: AnnotatedString(supporting),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        },
        trailingContent =
            if (selecting) null
            else
                trailing
                    ?: onMore?.let { more ->
                        {
                            IconButton(onClick = more) {
                                Icon(Icons.Rounded.MoreVert, contentDescription = "More options")
                            }
                        }
                    },
    ) {
        Text(
            title ?: AnnotatedString(track.shownTitle),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            fontWeight = if (playing) FontWeight.SemiBold else null,
        )
    }
}

@Composable
fun CollectionRow(
    title: String,
    supporting: String?,
    onClick: () -> Unit,
    leading: @Composable () -> Unit,
    modifier: Modifier = Modifier,
    titleText: AnnotatedString? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    ListItem(
        onClick = onClick,
        modifier = modifier,
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        leadingContent = leading,
        supportingContent =
            supporting?.let { { Text(it, maxLines = 1, overflow = TextOverflow.Ellipsis) } },
        trailingContent = trailing,
    ) {
        Text(titleText ?: AnnotatedString(title), maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
fun IconTile(
    icon: ImageVector,
    modifier: Modifier = Modifier,
    container: Color = MaterialTheme.colorScheme.secondaryContainer,
    content: Color = MaterialTheme.colorScheme.onSecondaryContainer,
) {
    Surface(color = container, shape = MaterialTheme.shapes.medium, modifier = modifier.size(Sizes.rowArt)) {
        Box(contentAlignment = Alignment.Center) { Icon(icon, null, tint = content) }
    }
}

@Composable
fun ArtistPortrait(song: ULong?, size: androidx.compose.ui.unit.Dp, modifier: Modifier = Modifier) {
    SongArtwork(song, size, modifier, shape = CircleShape, placeholder = Icons.Rounded.Person)
}

@Composable
fun SectionHeader(
    title: String,
    modifier: Modifier = Modifier,
    action: String? = null,
    onAction: (() -> Unit)? = null,
) {
    Row(
        modifier
            .fillMaxWidth()
            .padding(start = Sizes.gutter, end = 4.dp, top = 20.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            title,
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.weight(1f).semantics { heading() },
        )
        if (action != null && onAction != null) {
            TextButton(onClick = onAction) { Text(action) }
        }
    }
}

@Composable
fun EmptyState(
    icon: ImageVector,
    title: String,
    modifier: Modifier = Modifier,
    detail: String? = null,
    action: String? = null,
    onAction: (() -> Unit)? = null,
) {
    Column(
        modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Surface(
            color = MaterialTheme.colorScheme.secondaryContainer,
            shape = MaterialTheme.shapes.extraLarge,
            modifier = Modifier.size(96.dp),
        ) {
            Box(contentAlignment = Alignment.Center) {
                Icon(
                    icon,
                    null,
                    tint = MaterialTheme.colorScheme.onSecondaryContainer,
                    modifier = Modifier.size(40.dp),
                )
            }
        }
        Spacer(Modifier.height(24.dp))
        Text(
            title,
            style = MaterialTheme.typography.titleLarge,
            textAlign = TextAlign.Center,
            modifier = Modifier.widthIn(max = 420.dp),
        )
        if (detail != null) {
            Spacer(Modifier.height(8.dp))
            Text(
                detail,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.widthIn(max = 420.dp),
            )
        }
        if (action != null && onAction != null) {
            Spacer(Modifier.height(24.dp))
            Button(onClick = onAction) { Text(action) }
        }
    }
}

@Composable
fun PlayShuffleButtons(
    onPlay: () -> Unit,
    onShuffle: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    Row(
        modifier.fillMaxWidth().padding(horizontal = Sizes.gutter),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Button(
            onClick = onPlay,
            enabled = enabled,
            modifier = Modifier.weight(1f).height(56.dp),
            contentPadding = PaddingValues(horizontal = 16.dp),
        ) {
            Icon(
                Icons.Rounded.PlayArrow,
                null,
                modifier = Modifier.size(ButtonDefaults.IconSize),
            )
            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
            Text("Play")
        }
        FilledTonalButton(
            onClick = onShuffle,
            enabled = enabled,
            modifier = Modifier.weight(1f).height(56.dp),
            contentPadding = PaddingValues(horizontal = 16.dp),
        ) {
            Icon(
                Icons.Rounded.Shuffle,
                null,
                modifier = Modifier.size(ButtonDefaults.IconSize),
            )
            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
            Text("Shuffle")
        }
    }
}

fun lit(text: String, ranges: List<IntRange>, emphasis: SpanStyle): AnnotatedString =
    if (ranges.isEmpty()) AnnotatedString(text)
    else
        buildAnnotatedString {
            var at = 0
            ranges
                .sortedBy { it.first }
                .forEach { range ->
                    val start = range.first.coerceIn(at, text.length)
                    val end = (range.last + 1).coerceIn(start, text.length)
                    append(text.substring(at, start))
                    withStyle(emphasis) { append(text.substring(start, end)) }
                    at = end
                }
            append(text.substring(at))
        }
