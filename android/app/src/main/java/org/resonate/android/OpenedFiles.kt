package org.resonate.android

import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.os.storage.StorageManager
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.nio.file.Files
import java.util.Arrays
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.Track

object OpenedNames {
    const val LOCAL_PREFIX = "resonate-folder://local/"
    const val FOLDER = "opened"

    private const val UNRESERVED = "-._~"
    private const val COPIES_CHECKED = 100
    private val DOT_NAMES = setOf(".", "..")

    fun localUri(name: String): String = LOCAL_PREFIX + escaped(name)

    fun localName(uri: String): String? =
        uri.takeIf { it.startsWith(LOCAL_PREFIX) }
            ?.removePrefix(LOCAL_PREFIX)
            ?.let(::unescaped)
            ?.takeIf(::staysInTheFolder)

    private fun staysInTheFolder(name: String): Boolean =
        name.isNotEmpty() && '/' !in name && '\u0000' !in name && name !in DOT_NAMES

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

    fun safeName(wanted: String?, unnamed: String): String =
        wanted
            .orEmpty()
            .substringAfterLast('/')
            .map { if (it in "\\?:*\"<>|" || it.isISOControl()) '_' else it }
            .joinToString("")
            .trim()
            .takeUnless { it.isEmpty() || it in DOT_NAMES } ?: unnamed

    fun nameAmong(wanted: String, taken: Set<String>): String =
        if (wanted !in taken) wanted else numbered(wanted).first { it !in taken }

    fun copiesOf(wanted: String, taken: Set<String>): List<String> =
        listOf(wanted).filter { it in taken } + numbered(wanted).take(COPIES_CHECKED).filter { it in taken }

    private fun numbered(wanted: String): Sequence<String> {
        val stem = wanted.substringBeforeLast('.', wanted)
        val extension = wanted.substring(stem.length)
        return generateSequence(2) { it + 1 }.map { "$stem ($it)$extension" }
    }
}

object OpenedSources {
    private const val CONTENT = "content"

    fun mayOpen(scheme: String?, authority: String?, ownAuthorities: Set<String>): Boolean {
        val provider = authority?.substringAfterLast('@')?.lowercase().orEmpty()
        return scheme.equals(CONTENT, ignoreCase = true) &&
            provider.isNotEmpty() &&
            ownAuthorities.none { it.lowercase() == provider }
    }

    fun authoritiesOf(declared: List<String?>): Set<String> =
        declared.filterNotNull().flatMap { it.split(';') }.filter { it.isNotEmpty() }.toSet()
}

object OpenedCopying {
    const val BYTES_AT_MOST = 4L shl 30
    const val LEFT_FREE_AT_LEAST = 1L shl 30
    private const val CHUNK_BYTES = 64 * 1024

    fun ceiling(usableBytes: Long): Long =
        minOf(BYTES_AT_MOST, usableBytes - LEFT_FREE_AT_LEAST).coerceAtLeast(0)

    fun copiedWhole(input: InputStream, output: OutputStream, atMost: Long): Boolean {
        val chunk = ByteArray(CHUNK_BYTES)
        var copied = 0L
        while (true) {
            val read = input.read(chunk)
            if (read < 0) return true
            if (copied + read > atMost) return false
            output.write(chunk, 0, read)
            copied += read
        }
    }

    fun sameBytes(first: File, second: File): Boolean {
        if (first.length() != second.length()) return false
        val ours = ByteArray(CHUNK_BYTES)
        val theirs = ByteArray(CHUNK_BYTES)
        first.inputStream().use { one ->
            second.inputStream().use { other ->
                while (true) {
                    val read = one.readNBytes(ours, 0, CHUNK_BYTES)
                    if (read == 0) return true
                    if (other.readNBytes(theirs, 0, CHUNK_BYTES) != read) return false
                    if (!Arrays.equals(ours, 0, read, theirs, 0, read)) return false
                }
            }
        }
    }
}

class OpenedFiles(private val context: Context, private val library: LibraryRepository) {
    private val folder
        get() = File(context.filesDir, OpenedNames.FOLDER)

    private val placing = Mutex()

    private val ownAuthorities: Set<String> by lazy {
        OpenedSources.authoritiesOf(
            context.packageManager
                .getPackageInfo(
                    context.packageName,
                    PackageManager.PackageInfoFlags.of(
                        (PackageManager.GET_PROVIDERS or PackageManager.MATCH_DISABLED_COMPONENTS)
                            .toLong()
                    ),
                )
                .providers
                .orEmpty()
                .map { it.authority }
        )
    }

    fun mayOpen(opened: Uri): Boolean =
        OpenedSources.mayOpen(opened.scheme, opened.authority, ownAuthorities)

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
            if (!mayOpen(opened)) throw IOException(context.getString(R.string.opened_unreadable))
            inGrantedTree(opened)?.let { (uri, name) ->
                return@withContext library.holding { it.indexDocument(uri, name) }
            }
            val kept = copied(opened)
            try {
                library.holding { it.indexDocument(kept.uri, kept.name) }
            } catch (failure: Exception) {
                if (kept.fresh) kept.file.delete()
                throw failure
            }
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

    private class Copy(val file: File, val name: String, val fresh: Boolean) {
        val uri: String
            get() = OpenedNames.localUri(file.name)
    }

    private suspend fun copied(opened: Uri): Copy {
        val name = displayName(opened)
        val staged = staged(opened)
        try {
            return placing.withLock { placed(staged, name) }
        } finally {
            staged.delete()
        }
    }

    private fun staged(opened: Uri): File {
        val staging = File(context.cacheDir, STAGING).apply { mkdirs() }
        val staged = File.createTempFile(STAGED_PREFIX, null, staging)
        try {
            val storage = context.getSystemService(StorageManager::class.java)
            val atMost =
                OpenedCopying.ceiling(storage.getAllocatableBytes(storage.getUuidForPath(staging)))
            val whole =
                context.contentResolver.openInputStream(opened)?.use { input ->
                    staged.outputStream().use { OpenedCopying.copiedWhole(input, it, atMost) }
                } ?: throw IOException(context.getString(R.string.opened_unreadable))
            if (!whole) throw IOException(context.getString(R.string.opened_too_large))
        } catch (failure: Exception) {
            staged.delete()
            throw failure
        }
        return staged
    }

    private fun placed(staged: File, name: String): Copy {
        folder.mkdirs()
        val taken = folder.list().orEmpty().toSet()
        OpenedNames.copiesOf(name, taken)
            .map { File(folder, it) }
            .firstOrNull { OpenedCopying.sameBytes(it, staged) }
            ?.let {
                return Copy(it, name, fresh = false)
            }
        val destination = File(folder, OpenedNames.nameAmong(name, taken))
        Files.move(staged.toPath(), destination.toPath())
        return Copy(destination, name, fresh = true)
    }

    private fun displayName(opened: Uri): String {
        val asked =
            context.contentResolver
                .query(opened, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { if (it.moveToFirst()) it.getString(0) else null }
        return OpenedNames.safeName(
            asked ?: opened.lastPathSegment,
            context.getString(R.string.opened_unnamed),
        )
    }

    private companion object {
        const val STAGING = "opening"
        const val STAGED_PREFIX = "opened"
    }
}
