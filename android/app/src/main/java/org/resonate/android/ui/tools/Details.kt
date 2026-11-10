package org.resonate.android.ui.tools

import android.content.ClipData
import android.content.ClipboardManager
import android.net.Uri
import android.util.Log
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.TravelExplore
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
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.ArtistInformation
import org.resonate.android.nativecore.Inspector
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Artwork
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.components.Sizes
import org.resonate.android.ui.components.rememberLargeText
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.ui.shownArtist
import org.resonate.android.ui.shownTitle
import org.resonate.android.words

sealed interface InspectorState {
    data object Reading : InspectorState

    data object Failed : InspectorState

    data class Read(val inspector: Inspector) : InspectorState
}

class DetailsViewModel(private val resonate: Resonate, private val id: ULong) : ViewModel() {
    private val features = resonate.features
    private val mutableInspector = MutableStateFlow<InspectorState>(InspectorState.Reading)
    private var reading: Job? = null

    val inspector: StateFlow<InspectorState> = mutableInspector.asStateFlow()

    val artist: StateFlow<ArtistInformation?> = features.artist
    val online: StateFlow<Boolean> = features.online
    val enriching: StateFlow<Boolean> = features.enriching

    init {
        features.artist.value = null
        read()
    }

    fun read() {
        reading?.cancel()
        mutableInspector.value = InspectorState.Reading
        reading =
            viewModelScope.launch {
                val found = resonate.read { native -> withContext(Dispatchers.IO) { native.inspector(id) } }
                found.onFailure { Log.w("Resonate", "The song could not be inspected", it) }
                mutableInspector.value = found.fold({ InspectorState.Read(it) }, { InspectorState.Failed })
            }
    }

    fun enrich(track: Track) = features.enrich(listOf(track))

    fun lookUpArtist() = features.loadArtist(id)

    fun open(link: String) = resonate.host.open(Uri.parse(link))
}

@Composable
fun DetailsScreen(track: ULong) {
    val song = rememberSong(track)
    val model = resonateViewModel(key = "details-$track") { DetailsViewModel(it, track) }
    val reading by model.inspector.collectAsStateWithLifecycle()
    val inspector = (reading as? InspectorState.Read)?.inspector
    val artist by model.artist.collectAsStateWithLifecycle()
    val online by model.online.collectAsStateWithLifecycle()
    val enriching by model.enriching.collectAsStateWithLifecycle()
    var askedArtist by rememberSaveable { mutableStateOf(false) }
    val words = words()
    Page(stringResource(R.string.details_title)) { padding ->
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
                            Icon(Icons.Rounded.TravelExplore, null, Modifier.size(ButtonDefaults.IconSize))
                            Spacer(Modifier.width(ButtonDefaults.IconSpacing))
                            Text(
                                stringResource(
                                    if (enriching) R.string.details_finding
                                    else R.string.details_find
                                )
                            )
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
                                Text(stringResource(R.string.details_look_up_artist))
                            }
                        }
                    }
                }
                item(key = "audio") { SectionHeader(stringResource(R.string.details_audio)) }
                when (reading) {
                    InspectorState.Reading ->
                        item(key = "reading") {
                            Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) {
                                LoadingIndicator()
                            }
                        }
                    InspectorState.Failed ->
                        item(key = "unread") {
                            ListItem(
                                colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                                supportingContent = {
                                    Text(stringResource(R.string.details_unread_detail))
                                },
                                trailingContent = {
                                    TextButton(onClick = model::read) {
                                        Text(stringResource(R.string.details_try_again))
                                    }
                                },
                            ) {
                                Text(stringResource(R.string.details_unread))
                            }
                        }
                    is InspectorState.Read -> Unit
                }
                details(technicalDetails(words, song, inspector), "audio")
                item(key = "tags") { SectionHeader(stringResource(R.string.details_tags)) }
                details(taggedDetails(words, song, inspector), "tags")
                val found = artist
                if (askedArtist && found != null) {
                    artistItems(words, found, model::open)
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
    val stacked = rememberLargeText()
    val label: @Composable (Modifier) -> Unit = { modifier ->
        Text(
            detail.label,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = modifier,
        )
    }
    val value: @Composable (Modifier) -> Unit = { modifier ->
        Text(
            detail.value,
            style = MaterialTheme.typography.bodyLarge,
            maxLines = 6,
            overflow = TextOverflow.Ellipsis,
            modifier = modifier,
        )
    }
    Row(
        Modifier.fillMaxWidth()
            .heightIn(min = DETAIL_ROW_HEIGHT)
            .padding(start = Sizes.gutter, end = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (stacked) {
            Column(
                Modifier.weight(1f).padding(vertical = 8.dp).semantics(mergeDescendants = true) {}
            ) {
                label(Modifier)
                value(Modifier)
            }
        } else {
            Row(
                Modifier.weight(1f).padding(vertical = 8.dp).semantics(mergeDescendants = true) {},
                verticalAlignment = Alignment.Top,
            ) {
                label(Modifier.weight(DETAIL_LABEL_SHARE).padding(top = 2.dp, end = 12.dp))
                value(Modifier.weight(1f - DETAIL_LABEL_SHARE))
            }
        }
        Box(Modifier.size(COPY_SLOT), contentAlignment = Alignment.Center) {
            if (detail.copyable) {
                IconButton(
                    onClick = {
                        context
                            .getSystemService(ClipboardManager::class.java)
                            ?.setPrimaryClip(ClipData.newPlainText(detail.label, detail.value))
                    }
                ) {
                    Icon(
                        Icons.Rounded.ContentCopy,
                        contentDescription = stringResource(R.string.details_copy, detail.label),
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(20.dp),
                    )
                }
            }
        }
    }
}

private fun LazyListScope.artistItems(
    words: Words,
    artist: ArtistInformation,
    open: (String) -> Unit,
) {
    item(key = "artist") {
        SectionHeader(artist.name.ifBlank { stringResource(R.string.details_artist) })
    }
    val rows =
        listOf(
                Detail(words.string(R.string.details_artist_type), artist.kind),
                Detail(words.string(R.string.details_artist_country), artist.country),
                Detail(words.string(R.string.details_artist_area), artist.area),
                Detail(
                    words.string(R.string.details_artist_genres),
                    artist.genres.joinToString(", "),
                ),
            )
            .filter { it.value.isNotBlank() }
    details(rows, "artist")
    items(artist.links.distinct(), key = { "link-$it" }) { link ->
        ListItem(
            onClick = { open(link) },
            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
            supportingContent = { Text(link, maxLines = 1, overflow = TextOverflow.Ellipsis) },
            trailingContent = {
                Icon(
                    Icons.AutoMirrored.Rounded.OpenInNew,
                    contentDescription = stringResource(R.string.details_open_link),
                )
            },

        ) { Text(Uri.parse(link).host ?: link, maxLines = 1, overflow = TextOverflow.Ellipsis) }
    }
}

private val HEADER_ART = 200.dp
private val DETAIL_ROW_HEIGHT = 44.dp
private val COPY_SLOT = 48.dp

private const val DETAIL_LABEL_SHARE = 0.36f
