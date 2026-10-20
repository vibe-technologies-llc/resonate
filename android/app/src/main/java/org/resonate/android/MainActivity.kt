package org.resonate.android

import android.Manifest
import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.os.Environment
import android.provider.DocumentsContract
import android.util.Log
import androidx.core.content.ContextCompat
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import java.util.concurrent.CancellationException
import java.util.concurrent.ExecutionException

@androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])
class MainActivity : FlutterActivity() {
    private var bridge: FlutterBridge? = null
    private var controller: MediaController? = null
    private val library
        get() = (application as ResonateApplication).library

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        bridge =
            FlutterBridge(this, library, flutterEngine.dartExecutor.binaryMessenger) { controller }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        startService(Intent(this, PlaybackService::class.java))
        val pending =
            MediaController.Builder(
                    this,
                    SessionToken(this, ComponentName(this, PlaybackService::class.java)),
                )
                .buildAsync()
        pending.addListener(
            {
                if (isDestroyed) MediaController.releaseFuture(pending) else connected(pending)
            },
            ContextCompat.getMainExecutor(this),
        )
    }

    private fun connected(pending: ListenableFuture<MediaController>) {
        try {
            controller = pending.get()
        } catch (failure: ExecutionException) {
            Log.w("Resonate", "Playback controls could not connect", failure)
            library.notice("Playback controls could not connect")
        } catch (cancelled: CancellationException) {
            Log.w("Resonate", "Playback controls connection was cancelled", cancelled)
        }
    }

    fun chooseFolder() {
        startActivityForResult(
            Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
                .putExtra(
                    DocumentsContract.EXTRA_INITIAL_URI,
                    DocumentsContract.buildDocumentUri(
                        "com.android.externalstorage.documents",
                        "primary:${Environment.DIRECTORY_MUSIC}",
                    ),
                )
                .addFlags(
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or
                        Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                        Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
                ),
            FOLDER_REQUEST,
        )
    }

    fun chooseRoomResponse() {
        startActivityForResult(
            Intent(Intent.ACTION_OPEN_DOCUMENT)
                .addCategory(Intent.CATEGORY_OPENABLE)
                .setType("audio/*")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION),
            ROOM_REQUEST,
        )
    }

    fun shareLink(title: String, link: String) {
        val send =
            Intent(Intent.ACTION_SEND)
                .setType("text/plain")
                .putExtra(Intent.EXTRA_SUBJECT, title)
                .putExtra(Intent.EXTRA_TEXT, link)
        startActivity(Intent.createChooser(send, title))
    }

    fun allowNotifications() {
        requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), NOTIFICATION_REQUEST)
    }

    fun useDownloads() {
        if (
            checkSelfPermission(Manifest.permission.READ_MEDIA_AUDIO) ==
                PackageManager.PERMISSION_GRANTED
        ) {
            library.features.useDownloads()
            library.scan(MusicDocuments.downloads)
        } else {
            requestPermissions(arrayOf(Manifest.permission.READ_MEDIA_AUDIO), AUDIO_REQUEST)
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != AUDIO_REQUEST) return
        if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) {
            library.features.useDownloads()
            library.scan(MusicDocuments.downloads)
        } else {
            library.notice("Allow music access to read existing songs in Downloads")
        }
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        if (requestCode == ROOM_REQUEST) {
            library.features.room.choose(uri)
            return
        }
        if (requestCode != FOLDER_REQUEST) return
        try {
            val writable = data.flags and Intent.FLAG_GRANT_WRITE_URI_PERMISSION != 0
            contentResolver.takePersistableUriPermission(
                uri,
                if (writable)
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                else Intent.FLAG_GRANT_READ_URI_PERMISSION,
            )
            library.scan(uri)
        } catch (failure: SecurityException) {
            library.report(failure)
        }
    }

    override fun onStart() {
        super.onStart()
        library.features.foreground(true)
    }

    override fun onStop() {
        library.features.foreground(false)
        super.onStop()
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        bridge?.close()
        bridge = null
        super.cleanUpFlutterEngine(flutterEngine)
    }

    override fun onDestroy() {
        controller?.release()
        controller = null
        super.onDestroy()
    }

    private companion object {
        const val FOLDER_REQUEST = 41
        const val NOTIFICATION_REQUEST = 42
        const val AUDIO_REQUEST = 43
        const val ROOM_REQUEST = 44
    }
}
