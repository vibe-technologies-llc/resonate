package org.resonate.android

import android.Manifest
import android.app.SearchManager
import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
import android.os.Environment
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.ActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.lifecycle.lifecycleScope
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import java.util.concurrent.CancellationException
import java.util.concurrent.ExecutionException
import kotlinx.coroutines.launch
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.Host
import org.resonate.android.ui.Playback
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.ResonateApp

@androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])
class MainActivity : ComponentActivity(), Host {
    private var controller: MediaController? = null
    private var waitingToPlay: (suspend () -> List<Track>)? = null
    private val playback by lazy { Playback(library) { controller } }
    private val library
        get() = (application as ResonateApplication).library

    private val folderChooser =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult(), ::folderChosen)
    private val roomChooser =
        registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
            result.data?.data?.takeIf { result.resultCode == RESULT_OK }?.let {
                library.features.room.choose(it)
            }
        }
    private val audioPermission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            if (granted) library.features.useDownloadsAndScan()
            else library.notice("Allow music access to read existing songs in Downloads")
        }
    private val notificationPermission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
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
        val resonate = Resonate(library, playback, this)
        setContent { ResonateApp(resonate) }
        if (savedInstanceState == null) opening(intent)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        opening(intent)
    }

    private fun opening(intent: Intent) {
        when (intent.action) {
            Intent.ACTION_VIEW -> {
                val song = intent.data ?: return
                waitingToPlay = { listOf(library.openedFiles.track(song)) }
            }
            MediaStore.INTENT_ACTION_MEDIA_PLAY_FROM_SEARCH -> {
                val asked = intent.getStringExtra(SearchManager.QUERY).orEmpty()
                waitingToPlay = { library.browse.found(asked) }
            }
            else -> return
        }
        playWhatWasOpened()
    }

    private fun playWhatWasOpened() {
        if (controller == null) return
        val tracks = waitingToPlay ?: return
        waitingToPlay = null
        lifecycleScope.launch {
            try {
                val found = tracks()
                if (found.isEmpty()) library.notice("Nothing in your library matches")
                playback.play(found)
                library.refresh()
            } catch (cancelled: kotlin.coroutines.cancellation.CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
    }

    private fun connected(pending: ListenableFuture<MediaController>) {
        try {
            controller = pending.get()
            playWhatWasOpened()
        } catch (failure: ExecutionException) {
            Log.w("Resonate", "Playback controls could not connect", failure)
            library.notice("Playback controls could not connect")
        } catch (cancelled: CancellationException) {
            Log.w("Resonate", "Playback controls connection was cancelled", cancelled)
        }
    }

    override fun chooseFolder() {
        folderChooser.launch(
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
                )
        )
    }

    override fun chooseRoomResponse() {
        roomChooser.launch(
            Intent(Intent.ACTION_OPEN_DOCUMENT)
                .addCategory(Intent.CATEGORY_OPENABLE)
                .setType("audio/*")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        )
    }

    override fun share(title: String, link: String) {
        val send =
            Intent(Intent.ACTION_SEND)
                .setType("text/plain")
                .putExtra(Intent.EXTRA_SUBJECT, title)
                .putExtra(Intent.EXTRA_TEXT, link)
        startActivity(Intent.createChooser(send, title))
    }

    override fun open(link: Uri) {
        if (link.scheme == "https" || link.scheme == "http") {
            startActivity(Intent(Intent.ACTION_VIEW, link))
        }
    }

    override fun allowNotifications() {
        notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
    }

    override fun useDownloads() {
        if (
            checkSelfPermission(Manifest.permission.READ_MEDIA_AUDIO) ==
                PackageManager.PERMISSION_GRANTED
        ) {
            library.features.useDownloadsAndScan()
        } else {
            audioPermission.launch(Manifest.permission.READ_MEDIA_AUDIO)
        }
    }

    private fun folderChosen(result: ActivityResult) {
        val data = result.data ?: return
        val uri = data.data?.takeIf { result.resultCode == RESULT_OK } ?: return
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
        library.saveNow()
        super.onStop()
    }

    override fun onDestroy() {
        controller?.release()
        controller = null
        super.onDestroy()
    }
}
