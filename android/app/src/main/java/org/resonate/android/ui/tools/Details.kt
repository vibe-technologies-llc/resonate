package org.resonate.android.ui.tools

import android.content.ClipData
import android.content.ClipboardManager
import android.net.Uri
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.AutoFixHigh
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material.icons.rounded.PersonSearch
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import org.resonate.android.nativecore.ArtistInformation
import org.resonate.android.nativecore.Inspector
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Artwork
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.shownTitle

class DetailsViewModel(private val resonate: Resonate, private val id: ULong) : ViewModel() {
    private val features = resonate.features

    val inspector: StateFlow<Inspector?> =
        features.inspector
            .map { it?.takeIf { found -> found.track.id == id } }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)

    val artist: StateFlow<ArtistInformation?> = features.artist
    val online: StateFlow<Boolean> = features.online
    val enriching: StateFlow<Boolean> = features.enriching

    init {
        features.loadInspector(id)
    }

    fun enrich(track: Track) = features.enrich(listOf(track))

    fun lookUpArtist() = features.loadArtist(id)

    fun open(link: String) = resonate.host.open(Uri.parse(link))
}

@Composable
fun DetailsScreen(track: ULong) {
    val song = rememberSong(track)
    val model = resonateViewModel(key = "details-$track") { DetailsViewModel(it, track) }
    val inspector by model.inspector.collectAsStateWithLifecycle()
    val artist by model.artist.collectAsStateWithLifecycle()
    val online by model.online.collectAsStateWithLifecycle()
    val enriching by model.enriching.collectAsStateWithLifecycle()
    var askedArtist by rememberSaveable { mutableStateOf(false) }
    Page("Song details") { padding ->
        if (song == null) {
            MissingSong(padding)
            return@Page
        }
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
            LazyColumn(
                contentPadding = padding,
                modifier = Modifier.widthIn(max = 840.dp).fillMaxWidth().testTag("details-list"),
            ) {
                item(key = "header") { SongHeader(song) }
                item(key = "actions") {
                    FlowRow(
                        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        FilledTonalButton(
                            onClick = { model.enrich(song) },
                            enabled = online && !enriching,
                            contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                        ) {
                            Icon(Icons.Rounded.AutoFixHigh, null, Modifier.size(ButtonDefaults.IconSize))
                            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                            Text(if (enriching) "Finding…" else "Find artwork and details")
                        }
                        if (online) {
                            OutlinedButton(
                                onClick = {
                                    askedArtist = true
                                    model.lookUpArtist()
                                },
                                contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                            ) {
                                Icon(Icons.Rounded.PersonSearch, null, Modifier.size(ButtonDefaults.IconSize))
                                Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                                Text("Look up artist")
                            }
                        }
                    }
                }
                item(key = "audio") { SectionHeader("Audio") }
                if (inspector == null) {
                    item(key = "reading") {
                        Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) {
                            LoadingIndicator()
                        }
                    }
                }
                details(technicalDetails(song, inspector), "audio")
                item(key = "tags") { SectionHeader("Tags") }
                details(taggedDetails(song, inspector), "tags")
                val found = artist
                if (askedArtist && found != null) {
                    artistItems(found, model::open)
                }
            }
        }
    }
}

@Composable
fun SongHeader(track: Track) {
    Column(
        Modifier.fillMaxWidth().padding(horizontal = Sizes.gutter, vertical = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Artwork(track, HEADER_ART, shape = MaterialTheme.shapes.extraLarge)
        Spacer(Modifier.height(16.dp))
        Text(
            track.shownTitle,
            style = MaterialTheme.typography.headlineSmall,
            textAlign = TextAlign.Center,
            maxLines = 3,
            overflow = TextOverflow.Ellipsis,
        )
        Text(
            listOf(track.shownArtist, track.album).filter { it.isNotBlank() }.joinToString(" · "),
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

private fun LazyListScope.details(rows: List<Detail>, group: String) {
    items(rows, key = { "$group-${it.label}" }) { detail -> DetailRow(detail) }
}

@Composable
private fun DetailRow(detail: Detail) {
    val context = LocalContext.current
    ListItem(
        colors = ListItemDefaults.colors(containerColor = Color.Transparent),
        overlineContent = { Text(detail.label) },
        trailingContent =
            if (detail.copyable) {
                {
                    IconButton(
                        onClick = {
                            context
                                .getSystemService(ClipboardManager::class.java)
                                ?.setPrimaryClip(ClipData.newPlainText(detail.label, detail.value))
                        }
                    ) {
                        Icon(Icons.Rounded.ContentCopy, contentDescription = "Copy ${detail.label}")
                    }
                }
            } else null,
    ) { Text(detail.value, maxLines = 6, overflow = TextOverflow.Ellipsis) }
}

private fun LazyListScope.artistItems(artist: ArtistInformation, open: (String) -> Unit) {
    item(key = "artist") { SectionHeader(artist.name.ifBlank { "Artist" }) }
    val rows =
        listOf(
                Detail("Type", artist.kind),
                Detail("Country", artist.country),
                Detail("Area", artist.area),
                Detail("Genres", artist.genres.joinToString(", ")),
            )
            .filter { it.value.isNotBlank() }
    details(rows, "artist")
    items(artist.links, key = { "link-$it" }) { link ->
        ListItem(
            onClick = { open(link) },
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            supportingContent = { Text(link, maxLines = 1, overflow = TextOverflow.Ellipsis) },
            trailingContent = { Icon(Icons.AutoMirrored.Rounded.OpenInNew, contentDescription = "Open link") },
        ) { Text(Uri.parse(link).host ?: link, maxLines = 1, overflow = TextOverflow.Ellipsis) }
    }
}

private val HEADER_ART = 200.dp
