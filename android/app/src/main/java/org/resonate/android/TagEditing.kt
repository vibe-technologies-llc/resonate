package org.resonate.android

import android.content.Context
import android.net.Uri
import android.provider.MediaStore
import android.util.Log
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
    val label: String,
    val numbered: Boolean = false,
    val eachSongsOwn: Boolean = false,
) {
    Title("Title", eachSongsOwn = true),
    Artist("Artist"),
    Album("Album"),
    AlbumArtist("Album artist"),
    Genre("Genre"),
    Date("Year or date"),
    TrackNumber("Track", numbered = true, eachSongsOwn = true),
    TrackTotal("of", numbered = true),
    DiscNumber("Disc", numbered = true),
    DiscTotal("of", numbered = true),
    Composer("Composer");

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
    fun told(written: Int, failed: Int, failure: String?, undoing: Boolean): TagNotice =
        when {
            failure != null ->
                TagNotice(
                    if (written > 0) "$failure (${songs(failed)} left as they were)" else failure,
                    failure = true,
                )
            written > 0 ->
                TagNotice(
                    (if (undoing) "Put back the tags of " else "Saved the tags of ") + songs(written),
                    failure = false,
                )
            else -> TagNotice("Nothing to change", failure = false)
        }

    fun notAllowed(count: Int): String? =
        if (count == 0) null
        else if (count == 1) "Writing to 1 song was not allowed, so it was left as it was"
        else "Writing to $count songs was not allowed, so they were left as they were"

    fun cannotBeWritten(count: Int): String =
        "${songs(count)} cannot be written and will be left alone."

    fun editTitle(count: Int): String = if (count == 1) "Edit tags" else "Edit ${songs(count)}"

    private fun songs(count: Int): String = "$count song${if (count == 1) "" else "s"}"
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
            val native = library.native()
            ids.map { native.songTags(it) }
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
                val native = library.native()
                val uris = named(native)
                val allowed = allowedToWrite(uris, consent)
                val refused = uris.size - allowed.size
                val told =
                    if (allowed.isEmpty()) TagsWritten(0u, 0u, 0u, null) else write(native, allowed)
                mutableUndoable.value = native.tagsToUndo().isNotEmpty()
                if (told.written > 0u) library.refresh()
                val notice =
                    TagSentences.told(
                        told.written.toInt(),
                        told.failed.toInt() + refused,
                        told.failure ?: TagSentences.notAllowed(refused),
                        undoing,
                    )
                if (notice.failure) library.report(IllegalStateException(notice.text))
                else library.notice(notice.text)
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
