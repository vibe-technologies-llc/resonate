package org.resonate.android.ui.components

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.compose.animation.Crossfade
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.RectangleShape
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.resonate.android.ArtworkSizes
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.LocalResonate

@Stable
class ArtworkClock {
    var generation by mutableIntStateOf(0)
    val stamps = mutableStateMapOf<ULong, Int>()

    fun changed(songs: List<ULong>) {
        songs.forEach { stamps[it] = (stamps[it] ?: 0) + 1 }
    }
}

val LocalArtworkClock = staticCompositionLocalOf { ArtworkClock() }

@Composable
fun FollowArtworkChanges(clock: ArtworkClock) {
    val features = LocalResonate.current.features
    LaunchedEffect(features) { features.artworkGeneration.collect { clock.generation = it } }
    LaunchedEffect(features) { features.artChanges.collect(clock::changed) }
}

@Composable
fun rememberArtwork(song: ULong?, revision: ULong, edgePx: Int): Bitmap? {
    val features = LocalResonate.current.features
    val clock = LocalArtworkClock.current
    val stamp = song?.let { clock.stamps[it] } ?: 0
    val bucket = ArtworkSizes.edgeFor(edgePx)
    val generation = clock.generation
    val cached =
        remember(song, revision, generation, stamp, bucket) {
            song?.let { features.cachedArtwork(it, bucket, generation, stamp) }
        }
    val drawn = remember { DrawnSong() }
    val bitmap by
        produceState(cached, song, revision, generation, stamp, bucket) {
            if (cached != null || song != drawn.song) value = cached
            drawn.song = song
            value = song?.let { features.artworkBitmap(it, bucket, generation, stamp) }
        }
    return bitmap
}

private class DrawnSong {
    var song: ULong? = null
}

@Composable
fun rememberPicture(bytes: ByteArray?): Bitmap? {
    val bitmap by
        produceState<Bitmap?>(null, bytes) {
            value =
                bytes?.let {
                    withContext(Dispatchers.Default) {
                        BitmapFactory.decodeByteArray(it, 0, it.size)
                    }
                }
        }
    return bitmap
}

@Composable
fun Artwork(
    track: Track?,
    size: Dp,
    modifier: Modifier = Modifier,
    shape: Shape = MaterialTheme.shapes.small,
    placeholder: ImageVector = Icons.Rounded.MusicNote,
) {
    val edge = with(LocalDensity.current) { size.roundToPx() }
    Picture(
        bitmap = rememberArtwork(track?.id, track?.artRevision ?: 0u, edge),
        modifier = modifier.size(size),
        shape = shape,
        placeholder = placeholder,
    )
}

@Composable
fun SongArtwork(
    song: ULong?,
    size: Dp,
    modifier: Modifier = Modifier,
    shape: Shape = MaterialTheme.shapes.small,
    placeholder: ImageVector = Icons.Rounded.MusicNote,
) {
    val edge = with(LocalDensity.current) { size.roundToPx() }
    Picture(
        bitmap = rememberArtwork(song, 0u, edge),
        modifier = modifier.size(size),
        shape = shape,
        placeholder = placeholder,
    )
}

@Composable
fun WideArtwork(
    track: Track?,
    modifier: Modifier = Modifier,
    shape: Shape = MaterialTheme.shapes.extraLarge,
    placeholder: ImageVector = Icons.Rounded.MusicNote,
) {
    BoxWithConstraints(modifier.fillMaxWidth().aspectRatio(1f)) {
        val edge = with(LocalDensity.current) { maxWidth.roundToPx() }
        Picture(
            bitmap = rememberArtwork(track?.id, track?.artRevision ?: 0u, edge),
            modifier = Modifier.fillMaxSize(),
            shape = shape,
            placeholder = placeholder,
        )
    }
}

@Composable
fun Picture(
    bitmap: Bitmap?,
    modifier: Modifier = Modifier,
    shape: Shape = MaterialTheme.shapes.small,
    placeholder: ImageVector = Icons.Rounded.MusicNote,
) {
    Box(
        modifier.clip(shape).background(MaterialTheme.colorScheme.surfaceContainerHighest),
        contentAlignment = Alignment.Center,
    ) {
        Crossfade(bitmap, label = "artwork") { shown ->
            if (shown != null) {
                val image = remember(shown) { shown.asImageBitmap() }
                Image(
                    image,
                    contentDescription = null,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxSize(),
                )
            } else {
                Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
                    Icon(
                        placeholder,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.fillMaxSize(PLACEHOLDER_SHARE),
                    )
                }
            }
        }
    }
}

@Composable
fun PlaylistCover(
    covers: List<ULong>,
    size: Dp,
    modifier: Modifier = Modifier,
    shape: Shape = MaterialTheme.shapes.small,
) {
    when {
        covers.isEmpty() ->
            Picture(
                null,
                modifier.size(size),
                shape,
                Icons.AutoMirrored.Rounded.QueueMusic,
            )
        covers.size < 4 -> SongArtwork(covers.first(), size, modifier, shape)
        else -> {
            val quarter = size / 2
            Box(modifier.size(size).clip(shape)) {
                Column {
                    Row {
                        SongArtwork(covers[0], quarter, shape = RectangleShape)
                        SongArtwork(covers[1], quarter, shape = RectangleShape)
                    }
                    Row {
                        SongArtwork(covers[2], quarter, shape = RectangleShape)
                        SongArtwork(covers[3], quarter, shape = RectangleShape)
                    }
                }
            }
        }
    }
}

private const val PLACEHOLDER_SHARE = 0.42f
