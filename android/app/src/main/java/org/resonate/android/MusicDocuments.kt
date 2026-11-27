package org.resonate.android

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.util.Log
import android.webkit.MimeTypeMap
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import org.resonate.android.nativecore.StandingDocument

object MusicDocuments {
    const val TREE_DEPTH_AT_MOST = 32
    private const val LIKE = "LIKE"
    private const val AND = "AND"

    val downloads: Uri = MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)

    fun prefix(folder: Uri): String =
        if (folder == downloads) {
            "${MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)}/"
        } else {
            "$folder/document/"
        }

    fun rootDocument(folder: Uri): String =
        DocumentsContract.buildDocumentUriUsingTree(
                folder,
                DocumentsContract.getTreeDocumentId(folder),
            )
            .toString()

    fun downloadedAudio(context: Context): List<ScannedAudio> =
        downloadedAudioAt(
            context,
            "${MediaStore.MediaColumns.RELATIVE_PATH} $LIKE ?",
            "${Environment.DIRECTORY_DOWNLOADS}/%",
        )

    private fun downloadedAudioAt(
        context: Context,
        within: String,
        path: String,
    ): List<ScannedAudio> {
        val collection = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val columns =
            arrayOf(
                MediaStore.MediaColumns._ID,
                MediaStore.MediaColumns.DISPLAY_NAME,
                MediaStore.MediaColumns.SIZE,
                MediaStore.MediaColumns.DATE_MODIFIED,
            )
        val selection =
            listOf(
                    "${MediaStore.Files.FileColumns.MEDIA_TYPE} = ?",
                    within,
                    "${MediaStore.MediaColumns.IS_PENDING} = 0",
                    "${MediaStore.MediaColumns.IS_TRASHED} = 0",
                )
                .joinToString(" $AND ")
        val arguments =
            arrayOf(MediaStore.Files.FileColumns.MEDIA_TYPE_AUDIO.toString(), path)
        return buildList {
            context.contentResolver.query(collection, columns, selection, arguments, null)?.use {
                cursor ->
                while (cursor.moveToNext()) {
                    val name = cursor.getString(1) ?: continue
                    add(
                        ScannedAudio(
                            ContentUris.withAppendedId(collection, cursor.getLong(0)),
                            name,
                            if (cursor.isNull(2)) 0L else cursor.getLong(2),
                            if (cursor.isNull(3)) 0L else cursor.getLong(3) * 1000L,
                        )
                    )
                }
            }
        }
    }

    suspend fun listed(context: Context, folder: Uri): List<String> {
        if (folder == downloads) return downloadedAudio(context).map { it.uri.toString() }
        val pending = ArrayDeque<Pair<String, Int>>()
        pending.add(DocumentsContract.getTreeDocumentId(folder) to 0)
        return buildList {
            while (pending.isNotEmpty()) {
                currentCoroutineContext().ensureActive()
                val (parent, depth) = pending.removeFirst()
                val children = DocumentsContract.buildChildDocumentsUriUsingTree(folder, parent)
                val columns =
                    arrayOf(
                        DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                        DocumentsContract.Document.COLUMN_MIME_TYPE,
                    )
                try {
                    context.contentResolver.query(children, columns, null, null, null)?.use { cursor
                        ->
                        while (cursor.moveToNext()) {
                            val id = cursor.getString(0)
                            if (cursor.getString(1) == DocumentsContract.Document.MIME_TYPE_DIR) {
                                if (depth < TREE_DEPTH_AT_MOST) pending.add(id to depth + 1)
                            } else {
                                add(
                                    DocumentsContract.buildDocumentUriUsingTree(folder, id)
                                        .toString()
                                )
                            }
                        }
                    }
                } catch (failure: Exception) {
                    Log.w("Resonate", "A folder could not be listed", failure)
                }
            }
        }
    }

    fun standing(
        context: Context,
        folder: Uri,
        name: String,
        filed: List<List<String>>,
    ): List<StandingDocument> {
        if (folder != downloads) return StandingCopies(SafTree(context, folder)).standing(name, filed)
        val stem = StandingCopies.composed(name.substringBeforeLast('.'))
        val atTheRoot =
            downloadedAudio(context)
                .filter { StandingCopies.composed(it.name).startsWith(stem) }
                .map { StandingDocument(it.uri.toString(), it.name, emptyList()) }
        return atTheRoot +
            filed.flatMap { within ->
                downloadedAudioAt(
                        context,
                        "${MediaStore.MediaColumns.RELATIVE_PATH} = ?",
                        StandingCopies.relativePath(Environment.DIRECTORY_DOWNLOADS, within),
                    )
                    .map { StandingDocument(it.uri.toString(), it.name, within) }
            }
    }

    fun create(context: Context, folder: Uri, offered: String, name: String): Uri {
        val mime = mimeOfName(name) ?: offered
        if (folder == downloads) {
            val values =
                ContentValues().apply {
                    put(MediaStore.MediaColumns.DISPLAY_NAME, name)
                    put(MediaStore.MediaColumns.MIME_TYPE, mime)
                    put(
                        MediaStore.MediaColumns.RELATIVE_PATH,
                        "${Environment.DIRECTORY_DOWNLOADS}/",
                    )
                    put(MediaStore.MediaColumns.IS_PENDING, 1)
                }
            val created = requireNotNull(context.contentResolver.insert(downloads, values))
            return ContentUris.withAppendedId(
                MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY),
                ContentUris.parseId(created),
            )
        }
        val parent =
            DocumentsContract.buildDocumentUriUsingTree(
                folder,
                DocumentsContract.getTreeDocumentId(folder),
            )
        return requireNotNull(
            DocumentsContract.createDocument(context.contentResolver, parent, mime, name)
        )
    }

    private fun mimeOfName(name: String): String? =
        MimeTypeMap.getSingleton()
            .getMimeTypeFromExtension(name.substringAfterLast('.', "").lowercase())

    fun published(context: Context, uri: Uri): Boolean {
        if (uri.authority != MediaStore.AUTHORITY) return true
        val values = ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }
        return context.contentResolver.update(uri, values, null, null) == 1
    }

    fun publish(context: Context, uri: Uri) {
        check(published(context, uri)) { context.getString(R.string.transfer_not_made_available) }
    }

    fun settleLeftover(context: Context, uri: Uri, stranded: Boolean) {
        if (stranded) delete(context, uri) else publish(context, uri)
    }

    fun delete(context: Context, uri: Uri) {
        if (uri.authority == MediaStore.AUTHORITY) {
            val item = Uri.parse(TagFiles.writeUri(uri.toString()))
            val removed = context.contentResolver.delete(item, null, null)
            check(MediaStoreRemoval.settled(removed) { rowPresence(context, uri) }) {
                context.getString(R.string.deletion_song_not_removed)
            }
        } else {
            check(DocumentsContract.deleteDocument(context.contentResolver, uri)) {
                context.getString(R.string.deletion_song_not_removed)
            }
        }
    }

    fun presence(context: Context, uri: Uri): RowPresence {
        val authority = uri.authority ?: return RowPresence.Unknown
        return when {
            authority == MediaStore.AUTHORITY -> rowPresence(context, uri)
            DocumentsContract.isTreeUri(uri) ->
                DocumentPresence.inTree(
                    SafTree(
                        context,
                        DocumentsContract.buildTreeDocumentUri(
                            authority,
                            DocumentsContract.getTreeDocumentId(uri),
                        ),
                    ),
                    DocumentsContract.getDocumentId(uri),
                )
            else -> RowPresence.Unknown
        }
    }

    private fun rowPresence(context: Context, uri: Uri): RowPresence =
        MediaStoreRemoval.presence {
            context.contentResolver
                .query(uri, arrayOf(MediaStore.MediaColumns._ID), null, null, null)
                ?.use { it.moveToFirst() }
        }
}
