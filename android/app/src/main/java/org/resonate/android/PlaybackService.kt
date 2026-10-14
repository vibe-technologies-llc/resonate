package org.resonate.android

import android.app.PendingIntent
import android.content.Intent
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.MediaLibraryService
import androidx.media3.session.MediaSession

@UnstableApi
class PlaybackService : MediaLibraryService() {
    private var session: MediaLibrarySession? = null
    private var player: RustMediaPlayer? = null

    override fun onCreate() {
        super.onCreate()
        val library = (application as ResonateApplication).library
        library.retain()
        val adapter = RustMediaPlayer(this, library)
        player = adapter
        val openApp =
            PendingIntent.getActivity(
                this,
                0,
                Intent(this, MainActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
        session =
            MediaLibrarySession.Builder(this, adapter, object : MediaLibrarySession.Callback {})
                .setSessionActivity(openApp)
                .build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaLibrarySession? =
        session

    override fun onTaskRemoved(rootIntent: Intent?) {
        pauseAllPlayersAndStopSelf()
    }

    override fun onDestroy() {
        session?.release()
        player?.release()
        session = null
        super.onDestroy()
    }
}
