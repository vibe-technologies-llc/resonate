package org.resonate.android

import android.content.ContentResolver
import android.net.Uri
import android.provider.OpenableColumns

class NamedBytes(val name: String, val bytes: ByteArray)

fun ContentResolver.displayNameOf(uri: Uri): String =
    query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
        ?.use { cursor -> if (cursor.moveToFirst()) cursor.getString(0) else null }
        ?: uri.lastPathSegment.orEmpty().substringAfterLast('/')

fun ContentResolver.readUpTo(uri: Uri, bytesAtMost: ULong): NamedBytes {
    val bytes =
        openInputStream(uri)?.use { it.readNBytes((bytesAtMost + 1uL).toInt()) } ?: ByteArray(0)
    return NamedBytes(displayNameOf(uri), bytes)
}
