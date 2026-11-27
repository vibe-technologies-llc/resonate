package org.resonate.android

import android.content.Context
import android.content.SharedPreferences
import android.net.Uri
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.FiledSong
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.OrganisePlan
import org.resonate.android.nativecore.defaultOrganiseLayout

data class OrganiseUndo(val folder: Uri, val back: List<FiledSong>)

object OrganiseSentences {
    const val LISTED_AT_MOST = 50

    fun confirmTitle(words: Words, moves: Int): String =
        words.plural(R.plurals.organise_confirm, moves, moves)

    fun told(words: Words, moved: Int, failed: Int, undoing: Boolean): String {
        val said =
            if (undoing) words.plural(R.plurals.organise_put_back, moved, moved)
            else words.plural(R.plurals.organise_filed, moved, moved)
        return if (failed > 0) words.plural(R.plurals.organise_not_moved, failed, said, failed)
        else said
    }

    fun shown(words: Words, uri: String, rootId: String): String {
        val id = OpenedNames.unescaped(uri.substringAfterLast("/document/"))
        val relative = Ids.relative(id, rootId) ?: listOf(id)
        return words.string(R.string.organise_from, relative.joinToString(" / "))
    }
}

class FolderOrganising(
    private val context: Context,
    private val library: LibraryRepository,
    private val settings: SharedPreferences,
    private val scope: CoroutineScope,
    private val reread: suspend (MobilePlayer) -> Unit,
    private val filable: (Uri) -> Boolean,
    private val tellFinding: (String) -> Unit,
) {
    val busy = MutableStateFlow(false)
    val undo = MutableStateFlow<OrganiseUndo?>(null)
    val newSongs = MutableStateFlow(settings.getBoolean(NEW_SONGS_KEY, false))
    val arriving = MutableStateFlow(false)

    fun layout(): String = settings.getString(LAYOUT_KEY, null) ?: defaultOrganiseLayout()

    fun keepLayout(layout: String) {
        settings.edit().putString(LAYOUT_KEY, layout).apply()
    }

    fun organiseNewSongs(on: Boolean, layout: String) {
        settings.edit().putBoolean(NEW_SONGS_KEY, on).putString(LAYOUT_KEY, layout).apply()
        newSongs.value = on
    }

    suspend fun plan(folder: Uri, layout: String): OrganisePlan =
        withContext(Dispatchers.IO) {
            library.holding {
                it.planOrganise(MusicDocuments.rootDocument(folder), layout, emptyList())
            }
        }

    fun fileWhatArrived(folder: Uri, arrived: List<String>) {
        if (!filesWhatArrivesIn(folder) || arrived.isEmpty()) return
        scope.launch { library.afterScans { fileWhatArrivedNow(folder) { arrived } } }
    }

    fun filesWhatArrivesIn(folder: Uri): Boolean =
        newSongs.value && folder != MusicDocuments.downloads && filable(folder)

    suspend fun fileWhatArrivedNow(folder: Uri, arrived: suspend (MobilePlayer) -> List<String>) {
        if (!filesWhatArrivesIn(folder)) return
        arriving.value = true
        try {
            val plan =
                withContext(Dispatchers.IO) {
                    library.holding { native ->
                        val only = arrived(native)
                        if (only.isEmpty()) null
                        else native.planOrganise(MusicDocuments.rootDocument(folder), layout(), only)
                    }
                }
            if (plan == null || plan.moves.isEmpty()) return
            val result = withContext(Dispatchers.IO) { FolderFiling(SafTree(context, folder)).file(plan.moves) }
            library.inOrder { native -> native.documentsMoved(result.moved) }
            library.holding { reread(it) }
            val said = OrganiseSentences.told(library.words, result.moved.size, result.failed, false)
            if (result.failed > 0) library.notice(said) else tellFinding(said)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            library.report(failure)
        } finally {
            arriving.value = false
        }
    }

    fun organise(folder: Uri, songs: List<FiledSong>) = file(folder, songs, undoing = false)

    fun putBack() {
        val held = undo.value ?: return
        file(held.folder, held.back, undoing = true)
    }

    private fun file(folder: Uri, songs: List<FiledSong>, undoing: Boolean) {
        if (!busy.compareAndSet(false, true)) return
        if (library.scanning.value) library.notice(R.string.organise_waiting)
        scope.launch {
            try {
                val filed =
                    library.afterScans {
                        val result =
                            withContext(Dispatchers.IO) {
                                FolderFiling(SafTree(context, folder)).file(songs)
                            }
                        library.inOrder { native -> native.documentsMoved(result.moved) }
                        result
                    }
                undo.value =
                    if (undoing || filed.back.isEmpty()) null else OrganiseUndo(folder, filed.back)
                library.holding { reread(it) }
                library.notice(
                    OrganiseSentences.told(library.words, filed.moved.size, filed.failed, undoing)
                )
            } finally {
                busy.value = false
            }
        }
    }

    companion object {
        const val LAYOUT_KEY = "organise-layout"
        const val NEW_SONGS_KEY = "organise-new-songs"
    }
}
