package org.resonate.android

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import java.io.File
import java.io.IOException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.Track

object OpenedNames {
    const val LOCAL_PREFIX = "resonate-folder://local/"
    const val FOLDER = "opened"

    private const val UNRESERVED = "-._~"

    fun localUri(name: String): String = LOCAL_PREFIX + escaped(name)

    fun localName(uri: String): String? =
        uri.takeIf { it.startsWith(LOCAL_PREFIX) }?.removePrefix(LOCAL_PREFIX)?.let(::unescaped)

    private fun escaped(text: String): String =
        buildString {
            for (byte in text.toByteArray(Charsets.UTF_8)) {
                val letter = byte.toInt().toChar()
                if (byte >= 0 && (letter.isLetterOrDigit() || letter in UNRESERVED)) append(letter)
                else append('%').append("%02X".format(byte.toInt() and 0xFF))
            }
        }

    fun unescaped(text: String): String {
        val bytes = java.io.ByteArrayOutputStream()
        var at = 0
        while (at < text.length) {
            if (text[at] == '%' && at + 2 < text.length) {
                bytes.write(text.substring(at + 1, at + 3).toInt(16))
                at += 3
            } else {
                bytes.write(text[at].toString().toByteArray(Charsets.UTF_8))
                at++
            }
        }
        return bytes.toString(Charsets.UTF_8.name())
    }

    fun withinTree(documentId: String, treeId: String): Boolean =
        documentId == treeId ||
            documentId.startsWith(if (treeId.endsWith(":")) treeId else "$treeId/")

    fun treesOverlap(first: String, second: String): Boolean =
        first != second && (withinTree(first, second) || withinTree(second, first))

    fun safeName(wanted: String?): String =
        wanted
            .orEmpty()
            .substringAfterLast('/')
            .map { if (it in "\\?:*\"<>|" || it.isISOControl()) '_' else it }
            .joinToString("")
            .trim()
            .ifEmpty { "Opened song" }

    fun nameAmong(wanted: String, taken: Set<String>): String {
        if (wanted !in taken) return wanted
        val stem = wanted.substringBeforeLast('.', wanted)
        val extension = wanted.substring(stem.length)
        return generateSequence(2) { it + 1 }
            .map { "$stem ($it)$extension" }
            .first { it !in taken }
    }
}

class OpenedFiles(private val context: Context, private val library: LibraryRepository) {
    private val folder
        get() = File(context.filesDir, OpenedNames.FOLDER)

    fun descriptor(uri: String): Int? {
        val name = OpenedNames.localName(uri) ?: return null
        return try {
            android.os.ParcelFileDescriptor.open(
                    File(folder, name),
                    android.os.ParcelFileDescriptor.MODE_READ_ONLY,
                )
                .detachFd()
        } catch (gone: java.io.FileNotFoundException) {
            null
        }
    }

    suspend fun track(opened: Uri): Track =
        withContext(Dispatchers.IO) {
            val native = library.native()
            val kept = inGrantedTree(opened) ?: copied(opened)
            native.indexDocument(kept.first, kept.second)
        }

    private fun inGrantedTree(opened: Uri): Pair<String, String>? {
        if (!DocumentsContract.isDocumentUri(context, opened)) return null
        val documentId = DocumentsContract.getDocumentId(opened)
        val tree =
            context.contentResolver.persistedUriPermissions
                .filter { it.isReadPermission && it.uri.authority == opened.authority }
                .map { it.uri }
                .filter { DocumentsContract.isTreeUri(it) }
                .filter { OpenedNames.withinTree(documentId, DocumentsContract.getTreeDocumentId(it)) }
                .maxByOrNull { DocumentsContract.getTreeDocumentId(it).length } ?: return null
        val inside = DocumentsContract.buildDocumentUriUsingTree(tree, documentId)
        return inside.toString() to displayName(opened)
    }

    private fun copied(opened: Uri): Pair<String, String> {
        val name = displayName(opened)
        folder.mkdirs()
        val taken = folder.list().orEmpty().toSet()
        val kept = OpenedNames.nameAmong(name, taken)
        val destination = File(folder, kept)
        try {
            context.contentResolver.openInputStream(opened)?.use { input ->
                destination.outputStream().use { input.copyTo(it) }
            } ?: throw IOException("The file could not be opened")
        } catch (failure: Exception) {
            destination.delete()
            throw failure
        }
        return OpenedNames.localUri(kept) to name
    }

    private fun displayName(opened: Uri): String {
        val asked =
            context.contentResolver
                .query(opened, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { if (it.moveToFirst()) it.getString(0) else null }
        return OpenedNames.safeName(asked ?: opened.lastPathSegment)
    }
}
