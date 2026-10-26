package org.resonate.android

import android.net.Uri
import androidx.media3.session.MediaController
import org.resonate.android.ui.Host

class CurrentActivity : Host {
    @Volatile private var host: Host? = null
    @Volatile var controller: MediaController? = null
        private set

    fun attach(activity: Host) {
        host = activity
    }

    fun detach(activity: Host) {
        if (host === activity) host = null
    }

    fun connected(connection: MediaController) {
        controller = connection
    }

    fun disconnected(connection: MediaController) {
        if (controller === connection) controller = null
    }

    override fun chooseFolder() {
        host?.chooseFolder()
    }

    override fun chooseRoomResponse() {
        host?.chooseRoomResponse()
    }

    override fun useDownloads() {
        host?.useDownloads()
    }

    override fun allowNotifications() {
        host?.allowNotifications()
    }

    override fun share(title: String, link: String) {
        host?.share(title, link)
    }

    override fun open(link: Uri) {
        host?.open(link)
    }

    override suspend fun confirmDelete(uris: List<Uri>): Boolean =
        host?.confirmDelete(uris) ?: false

    override suspend fun confirmWrite(uris: List<Uri>): Boolean = host?.confirmWrite(uris) ?: false

    override suspend fun allowMicrophone(): Boolean = host?.allowMicrophone() ?: false

    override fun openAppSettings() {
        host?.openAppSettings()
    }
}
