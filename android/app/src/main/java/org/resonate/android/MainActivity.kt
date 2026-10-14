package org.resonate.android

import android.Manifest
import android.content.ComponentName
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.core.content.ContextCompat
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken

@androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])
class MainActivity : ComponentActivity() {
    private var controller by mutableStateOf<MediaController?>(null)
    private val library
        get() = (application as ResonateApplication).library

    private val folder =
        registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
            uri?.let {
                try {
                    contentResolver.takePersistableUriPermission(
                        it,
                        Intent.FLAG_GRANT_READ_URI_PERMISSION or
                            Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
                    )
                    library.scan(it)
                } catch (failure: SecurityException) {
                    try {
                        contentResolver.takePersistableUriPermission(
                            it,
                            Intent.FLAG_GRANT_READ_URI_PERMISSION,
                        )
                    } catch (denied: SecurityException) {
                        android.util.Log.w("Resonate", "Folder grant cannot be persisted", denied)
                    }
                    library.scan(it)
                }
            }
        }
    private val notifications =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
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
        setContent {
            ResonateScreen(
                library,
                controller,
                chooseFolder = { folder.launch(null) },
                allowNotifications = {
                    notifications.launch(Manifest.permission.POST_NOTIFICATIONS)
                },
            )
        }
    }

    override fun onDestroy() {
        controller?.release()
        controller = null
        super.onDestroy()
    }
}
