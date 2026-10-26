package org.resonate.android

import android.Manifest
import android.app.PendingIntent
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
import androidx.activity.result.IntentSenderRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.lifecycle.lifecycleScope
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import com.google.common.util.concurrent.ListenableFuture
import java.util.concurrent.CancellationException
import java.util.concurrent.ExecutionException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.Host
import org.resonate.android.ui.ResonateApp

private const val PAIRING_SCHEME = "resonate"

@androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])
class MainActivity : ComponentActivity(), Host {
    private var controller: MediaController? = null
    private var connecting: ListenableFuture<MediaController>? = null
    private var waitingToPlay: (suspend () -> List<Track>)? = null
    private val asking = Mutex()
    private val resonateApplication
        get() = application as ResonateApplication

    private val library
        get() = resonateApplication.library

    private val playback
        get() = resonateApplication.resonate.playback

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
    private var consentAnswer: CompletableDeferred<Boolean>? = null
    private val mediaConsent =
        registerForActivityResult(ActivityResultContracts.StartIntentSenderForResult()) { result ->
            consentAnswer?.complete(result.resultCode == RESULT_OK)
            consentAnswer = null
        }
    private var microphoneAnswer: CompletableDeferred<Boolean>? = null
    private val microphonePermission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            microphoneAnswer?.complete(granted)
            microphoneAnswer = null
        }
    private val notificationPermission =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        resonateApplication.current.attach(this)
        startService(Intent(this, PlaybackService::class.java))
        connect()

        val resonate = resonateApplication.resonate
        setContent { ResonateApp(resonate) }
        if (savedInstanceState == null) opening(intent)
    }

    private fun connect() {
        if (controller != null || connecting != null) return
        val pending =
            MediaController.Builder(
                    this,
                    SessionToken(this, ComponentName(this, PlaybackService::class.java)),
                )
                .setListener(
                    object : MediaController.Listener {
                        override fun onDisconnected(lost: MediaController) {
                            disconnected(lost)
                        }
                    }
                )
                .buildAsync()
        connecting = pending
        pending.addListener(
            {
                if (connecting === pending) connecting = null
                if (isDestroyed) MediaController.releaseFuture(pending) else connected(pending)
            },
            ContextCompat.getMainExecutor(this),
        )
    }

    private fun disconnected(lost: MediaController) {
        if (controller !== lost) return
        resonateApplication.current.disconnected(lost)
        lost.release()
        controller = null
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        opening(intent)
    }

    private fun opening(intent: Intent) {
        when (intent.action) {
            Intent.ACTION_VIEW -> {
                val opened = intent.data ?: return
                if (opened.scheme == PAIRING_SCHEME) {
                    library.features.offerPairing(opened.toString())
                    return
                }
                waitingToPlay = { listOf(library.openedFiles.track(opened)) }
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
            val connection = pending.get()
            controller = connection
            resonateApplication.current.connected(connection)
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

    override suspend fun confirmDelete(uris: List<Uri>): Boolean =
        consented(MediaStore.createDeleteRequest(contentResolver, uris))

    override suspend fun confirmWrite(uris: List<Uri>): Boolean =
        consented(MediaStore.createWriteRequest(contentResolver, uris))

    private suspend fun consented(request: PendingIntent): Boolean =
        asking.withLock {
            val answer = CompletableDeferred<Boolean>()
            val asked = IntentSenderRequest.Builder(request.intentSender).build()

            withContext(Dispatchers.Main) {
                if (isDestroyed) {
                    answer.complete(false)
                } else {
                    consentAnswer = answer
                    mediaConsent.launch(asked)
                }
            }
            answer.await()
        }

    override suspend fun allowMicrophone(): Boolean {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED)
            return true
        return asking.withLock {
            val answer = CompletableDeferred<Boolean>()

            withContext(Dispatchers.Main) {
                if (isDestroyed) {
                    answer.complete(false)
                } else {
                    microphoneAnswer = answer
                    microphonePermission.launch(Manifest.permission.RECORD_AUDIO)
                }
            }
            answer.await()
        }
    }

    override fun openAppSettings() {
        startActivity(
            Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
                .setData(Uri.fromParts("package", packageName, null))
        )
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
        if (overlapsGrantedFolder(uri)) {
            library.report(
                IllegalArgumentException(
                    "${uri.lastPathSegment.orEmpty().substringAfterLast('/').substringAfter(':')} " +
                        "holds or sits inside a folder already in your library"
                )
            )
            return
        }
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

    private fun overlapsGrantedFolder(chosen: Uri): Boolean {
        if (!DocumentsContract.isTreeUri(chosen)) return false
        val chosenId = DocumentsContract.getTreeDocumentId(chosen)
        return contentResolver.persistedUriPermissions
            .filter { it.isReadPermission && it.uri.authority == chosen.authority }
            .filter { DocumentsContract.isTreeUri(it.uri) }
            .any { OpenedNames.treesOverlap(chosenId, DocumentsContract.getTreeDocumentId(it.uri)) }
    }

    override fun onStart() {
        super.onStart()
        connect()
        library.features.foreground(true)
    }

    override fun onStop() {
        library.features.foreground(false)
        library.saveNow()
        super.onStop()
    }

    override fun onDestroy() {
        resonateApplication.current.detach(this)
        consentAnswer?.complete(false)
        consentAnswer = null
        microphoneAnswer?.complete(false)
        microphoneAnswer = null
        connecting?.let(MediaController::releaseFuture)
        connecting = null
        controller?.let {
            resonateApplication.current.disconnected(it)
            it.release()
        }
        controller = null
        super.onDestroy()
    }
}
