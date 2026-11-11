package org.resonate.android

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.database.Cursor
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Binder
import android.os.ParcelFileDescriptor
import android.os.Process
import java.io.FileNotFoundException
import java.io.IOException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

object ArtworkUris {
    private const val SCHEME = "content"
    private const val TRACK = "track"
    private const val AUTHORITY_SUFFIX = ".art"

    fun authority(packageName: String): String = packageName + AUTHORITY_SUFFIX

    fun uri(authority: String, track: ULong): String = "$SCHEME://$authority/$TRACK/$track"

    fun trackOf(authority: String, uri: String): ULong? =
        uri.removePrefix("$SCHEME://$authority/$TRACK/")
            .takeIf { it.length < uri.length }
            ?.toULongOrNull()
}

object ArtworkReaders {
    const val MEDIA_CONTENT_CONTROL = "android.permission.MEDIA_CONTENT_CONTROL"

    fun mayRead(
        ownApp: Boolean,
        packagesOfCaller: Collection<String>,
        callerControlsMedia: Boolean,
    ): Boolean =
        ownApp || callerControlsMedia || packagesOfCaller.any { it in BROWSING_HOSTS }
}

class ArtworkProvider : ContentProvider() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onCreate(): Boolean = true

    override fun getType(uri: Uri): String = JPEG

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        val context = context ?: throw FileNotFoundException(uri.toString())
        val track =
            ArtworkUris.trackOf(ArtworkUris.authority(context.packageName), uri.toString())
                ?: throw FileNotFoundException(uri.toString())
        if (mode != READ_ONLY || !callerMayRead(context)) {
            throw FileNotFoundException(uri.toString())
        }
        val library = (context.applicationContext as ResonateApplication).library
        val (read, write) = ParcelFileDescriptor.createReliablePipe().let { it[0] to it[1] }
        scope.launch {
            val jpeg = library.features.artworkJpeg(track, ArtworkSizes.BROWSE_EDGE)
            try {
                if (jpeg == null) write.closeWithError(NO_COVER)
                else ParcelFileDescriptor.AutoCloseOutputStream(write).use { it.write(jpeg) }
            } catch (_: IOException) {
                write.close()
            }
        }
        return read
    }

    private fun callerMayRead(context: Context): Boolean {
        val uid = Binder.getCallingUid()
        return ArtworkReaders.mayRead(
            ownApp = uid == Process.myUid(),
            packagesOfCaller = context.packageManager.getPackagesForUid(uid)?.toList().orEmpty(),
            callerControlsMedia =
                context.checkCallingPermission(ArtworkReaders.MEDIA_CONTENT_CONTROL) ==
                    PackageManager.PERMISSION_GRANTED,
        )
    }

    override fun query(
        uri: Uri,
        projection: Array<String>?,
        selection: String?,
        selectionArgs: Array<String>?,
        sortOrder: String?,
    ): Cursor? = null

    override fun insert(uri: Uri, values: ContentValues?): Uri? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<String>?): Int = 0

    override fun update(
        uri: Uri,
        values: ContentValues?,
        selection: String?,
        selectionArgs: Array<String>?,
    ): Int = 0

    private companion object {
        const val JPEG = "image/jpeg"
        const val READ_ONLY = "r"
        const val NO_COVER = "uncovered"
    }
}
