package org.resonate.android

import android.content.ComponentName
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.net.Uri
import android.provider.MediaStore
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.Transport

@androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])
@RunWith(AndroidJUnit4::class)
class PlaybackLifecycleTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context: Context = instrumentation.targetContext
    private val library
        get() = (context.applicationContext as ResonateApplication).library

    private val uris = ArrayList<Uri>()
    private var controller: MediaController? = null
    private lateinit var songs: List<Track>

    @Before
    fun connectWithThreeSongsQueued() {
        connect()
        songs = listOf("a", "b", "c").map { indexed("Resonate lifecycle $it.wav") }
        onMain { controller().setMediaItems(songs.map { it.mediaItem() }, 0, 0) }
        eventually { library.queue.value.map { it.id } == songs.map { it.id } }
    }

    @After
    fun leaveNothingRunning() {
        releaseController()
        context.stopService(Intent(context, PlaybackService::class.java))
        eventually { library.retained == 0 }
        uris.forEach { context.contentResolver.delete(it, null, null) }
    }

    @Test
    fun aTransientFocusLossPausesAndRegainingFocusResumesWhatWasPlaying() {
        startPlaying()
        val thief = focusRequest(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)

        assertGranted(audio().requestAudioFocus(thief))
        eventually { transport() == Transport.PAUSED }

        audio().abandonAudioFocusRequest(thief)
        eventually { transport() == Transport.PLAYING }
    }

    @Test
    fun aPermanentFocusLossPausesAndRegainingFocusDoesNotResume() {
        startPlaying()
        val thief = focusRequest(AudioManager.AUDIOFOCUS_GAIN)

        assertGranted(audio().requestAudioFocus(thief))
        eventually { transport() == Transport.PAUSED }

        audio().abandonAudioFocusRequest(thief)
        Thread.sleep(SETTLED_QUIET_MS)
        assertEquals(Transport.PAUSED, transport())
    }

    @Test
    fun aTransientLossWhileAlreadyPausedResumesNothing() {
        startPlaying()
        onMain { controller().pause() }
        eventually { transport() == Transport.PAUSED }
        val thief = focusRequest(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)

        assertGranted(audio().requestAudioFocus(thief))
        audio().abandonAudioFocusRequest(thief)

        Thread.sleep(SETTLED_QUIET_MS)
        assertEquals(Transport.PAUSED, transport())
    }

    @Test
    fun aPauseTheListenerMakesDuringATransientLossIsNotUndoneByRegainingFocus() {
        startPlaying()
        val thief = focusRequest(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
        assertGranted(audio().requestAudioFocus(thief))
        eventually { transport() == Transport.PAUSED }

        onMain { controller().pause() }
        audio().abandonAudioFocusRequest(thief)

        Thread.sleep(SETTLED_QUIET_MS)
        assertEquals(Transport.PAUSED, transport())
    }

    @Test
    fun theOutputBecomingNoisyPausesAndNothingResumesIt() {
        library.retain()
        val adapter = onMainResult { RustMediaPlayer(context, library) }
        try {
            onMain { adapter.play() }
            eventually { transport() == Transport.PLAYING }
            val thief = focusRequest(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
            assertGranted(audio().requestAudioFocus(thief))
            eventually { transport() == Transport.PAUSED }

            onMain { adapter.outputBecameNoisy() }
            audio().abandonAudioFocusRequest(thief)

            Thread.sleep(SETTLED_QUIET_MS)
            assertEquals(Transport.PAUSED, transport())
        } finally {
            onMain { adapter.release() }
        }
    }

    @Test
    fun aControllersQueueEditsReachTheEnginesQueue() {
        val extra = indexed("Resonate lifecycle d.wav")
        val a = songs[0].id
        val b = songs[1].id
        val c = songs[2].id

        onMain { controller().addMediaItems(1, listOf(extra.mediaItem())) }
        eventually { queued() == listOf(a, extra.id, b, c) }

        onMain { controller().moveMediaItem(0, 3) }
        eventually { queued() == listOf(extra.id, b, c, a) }

        onMain { controller().removeMediaItem(2) }
        eventually { queued() == listOf(extra.id, b, a) }

        onMain { controller().addMediaItems(listOf(songs[2].mediaItem())) }
        eventually { queued() == listOf(extra.id, b, a, c) }

        onMain { controller().removeMediaItems(0, 2) }
        eventually { queued() == listOf(a, c) }
    }

    @Test
    fun destroyingTheServiceGivesTheRepositoryBackAndAnotherOpensItAgain() {
        startPlaying()
        library.request { it.save() }
        assertTrue(library.retained > 0)

        releaseController()
        context.stopService(Intent(context, PlaybackService::class.java))
        eventually { library.retained == 0 }

        eventually { library.snapshot.value == null }

        connect()
        eventually { library.retained > 0 && library.snapshot.value?.current != null }
        assertEquals(Transport.PAUSED, transport())
        eventually { library.queue.value.map { it.id } == songs.map { it.id } }
    }

    @Test
    fun aPollTickThatThrowsIsSaidOnceAndPollingGoesOn() {
        val native = runBlocking { withTimeout(OPENED_WITHIN_MS) { library.native() } }
        library.dismissStatus()

        native.shutdown()

        eventually { library.status.value != null }
        val said = library.status.value
        library.dismissStatus()
        Thread.sleep(SETTLED_QUIET_MS)
        assertNull("The same failure was said again", library.status.value)
        assertTrue(said!!.isNotEmpty())
    }

    private fun indexed(name: String): Track {
        val values =
            ContentValues().apply {
                put(MediaStore.Audio.Media.DISPLAY_NAME, name)
                put(MediaStore.Audio.Media.MIME_TYPE, "audio/wav")
                put(MediaStore.Audio.Media.RELATIVE_PATH, "Music/ResonateTests")
            }
        val uri =
            requireNotNull(
                context.contentResolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values)
            )
        uris.add(uri)
        requireNotNull(context.contentResolver.openOutputStream(uri)).use {
            it.write(pcmTone(TONE_SECONDS))
        }
        val track = CompletableDeferred<Track>()
        library.request { track.complete(it.indexDocument(uri.toString(), name)) }
        return runBlocking { withTimeout(OPENED_WITHIN_MS) { track.await() } }
    }

    private fun connect() {
        context.startService(Intent(context, PlaybackService::class.java))
        val pending =
            MediaController.Builder(
                    context,
                    SessionToken(context, ComponentName(context, PlaybackService::class.java)),
                )
                .buildAsync()
        controller = pending.get(15, TimeUnit.SECONDS)
    }

    private fun releaseController() {
        controller?.let { held -> instrumentation.runOnMainSync { held.release() } }
        controller = null
    }

    private fun controller() = checkNotNull(controller) { "No controller is connected" }

    private fun startPlaying() {
        onMain {
            controller().prepare()
            controller().play()
        }
        eventually { transport() == Transport.PLAYING }
    }

    private fun onMain(action: () -> Unit) = instrumentation.runOnMainSync(action)

    private fun <T> onMainResult(action: () -> T): T {
        var result: T? = null
        instrumentation.runOnMainSync { result = action() }
        @Suppress("UNCHECKED_CAST") return result as T
    }

    private fun audio() = context.getSystemService(AudioManager::class.java)

    private fun transport() = library.snapshot.value?.transport

    private fun queued() = library.queue.value.map { it.id }

    private fun focusRequest(gain: Int) =
        AudioFocusRequest.Builder(gain)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_MEDIA)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build()
            )
            .setOnAudioFocusChangeListener {}
            .build()

    private fun assertGranted(result: Int) =
        assertEquals(AudioManager.AUDIOFOCUS_REQUEST_GRANTED, result)

    private fun eventually(condition: () -> Boolean) {
        val deadline = System.nanoTime() + SETTLED_WITHIN_NS
        while (!condition()) {
            if (System.nanoTime() > deadline)
                throw AssertionError("Playback lifecycle state did not settle")
            Thread.sleep(50)
        }
    }

    private companion object {
        const val TONE_SECONDS = 60
        const val SETTLED_WITHIN_NS = 15_000_000_000L
        const val SETTLED_QUIET_MS = 1_500L
        const val OPENED_WITHIN_MS = 15_000L
    }
}
