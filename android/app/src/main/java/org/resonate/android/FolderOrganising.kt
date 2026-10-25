package org.resonate.android

import android.content.Context
import android.content.SharedPreferences
import android.net.Uri
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.FiledSong
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.OrganisePlan
import org.resonate.android.nativecore.defaultOrganiseLayout
import org.resonate.android.ui.counted

data class OrganiseUndo(val folder: Uri, val back: List<FiledSong>)

object OrganiseSentences {
    const val WAITING = "Filing songs once the scan ends"
    const val LISTED_AT_MOST = 50

    fun confirmTitle(moves: Int): String = "Move ${counted(moves, "song")}?"

    fun told(moved: Int, failed: Int, undoing: Boolean): String {
        val said =
            if (undoing) "Put ${counted(moved, "song")} back where they were"
            else "Filed ${counted(moved, "song")}"
        return if (failed > 0) "$said; ${counted(failed, "song")} could not be moved" else said
    }

    fun shown(uri: String, rootId: String): String {
        val id = OpenedNames.unescaped(uri.substringAfterLast("/document/"))
        val relative = Ids.relative(id, rootId) ?: listOf(id)
        return "from " + relative.joinToString(" / ")
    }
}

class FolderOrganising(
    private val context: Context,
    private val library: LibraryRepository,
    private val settings: SharedPreferences,
    private val scope: CoroutineScope,
    private val reread: suspend (MobilePlayer) -> Unit,
) {
    val busy = MutableStateFlow(false)
    val undo = MutableStateFlow<OrganiseUndo?>(null)

    fun layout(): String = settings.getString(LAYOUT_KEY, null) ?: defaultOrganiseLayout()

    fun keepLayout(layout: String) {
        settings.edit().putString(LAYOUT_KEY, layout).apply()
    }

    suspend fun plan(folder: Uri, layout: String): OrganisePlan =
        withContext(Dispatchers.IO) {
            library.native().planOrganise(MusicDocuments.rootDocument(folder), layout)
        }

    fun organise(folder: Uri, songs: List<FiledSong>) = file(folder, songs, undoing = false)

    fun putBack() {
        val held = undo.value ?: return
        file(held.folder, held.back, undoing = true)
    }

    private fun file(folder: Uri, songs: List<FiledSong>, undoing: Boolean) {
        if (!busy.compareAndSet(false, true)) return
        if (library.scanning.value) library.notice(OrganiseSentences.WAITING)
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
                reread(library.native())
                library.notice(OrganiseSentences.told(filed.moved.size, filed.failed, undoing))
            } finally {
                busy.value = false
            }
        }
    }

    companion object {
        const val LAYOUT_KEY = "organise-layout"
    }
}
