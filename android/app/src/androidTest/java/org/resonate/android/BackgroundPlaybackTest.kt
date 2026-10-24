package org.resonate.android

import android.app.ActivityManager
import android.app.Notification
import android.app.NotificationManager
import android.content.ComponentName
import android.content.ContentValues
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Color
import android.provider.MediaStore
import androidx.media3.common.Player
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.StaleObjectException
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import java.io.ByteArrayOutputStream
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.SleepTimer
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.Transport

@androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])
@RunWith(AndroidJUnit4::class)
class BackgroundPlaybackTest {
    @Test
    fun backgroundPlaybackShowsCoverOpensAppAndStopsWhenTaskIsDismissed() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val resolver = context.contentResolver
        instrumentation.uiAutomation.grantRuntimePermission(
            context.packageName,
            android.Manifest.permission.POST_NOTIFICATIONS,
        )
        val device = UiDevice.getInstance(instrumentation)
        device.pressHome()
        val values =
            ContentValues().apply {
                put(MediaStore.Audio.Media.DISPLAY_NAME, "Resonate playback test.wav")
                put(MediaStore.Audio.Media.MIME_TYPE, "audio/wav")
                put(MediaStore.Audio.Media.RELATIVE_PATH, "Music/ResonateTests")
            }
        val uri =
            requireNotNull(resolver.insert(MediaStore.Audio.Media.EXTERNAL_CONTENT_URI, values))
        try {
            requireNotNull(resolver.openOutputStream(uri)).use { it.write(pcmTone(90)) }
            ActivityScenario.launch(MainActivity::class.java).use {
                val library = (context.applicationContext as ResonateApplication).library
                val pending =
                    MediaController.Builder(
                            context,
                            SessionToken(
                                context,
                                ComponentName(context, PlaybackService::class.java),
                            ),
                        )
                        .buildAsync()
                val controller = pending.get(15, TimeUnit.SECONDS)
                try {
                    val indexed = CompletableDeferred<Track>()
                    library.request { native ->
                        val track =
                            native.indexDocument(uri.toString(), "Resonate playback test.wav")
                        val png =
                            ByteArrayOutputStream().use { output ->
                                val bitmap = Bitmap.createBitmap(16, 16, Bitmap.Config.ARGB_8888)
                                bitmap.eraseColor(Color.BLUE)
                                bitmap.compress(Bitmap.CompressFormat.PNG, 100, output)
                                bitmap.recycle()
                                output.toByteArray()
                            }
                        native.setArtwork(track.id, png)
                        indexed.complete(track)
                    }
                    val track = runBlocking { withTimeout(15000) { indexed.await() } }
                    instrumentation.runOnMainSync {
                        controller.setMediaItem(track.mediaItem())
                        controller.prepare()
                        controller.play()
                    }
                    eventually { library.snapshot.value?.transport == Transport.PLAYING }
                    assertEquals(track.id, library.snapshot.value?.current?.id)
                    eventually {
                        var present = false
                        instrumentation.runOnMainSync {
                            present = controller.mediaMetadata.artworkData?.isNotEmpty() == true
                        }
                        present
                    }
                    eventually {
                        instrumentation.uiAutomation.clearCache()
                        device.hasObject(By.descContains("Song progress"))
                    }
                    assertTrue(device.hasObject(By.desc("Next")))
                    device.pressHome()
                    val position = library.snapshot.value?.positionMs ?: 0uL
                    eventually { (library.snapshot.value?.positionMs ?: 0uL) > position + 500uL }
                    context.sendBroadcast(
                        Intent(context, ResonatePlayerWidgetProvider::class.java)
                            .setAction(ResonateWidgets.TOGGLE)
                    )
                    eventually { library.snapshot.value?.transport == Transport.PAUSED }
                    context.sendBroadcast(
                        Intent(context, ResonatePlayerWidgetProvider::class.java)
                            .setAction(ResonateWidgets.TOGGLE)
                    )
                    eventually { library.snapshot.value?.transport == Transport.PLAYING }
                    assertTrue((library.snapshot.value?.positionMs ?: 0uL) > 0uL)
                    library.request { it.setSleep(SleepTimer.After(60u)) }
                    eventually { library.snapshot.value?.sleep is SleepTimer.After }
                    assertTrue((library.snapshot.value?.sleepRemainingMs ?: 0uL) > 50_000uL)
                    library.request { it.setSleep(null) }
                    eventually { library.snapshot.value?.sleep == null }
                    assertEquals(Transport.PLAYING, library.snapshot.value?.transport)
                    library.request { it.setSleep(SleepTimer.After(2u)) }
                    eventually { library.snapshot.value?.transport == Transport.PAUSED }
                    assertEquals(null, library.snapshot.value?.sleep)
                    assertEquals(track.id, library.snapshot.value?.current?.id)
                    instrumentation.runOnMainSync { controller.play() }
                    eventually { library.snapshot.value?.transport == Transport.PLAYING }
                    val notifications = context.getSystemService(NotificationManager::class.java)
                    eventually {
                        notifications.activeNotifications.any {
                            it.notification.getLargeIcon() != null &&
                                it.notification.contentIntent != null
                        }
                    }
                    assertTrue(device.openNotification())
                    clickNotificationControl(device, By.text(track.title))
                    assertTrue(
                        "Tapping the media notification opens Resonate",
                        device.wait(Until.hasObject(By.descContains("Song progress")), 10000),
                    )
                    for (paused in listOf(false, true)) {
                        assertTrue(device.openNotification())
                        if (paused) {
                            eventually {
                                instrumentation.uiAutomation.clearCache()
                                device.hasObject(By.desc("Close"))
                            }
                            instrumentation.runOnMainSync { controller.pause() }
                            eventually { library.snapshot.value?.transport == Transport.PAUSED }
                        }
                        clickNotificationControl(device, By.desc("Close"))
                        eventually { library.snapshot.value?.transport == Transport.STOPPED }
                        eventually {
                            notifications.activeNotifications.none {
                                it.notification.extras.containsKey(Notification.EXTRA_MEDIA_SESSION)
                            }
                        }
                        eventually {
                            var stopped = false
                            instrumentation.runOnMainSync {
                                stopped =
                                    controller.playbackState == Player.STATE_IDLE &&
                                        !controller.playWhenReady
                            }
                            stopped
                        }
                        Thread.sleep(500)
                        assertTrue(
                            "The closed notification stays dismissed while a controller is bound",
                            notifications.activeNotifications.none {
                                it.notification.extras.containsKey(Notification.EXTRA_MEDIA_SESSION)
                            },
                        )
                        device.pressBack()
                        instrumentation.runOnMainSync {
                            controller.prepare()
                            controller.play()
                        }
                        eventually { library.snapshot.value?.transport == Transport.PLAYING }
                        eventually {
                            notifications.activeNotifications.any {
                                it.notification.extras.containsKey(Notification.EXTRA_MEDIA_SESSION)
                            }
                        }
                    }
                    val tasks = context.getSystemService(ActivityManager::class.java).appTasks
                    assertTrue(tasks.isNotEmpty())
                    instrumentation.runOnMainSync { tasks.forEach { it.finishAndRemoveTask() } }
                    eventually { library.snapshot.value?.transport == Transport.PAUSED }
                    var playing = true
                    eventually {
                        instrumentation.runOnMainSync { playing = controller.playWhenReady }
                        !playing
                    }
                    val pausedPosition = library.snapshot.value?.positionMs
                    Thread.sleep(500)
                    assertEquals(pausedPosition, library.snapshot.value?.positionMs)
                } finally {
                    instrumentation.runOnMainSync {
                        controller.stop()
                        controller.release()
                    }
                }
            }
        } finally {
            resolver.delete(uri, null, null)
        }
    }

    private fun clickNotificationControl(device: UiDevice, selector: BySelector) {
        eventually {
            InstrumentationRegistry.getInstrumentation().uiAutomation.clearCache()
            try {
                val control = device.findObject(selector) ?: return@eventually false
                control.click()
                true
            } catch (stale: StaleObjectException) {
                false
            }
        }
    }

    private fun eventually(condition: () -> Boolean) {
        val deadline = System.nanoTime() + 15_000_000_000L
        while (!condition()) {
            if (System.nanoTime() > deadline)
                throw AssertionError("Background media state did not settle")
            Thread.sleep(50)
        }
    }
}
