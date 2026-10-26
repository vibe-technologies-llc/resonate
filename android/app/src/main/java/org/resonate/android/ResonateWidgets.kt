package org.resonate.android

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RadialGradient
import android.graphics.Shader
import android.os.Bundle
import android.util.Base64
import android.util.Log
import android.widget.RemoteViews
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaController
import androidx.media3.session.SessionToken
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.Transport

@UnstableApi
open class ResonatePlayerWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
        ResonateWidgets.refresh(context, goAsync())
    }

    override fun onAppWidgetOptionsChanged(
        context: Context,
        manager: AppWidgetManager,
        id: Int,
        options: Bundle,
    ) {
        ResonateWidgets.refresh(context, goAsync())
    }

    override fun onReceive(context: Context, intent: Intent) {
        super.onReceive(context, intent)
        if (intent.action in ResonateWidgets.actions) {
            ResonateWidgets.control(
                context.applicationContext,
                requireNotNull(intent.action),
                goAsync(),
            )
        }
    }
}

@UnstableApi class ResonateArtworkWidgetProvider : ResonatePlayerWidgetProvider()

@UnstableApi
object ResonateWidgets {
    const val TOGGLE = "org.resonate.android.widget.TOGGLE"
    const val PREVIOUS = "org.resonate.android.widget.PREVIOUS"
    const val NEXT = "org.resonate.android.widget.NEXT"
    val actions = setOf(TOGGLE, PREVIOUS, NEXT)
    private const val ARTWORK_EDGE_AT_MOST = 512
    private val scope =
        CoroutineScope(
            SupervisorJob() +
                Dispatchers.Main.immediate +
                CoroutineExceptionHandler { _, failure ->
                    Log.w("ResonateWidgets", "A widget update failed", failure)
                }
        )
    private val io = Dispatchers.IO.limitedParallelism(1)
    @Volatile private var latest: WidgetSong? = null
    private var publishing: Job? = null

    private data class WidgetSong(
        val title: String,
        val artist: String,
        val album: String,
        val playing: Boolean,
        val artwork: ByteArray?,
    )

    fun publish(context: Context, snapshot: Snapshot?, artwork: ByteArray?) {
        val track = snapshot?.current
        val song =
            WidgetSong(
                track?.title ?: "Resonate",
                track?.artist.orEmpty().ifBlank { "Your music, everywhere" },
                track?.album.orEmpty(),
                snapshot?.transport == Transport.PLAYING ||
                    snapshot?.transport == Transport.LOADING,
                artwork,
            )
        if (song == latest) return
        latest = song
        publishing?.cancel()
        publishing = scope.launch {
            withContext(io) {
                val preferences = context.getSharedPreferences("widgets", Context.MODE_PRIVATE)
                preferences
                    .edit()
                    .putString("title", song.title)
                    .putString("artist", song.artist)
                    .putString("album", song.album)
                    .putString(
                        "artwork",
                        song.artwork?.let { Base64.encodeToString(it, Base64.NO_WRAP) },
                    )
                    .apply()
                ensureActive()
                render(context, song)
            }
        }
    }

    fun stopped(context: Context) {
        val song = latest ?: return
        latest = song.copy(playing = false)
        publishing?.cancel()
        publishing = scope.launch(io) { render(context, song.copy(playing = false)) }
    }

    fun refresh(context: Context, result: BroadcastReceiver.PendingResult) {
        scope.launch {
            try {
                withContext(io) {
                    val preferences = context.getSharedPreferences("widgets", Context.MODE_PRIVATE)
                    val song =
                        latest
                            ?: WidgetSong(
                                preferences.getString("title", "Resonate") ?: "Resonate",
                                preferences.getString("artist", "Your music, everywhere").orEmpty(),
                                preferences.getString("album", "").orEmpty(),
                                false,
                                preferences.getString("artwork", null)?.let(::storedArtwork),
                            )
                    render(context, song)
                }
            } finally {
                result.finish()
            }
        }
    }

    private fun storedArtwork(encoded: String): ByteArray? =
        try {
            Base64.decode(encoded, Base64.NO_WRAP)
        } catch (failure: IllegalArgumentException) {
            Log.w("ResonateWidgets", "The kept artwork could not be read", failure)
            null
        }

    fun control(context: Context, action: String, result: BroadcastReceiver.PendingResult) {
        scope.launch {
            val connection =
                MediaController.Builder(
                        context,
                        SessionToken(context, ComponentName(context, PlaybackService::class.java)),
                    )
                    .buildAsync()
            try {
                val controller = withContext(Dispatchers.IO) { connection.get(8, TimeUnit.SECONDS) }
                when (action) {
                    TOGGLE ->
                        if (controller.playWhenReady) controller.pause()
                        else {
                            controller.prepare()
                            controller.play()
                        }
                    PREVIOUS -> controller.seekToPrevious()
                    NEXT -> controller.seekToNext()
                }
            } catch (failure: java.util.concurrent.ExecutionException) {
                Log.w("ResonateWidgets", "Connect playback session", failure)
            } catch (failure: java.util.concurrent.TimeoutException) {
                Log.w("ResonateWidgets", "Connect playback session", failure)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: IllegalStateException) {
                Log.w("ResonateWidgets", "Control playback session", failure)
            } catch (failure: SecurityException) {
                Log.w("ResonateWidgets", "Control playback session", failure)
            } finally {
                MediaController.releaseFuture(connection)
                result.finish()
            }
        }
    }

    private fun render(context: Context, song: WidgetSong) {
        try {
            drawn(context, song)
        } catch (failure: Exception) {
            Log.w("ResonateWidgets", "The widgets could not be drawn", failure)
        } catch (failure: OutOfMemoryError) {
            Log.w("ResonateWidgets", "The widget artwork did not fit in memory", failure)
        }
    }

    private fun drawn(context: Context, song: WidgetSong) {
        val manager = AppWidgetManager.getInstance(context)
        val layouts =
            listOf(
                ResonatePlayerWidgetProvider::class.java to R.layout.resonate_player_widget,
                ResonateArtworkWidgetProvider::class.java to R.layout.resonate_artwork_widget,
            )
        val installed =
            layouts
                .map { (provider, layout) ->
                    manager.getAppWidgetIds(ComponentName(context, provider)) to layout
                }
                .filter { it.first.isNotEmpty() }
        if (installed.isEmpty()) return
        val art = song.artwork?.let(::decoded)
        val background = backdrop(art)
        for ((ids, layout) in installed) {
            val views = RemoteViews(context.packageName, layout)
            views.setTextViewText(R.id.widget_title, song.title)
            views.setTextViewText(R.id.widget_artist, song.artist)
            views.setTextViewText(R.id.widget_album, song.album)
            views.setImageViewBitmap(R.id.widget_backdrop, background)
            if (art != null) views.setImageViewBitmap(R.id.widget_artwork, art)
            else views.setImageViewResource(R.id.widget_artwork, R.drawable.widget_cover)
            views.setImageViewResource(
                R.id.widget_play_pause,
                if (song.playing) R.drawable.widget_pause else R.drawable.widget_play,
            )
            views.setContentDescription(
                R.id.widget_play_pause,
                context.getString(
                    if (song.playing) R.string.widget_pause else R.string.widget_play
                ),
            )
            views.setOnClickPendingIntent(
                R.id.widget_root,
                PendingIntent.getActivity(
                    context,
                    0,
                    Intent(context, MainActivity::class.java)
                        .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                ),
            )
            for ((id, action) in
                listOf(
                    R.id.widget_prev to PREVIOUS,
                    R.id.widget_play_pause to TOGGLE,
                    R.id.widget_next to NEXT,
                )) {
                views.setOnClickPendingIntent(
                    id,
                    PendingIntent.getBroadcast(
                        context,
                        id,
                        Intent(context, ResonatePlayerWidgetProvider::class.java).setAction(action),
                        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                    ),
                )
            }
            manager.updateAppWidget(ids, views)
        }
    }

    private fun decoded(bytes: ByteArray): Bitmap? {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)

        var sample = 1
        while (maxOf(bounds.outWidth, bounds.outHeight) / (sample * 2) >= ARTWORK_EDGE_AT_MOST)
            sample *= 2
        val options = BitmapFactory.Options().apply { inSampleSize = sample }
        return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options)
    }

    private fun backdrop(art: Bitmap?): Bitmap {
        val bitmap = Bitmap.createBitmap(320, 180, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        canvas.drawColor(Color.TRANSPARENT)
        val tone = art?.getPixel(art.width / 2, art.height / 2) ?: Color.rgb(185, 170, 255)
        val glow = Color.argb(100, Color.red(tone), Color.green(tone), Color.blue(tone))
        canvas.drawCircle(
            50f,
            20f,
            290f,
            Paint().apply {
                shader =
                    RadialGradient(50f, 20f, 290f, glow, Color.TRANSPARENT, Shader.TileMode.CLAMP)
            },
        )
        return bitmap
    }
}
