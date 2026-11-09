package org.resonate.android

import android.content.Context
import android.net.Uri
import android.provider.MediaStore
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.IdenticalSongs
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.shownTitle

data class FilesRemoved(val gone: List<Track>, val kept: Int)

data class Deleted(
    val removed: Int,
    val kept: Int,
    val folded: Int = 0,
    val titles: List<String> = emptyList(),
)

object DeletionSentences {
    fun kept(words: Words, count: Int): String =
        words.plural(R.plurals.deletion_files_kept, count, count)

    fun deleted(words: Words, tracks: List<String>): String =
        if (tracks.size == 1) words.string(R.string.deletion_deleted_one, tracks.single())
        else words.plural(R.plurals.deletion_deleted_songs, tracks.size, tracks.size)

    fun folded(words: Words, count: Int): String =
        words.plural(R.plurals.deletion_folded, count, count)

    fun title(words: Words, titles: List<String>): String =
        if (titles.size == 1) words.string(R.string.deletion_title_one, titles.single())
        else words.plural(R.plurals.deletion_title_songs, titles.size, titles.size)
}

class SongDeletion(
    private val context: Context,
    private val library: LibraryRepository,
    private val consent: suspend (List<Uri>) -> Boolean,
) {
    suspend fun delete(tracks: List<Track>): Deleted =
        withContext(Dispatchers.IO) {
            val removed = removeFiles(tracks)
            if (removed.gone.isNotEmpty())
                library.native().forgetDeletedSongs(removed.gone.map { it.id })
            Deleted(
                removed.gone.size,
                removed.kept,
                0,
                removed.gone.map { it.shownTitle(library.words) },
            )
        }

    suspend fun foldCopies(sets: List<IdenticalSongs>): Deleted =
        withContext(Dispatchers.IO) {
            val native = library.native()
            var folded = 0
            var kept = 0
            for (set in sets) {
                val removed = removeFiles(set.copies)
                kept += removed.kept
                if (removed.gone.isEmpty()) continue
                val told = native.foldIdenticalSongs(set.kept.id, removed.gone.map { it.id })
                folded += told.forgotten.toInt()
            }
            Deleted(0, kept, folded)
        }

    private suspend fun removeFiles(tracks: List<Track>): FilesRemoved {
        val gone = mutableListOf<Track>()
        val asking = mutableListOf<Track>()
        var kept = 0
        for (track in tracks) {
            try {
                remove(track.uri)
                gone += track
            } catch (refused: SecurityException) {
                if (Uri.parse(track.uri).authority == MediaStore.AUTHORITY) asking += track else kept++
            } catch (failure: Exception) {
                library.report(failure)
                kept++
            }
        }
        if (asking.isNotEmpty()) {
            if (consent(asking.map { Uri.parse(it.uri) })) gone += asking else kept += asking.size
        }
        return FilesRemoved(gone, kept)
    }

    private fun remove(uri: String) {
        val own = OpenedNames.localName(uri)
        if (own != null) {
            File(File(context.filesDir, OpenedNames.FOLDER), own).let {
                check(!it.exists() || it.delete()) {
                    context.getString(R.string.deletion_song_not_removed)
                }
            }
            return
        }
        MusicDocuments.delete(context, Uri.parse(uri))
    }
}
