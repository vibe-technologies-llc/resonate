package org.resonate.android

import android.content.Context
import android.net.Uri
import android.provider.MediaStore
import android.util.Log
import androidx.annotation.StringRes
import java.io.File
import java.io.FileNotFoundException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.DocumentRewriting
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.SongFile
import org.resonate.android.nativecore.SongTags
import org.resonate.android.nativecore.TagsWritten
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.audioExtensions
import org.resonate.android.nativecore.documentNotFound
import org.resonate.android.nativecore.documentRefused

enum class TagName(
    @StringRes val label: Int,
    val numbered: Boolean = false,
    val eachSongsOwn: Boolean = false,
) {
    Title(R.string.tagging_title, eachSongsOwn = true),
    Artist(R.string.tagging_artist),
    Album(R.string.tagging_album),
    AlbumArtist(R.string.tagging_album_artist),
    Genre(R.string.tagging_genre),
    Date(R.string.tagging_date),
    TrackNumber(R.string.tagging_track, numbered = true, eachSongsOwn = true),
    TrackTotal(R.string.tagging_of, numbered = true),
    DiscNumber(R.string.tagging_disc, numbered = true),
    DiscTotal(R.string.tagging_of, numbered = true),
    Composer(R.string.tagging_composer);

    fun of(tags: SongTags): String? =
        when (this) {
            Title -> tags.title
            Artist -> tags.artist
            Album -> tags.album
            AlbumArtist -> tags.albumArtist
            Genre -> tags.genre
            Date -> tags.date
            TrackNumber -> tags.trackNumber
            TrackTotal -> tags.trackTotal
            DiscNumber -> tags.discNumber
            DiscTotal -> tags.discTotal
            Composer -> tags.composer
        }

    fun set(tags: SongTags, value: String?) {
        when (this) {
            Title -> tags.title = value
            Artist -> tags.artist = value
            Album -> tags.album = value
            AlbumArtist -> tags.albumArtist = value
            Genre -> tags.genre = value
            Date -> tags.date = value
            TrackNumber -> tags.trackNumber = value
            TrackTotal -> tags.trackTotal = value
            DiscNumber -> tags.discNumber = value
            DiscTotal -> tags.discTotal = value
            Composer -> tags.composer = value
        }
    }
}

data class SharedTags(val common: Map<TagName, String>, val differing: Set<TagName>)

object TagDiff {
    const val SONGS_READ_AT_MOST = 500

    fun common(all: List<SongTags>): SharedTags {
        val common = mutableMapOf<TagName, String>()
        val differing = mutableSetOf<TagName>()
        for (name in TagName.entries) {
            val values = all.map { name.of(it).orEmpty() }.toSet()
            when {
                values.size == 1 -> common[name] = values.single()
                values.size > 1 -> differing += name
            }
        }
        return SharedTags(common, differing)
    }

    fun changed(
        shown: Map<TagName, String>,
        read: Map<TagName, String>,
        mixed: Set<TagName>,
        emptied: Set<TagName> = emptySet(),
    ): List<TagName> =
        TagName.entries.filter { name ->
            val now = shown[name].orEmpty()
            if (name in mixed) now.isNotEmpty() || name in emptied else now != read[name].orEmpty()
        }

    fun change(shown: Map<TagName, String>, changed: List<TagName>): SongTags =
        SongTags().also { tags -> changed.forEach { it.set(tags, shown[it].orEmpty()) } }
}

data class TagNotice(val text: String, val failure: Boolean)

object TagSentences {
    fun told(
        words: Words,
        written: Int,
        failed: Int,
        failure: String?,
        undoing: Boolean,
    ): TagNotice =
        when {
            failure != null ->
                TagNotice(
                    if (written > 0)
                        words.plural(R.plurals.tagging_failed_and_left, failed, failure, failed)
                    else failure,
                    failure = true,
                )
            written > 0 ->
                TagNotice(
                    words.plural(
                        if (undoing) R.plurals.tagging_put_back else R.plurals.tagging_saved,
                        written,
                        written,
                    ),
                    failure = false,
                )
            else -> TagNotice(words.string(R.string.tagging_nothing_to_change), failure = false)
        }

    fun notAllowed(words: Words, count: Int): String? =
        if (count == 0) null else words.plural(R.plurals.tagging_not_allowed, count, count)

    fun cannotBeWritten(words: Words, count: Int): String =
        words.plural(R.plurals.tagging_cannot_be_written, count, count)

    fun editTitle(words: Words, count: Int): String =
        if (count == 1) words.string(R.string.tagging_edit_one)
        else words.plural(R.plurals.tagging_edit_several, count, count)
}

object TagFiles {
    fun songFile(uri: String, openedFolder: File): SongFile =
        SongFile(uri, OpenedNames.localName(uri)?.let { File(openedFolder, it).absolutePath }.orEmpty())

    fun extensionOf(name: String): String = name.substringAfterLast('.', "").lowercase()

    private val mediaFile = Regex("^content://media/([^/]+)/file/(\\d+)$")

    fun writeUri(uri: String): String =
        mediaFile.matchEntire(uri)?.let { found ->
            val (volume, id) = found.destructured
            "content://media/$volume/audio/media/$id"
        } ?: uri
}

class DocumentRewrites(private val context: Context) : DocumentRewriting {
    override fun rewriteDocument(uri: String): Int =
        try {
            context.contentResolver
                .openFileDescriptor(Uri.parse(TagFiles.writeUri(uri)), "wt")
                ?.detachFd()
                ?: documentRefused()
        } catch (gone: FileNotFoundException) {
            documentNotFound()
        } catch (failure: Exception) {
            Log.w("Resonate", "A document could not be opened for writing", failure)
            documentRefused()
        }
}

class TagEditing(
    private val context: Context,
    private val library: LibraryRepository,
    private val scope: CoroutineScope,
    private val deletable: (Track) -> Boolean,
) {
    @Volatile private var writableKinds: Set<String> = emptySet()
    private val mutableUndoable = MutableStateFlow(false)

    val undoable: StateFlow<Boolean> = mutableUndoable

    fun initialize(native: MobilePlayer) {
        writableKinds = audioExtensions().filter { native.tagsWritable("song.$it") }.toSet()
    }

    fun writable(track: Track): Boolean =
        TagFiles.extensionOf(track.name) in writableKinds && deletable(track)

    suspend fun read(ids: List<ULong>): List<SongTags> =
        withContext(Dispatchers.IO) {
            library.holding { native -> ids.map { native.songTags(it) } }
        }

    fun edit(tracks: List<Track>, change: SongTags, consent: suspend (List<Uri>) -> Boolean) {
        run(undoing = false, consent, { _ -> tracks.map { it.uri } }) { native, uris ->
            native.editTags(files(uris), change)
        }
    }

    fun undo(consent: suspend (List<Uri>) -> Boolean) {
        run(undoing = true, consent, { native -> native.tagsToUndo() }) { native, uris ->
            native.undoTags(files(uris))
        }
    }

    private fun run(
        undoing: Boolean,
        consent: suspend (List<Uri>) -> Boolean,
        named: (MobilePlayer) -> List<String>,
        write: (MobilePlayer, List<String>) -> TagsWritten,
    ) {
        scope.launch {
            try {
                library.holding { native ->
                    val uris = named(native)
                    val allowed = allowedToWrite(uris, consent)
                    val refused = uris.size - allowed.size
                    val told =
                        if (allowed.isEmpty()) TagsWritten(0u, 0u, 0u, null) else write(native, allowed)
                    mutableUndoable.value = native.tagsToUndo().isNotEmpty()
                    if (told.written > 0u) library.refresh()
                    val notice =
                        TagSentences.told(
                            library.words,
                            told.written.toInt(),
                            told.failed.toInt() + refused,
                            told.failure ?: TagSentences.notAllowed(library.words, refused),
                            undoing,
                        )
                    if (notice.failure) library.report(IllegalStateException(notice.text))
                    else library.notice(notice.text)
                }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
    }

    private suspend fun allowedToWrite(
        uris: List<String>,
        consent: suspend (List<Uri>) -> Boolean,
    ): List<String> {
        val needing =
            withContext(Dispatchers.IO) {
                uris.filter { needsConsent(Uri.parse(TagFiles.writeUri(it))) }
            }
        if (needing.isEmpty()) return uris
        if (consent(needing.map { Uri.parse(TagFiles.writeUri(it)) })) return uris
        return uris - needing.toSet()
    }

    private fun needsConsent(uri: Uri): Boolean {
        if (uri.authority != MediaStore.AUTHORITY) return false
        return try {
            context.contentResolver.openFileDescriptor(uri, "rw")?.close()
            false
        } catch (refused: Exception) {
            true
        }
    }

    private fun files(uris: List<String>): List<SongFile> {
        val opened = File(context.filesDir, OpenedNames.FOLDER)
        return uris.map { TagFiles.songFile(it, opened) }
    }
}
