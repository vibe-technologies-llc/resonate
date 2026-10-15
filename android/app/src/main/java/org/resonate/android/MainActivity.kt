package org.resonate.android

import android.Manifest
import android.content.ComponentName
import android.content.Intent
import android.os.Bundle
import androidx.core.content.ContextCompat
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine

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
                if (!isDestroyed) controller = pending.get()
                else MediaController.releaseFuture(pending)
            },
            ContextCompat.getMainExecutor(this),
        )
    }

    fun chooseFolder() {
        startActivityForResult(
            Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
                .addFlags(
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or
                        Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                        Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
                ),
            FOLDER_REQUEST,
        )
    }

    fun allowNotifications() {
        requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), NOTIFICATION_REQUEST)
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != FOLDER_REQUEST || resultCode != RESULT_OK) return
        val uri = data?.data ?: return
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
    }
}
