package org.resonate.android

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.util.Log
import java.util.Locale
import org.resonate.android.nativecore.DocumentBeside
import org.resonate.android.nativecore.MadeBeside
import org.resonate.android.nativecore.SheetsBeside
import org.resonate.android.nativecore.documentNotFound
import org.resonate.android.nativecore.documentRefused
import org.resonate.android.nativecore.lyricFolders

object SheetPlaces {
    const val SHEET_TYPE = "application/octet-stream"

    private const val TREE = "tree"
    private const val DOCUMENT = "document"

    fun reachable(uri: String): Boolean {
        val parsed = Uri.parse(uri)
        val segments = parsed.pathSegments
        return parsed.scheme == "content" &&
            segments.size == 4 &&
            segments[0] == TREE &&
            segments[2] == DOCUMENT
    }
}

class SheetsBesideTree(private val context: Context) : SheetsBeside {
    private val resolver
        get() = context.contentResolver

    private val folders: Set<String> by lazy { lyricFolders().toSet() }

    override fun sheetsBeside(uri: String): List<DocumentBeside>? =
        refusable({ Log.w("Resonate", "The documents beside a song could not be listed", it) }) {
            val (tree, parent) = placeOf(uri) ?: return@refusable null
            val beside = tree.children(parent) ?: return@refusable null
            buildList {
                for (entry in beside) {
                    if (!entry.directory) {
                        add(DocumentBeside(entry.name, tree.uri(entry.id), false))
                    } else if (entry.name.lowercase(Locale.ROOT) in folders) {
                        for (within in tree.children(entry.id).orEmpty()) {
                            if (!within.directory)
                                add(DocumentBeside(within.name, tree.uri(within.id), true))
                        }
                    }
                }
            }
        }

    override fun makeBeside(uri: String, name: String): MadeBeside? =
        refusable({ Log.w("Resonate", "A lyric sheet could not be made beside a song", it) }) {
            val (tree, parent) = placeOf(uri) ?: return@refusable null
            val made =
                DocumentsContract.createDocument(
                    resolver,
                    Uri.parse(tree.uri(parent)),
                    SheetPlaces.SHEET_TYPE,
                    name,
                ) ?: return@refusable null
            val descriptor = resolver.openFileDescriptor(made, "w")?.detachFd()
            if (descriptor == null) {
                DocumentsContract.deleteDocument(resolver, made)
                return@refusable null
            }
            MadeBeside(made.toString(), descriptor)
        }

    override fun rewriteBeside(uri: String): Int =
        try {
            resolver.openFileDescriptor(Uri.parse(uri), "wt")?.detachFd() ?: documentRefused()
        } catch (gone: java.io.FileNotFoundException) {
            documentNotFound()
        } catch (failure: Exception) {
            Log.w("Resonate", "A lyric sheet could not be opened for writing", failure)
            documentRefused()
        }

    override fun removeBeside(uri: String): Boolean =
        refusable({ Log.w("Resonate", "A lyric sheet cut short could not be removed", it) }) {
            DocumentsContract.deleteDocument(resolver, Uri.parse(uri))
        } ?: false

    private fun placeOf(uri: String): Pair<SafTree, String>? {
        if (!SheetPlaces.reachable(uri)) return null
        val document = Uri.parse(uri)
        val tree =
            DocumentsContract.buildTreeDocumentUri(
                document.authority,
                DocumentsContract.getTreeDocumentId(document),
            )
        val path = DocumentsContract.findDocumentPath(resolver, document)?.path ?: return null
        if (path.size < 2) return null
        return SafTree(context, tree) to path[path.size - 2]
    }

    private fun <T> refusable(refused: (Exception) -> Unit, action: () -> T?): T? =
        try {
            action()
        } catch (failure: Exception) {
            refused(failure)
            null
        }
}
