package org.resonate.android

import android.content.Context
import android.net.wifi.WifiManager
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.Track
import org.resonate.android.nativecore.Transport

internal fun streamsOverTheNetwork(current: Track?, transport: Transport?): Boolean =
    current?.served == true && (transport == Transport.PLAYING || transport == Transport.LOADING)

internal class StreamingWifi(context: Context) {
    @Suppress("DEPRECATION")
    private val lock: WifiManager.WifiLock? =
        context.applicationContext
            .getSystemService(WifiManager::class.java)
            ?.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "resonate-streaming")
            ?.apply { setReferenceCounted(false) }

    fun follow(snapshot: Snapshot?) {
        if (streamsOverTheNetwork(snapshot?.current, snapshot?.transport)) hold() else release()
    }

    fun release() {
        lock?.takeIf { it.isHeld }?.release()
    }

    private fun hold() {
        lock?.takeIf { !it.isHeld }?.acquire()
    }
}
