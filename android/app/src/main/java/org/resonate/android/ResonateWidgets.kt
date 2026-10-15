package org.resonate.android

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.widget.RemoteViews
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import org.resonate.android.nativecore.Transport

class ResonatePlayerWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(
        context: Context,
        appWidgetManager: AppWidgetManager,
        appWidgetIds: IntArray,
    ) {
        val app = context.applicationContext as? ResonateApplication ?: return
        ResonateWidgets.updateAll(context, app.library)
    }

    override fun onReceive(context: Context, intent: Intent) {
        super.onReceive(context, intent)
        ResonateWidgets.handleAction(context, intent.action)
    }
}

class ResonateTurntableWidgetProvider : AppWidgetProvider() {
    override fun onUpdate(
        context: Context,
        appWidgetManager: AppWidgetManager,
        appWidgetIds: IntArray,
    ) {
        val app = context.applicationContext as? ResonateApplication ?: return
        ResonateWidgets.updateAll(context, app.library)
    }

    override fun onReceive(context: Context, intent: Intent) {
        super.onReceive(context, intent)
        ResonateWidgets.handleAction(context, intent.action)
    }
}

object ResonateWidgets {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    const val ACTION_PLAY_PAUSE = "org.resonate.android.ACTION_WIDGET_PLAY_PAUSE"
    const val ACTION_PREVIOUS = "org.resonate.android.ACTION_WIDGET_PREVIOUS"
    const val ACTION_NEXT = "org.resonate.android.ACTION_WIDGET_NEXT"

    fun handleAction(context: Context, action: String?) {
        val app = context.applicationContext as? ResonateApplication ?: return
        val library = app.library
        when (action) {
            ACTION_PLAY_PAUSE -> {
                library.request { native ->
                    val snapshot = library.snapshot.value
                    if (snapshot?.transport == Transport.PLAYING) native.pause()
                    else native.play()
                }
            }
            ACTION_PREVIOUS -> {
                library.request { native ->
                    val snapshot = library.snapshot.value
                    val current = snapshot?.queueIndex?.toInt() ?: 0
                    if ((snapshot?.positionMs ?: 0uL) > 3000uL || current == 0) {
                        native.seek(0uL)
                    } else {
                        native.jumpTo((current - 1).toUInt())
                    }
                }
            }
            ACTION_NEXT -> {
                library.request { native ->
                    val snapshot = library.snapshot.value
                    val queue = library.queue.value
                    val current = snapshot?.queueIndex?.toInt() ?: 0
                    if (current + 1 < queue.size) {
                        native.jumpTo((current + 1).toUInt())
                    }
                }
            }
            else -> return
        }
        updateAll(context, library)
    }

    fun updateAll(context: Context, library: LibraryRepository) {
        val manager = AppWidgetManager.getInstance(context) ?: return
        val playerComp = ComponentName(context, ResonatePlayerWidgetProvider::class.java)
        val turntableComp = ComponentName(context, ResonateTurntableWidgetProvider::class.java)
        val playerIds = manager.getAppWidgetIds(playerComp)
        val turntableIds = manager.getAppWidgetIds(turntableComp)
        if (playerIds.isEmpty() && turntableIds.isEmpty()) return

        val snapshot = library.snapshot.value
        val track = snapshot?.current
        val playing = snapshot?.transport == Transport.PLAYING
        val title = track?.title ?: "Resonate"
        val artist = track?.artist?.ifBlank { "No song playing" } ?: "No song playing"
        val album = track?.album.orEmpty().ifBlank { "Resonate Audio" }
        val playPauseIcon =
            if (playing) android.R.drawable.ic_media_pause else android.R.drawable.ic_media_play

        val openApp =
            PendingIntent.getActivity(
                context,
                0,
                Intent(context, MainActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        val prevIntent =
            PendingIntent.getBroadcast(
                context,
                1,
                Intent(context, ResonatePlayerWidgetProvider::class.java)
                    .setAction(ACTION_PREVIOUS),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        val playIntent =
            PendingIntent.getBroadcast(
                context,
                2,
                Intent(context, ResonatePlayerWidgetProvider::class.java)
                    .setAction(ACTION_PLAY_PAUSE),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        val nextIntent =
            PendingIntent.getBroadcast(
                context,
                3,
                Intent(context, ResonatePlayerWidgetProvider::class.java).setAction(ACTION_NEXT),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )

        fun applyViews(artBitmap: Bitmap?) {
            if (playerIds.isNotEmpty()) {
                val views = RemoteViews(context.packageName, R.layout.resonate_player_widget)
                views.setTextViewText(R.id.widget_title, title)
                views.setTextViewText(R.id.widget_artist, artist)
                views.setImageViewResource(R.id.widget_play_pause, playPauseIcon)
                views.setOnClickPendingIntent(R.id.widget_root, openApp)
                views.setOnClickPendingIntent(R.id.widget_prev, prevIntent)
                views.setOnClickPendingIntent(R.id.widget_play_pause, playIntent)
                views.setOnClickPendingIntent(R.id.widget_next, nextIntent)
                if (artBitmap != null) {
                    views.setImageViewBitmap(R.id.widget_artwork, artBitmap)
                } else {
                    views.setImageViewResource(
                        R.id.widget_artwork,
                        android.R.drawable.ic_media_play,
                    )
                }
                manager.updateAppWidget(playerIds, views)
            }

            if (turntableIds.isNotEmpty()) {
                val views = RemoteViews(context.packageName, R.layout.resonate_turntable_widget)
                views.setTextViewText(R.id.widget_title, title)
                views.setTextViewText(R.id.widget_artist, artist)
                views.setTextViewText(R.id.widget_album, album)
                views.setImageViewResource(R.id.widget_play_pause, playPauseIcon)
                views.setOnClickPendingIntent(R.id.widget_root, openApp)
                views.setOnClickPendingIntent(R.id.widget_prev, prevIntent)
                views.setOnClickPendingIntent(R.id.widget_play_pause, playIntent)
                views.setOnClickPendingIntent(R.id.widget_next, nextIntent)
                if (artBitmap != null) {
                    views.setImageViewBitmap(R.id.widget_artwork, artBitmap)
                } else {
                    views.setImageViewResource(
                        R.id.widget_artwork,
                        android.R.drawable.ic_media_play,
                    )
                }
                manager.updateAppWidget(turntableIds, views)
            }
        }

        applyViews(null)
        if (track != null) {
            scope.launch {
                val rawArt = library.features.mediaArtwork(track)
                val artBitmap = rawArt?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
                if (artBitmap != null) {
                    applyViews(artBitmap)
                }
            }
        }
    }
}
