package org.resonate.android

import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.MediaStore

object MusicDocuments {
    val downloads: Uri = MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)

    fun downloadedAudio(context: Context): List<ScannedAudio> {
        val collection = MediaStore.Files.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val columns =
            arrayOf(
                MediaStore.MediaColumns._ID,
                MediaStore.MediaColumns.DISPLAY_NAME,
                MediaStore.MediaColumns.SIZE,
                MediaStore.MediaColumns.DATE_MODIFIED,
            )
        val selection =
            "${MediaStore.Files.FileColumns.MEDIA_TYPE} = ? AND " +
                "${MediaStore.MediaColumns.RELATIVE_PATH} LIKE ? AND " +
                "${MediaStore.MediaColumns.IS_PENDING} = 0 AND " +
                "${MediaStore.MediaColumns.IS_TRASHED} = 0"
        val arguments =
            arrayOf(
                MediaStore.Files.FileColumns.MEDIA_TYPE_AUDIO.toString(),
                "${Environment.DIRECTORY_DOWNLOADS}/%",
            )
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

    fun create(context: Context, folder: Uri, mime: String, name: String): Uri {
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

    fun publish(context: Context, uri: Uri) {
        if (uri.authority != MediaStore.AUTHORITY) return
        val values = ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }
        check(context.contentResolver.update(uri, values, null, null) == 1) {
            "The downloaded song could not be made available"
        }
    }

    fun delete(context: Context, uri: Uri) {
        if (uri.authority == MediaStore.AUTHORITY) {
            context.contentResolver.delete(uri, null, null)
        } else {
            DocumentsContract.deleteDocument(context.contentResolver, uri)
        }
    }
}
