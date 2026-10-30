package org.resonate.android

import android.content.Context
import android.content.SharedPreferences
import android.net.wifi.WifiManager
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.NetworkRenderer

private const val LOOKS_FOR_RENDERERS = "renderers"

class NetworkRenderers(
    context: Context,
    private val library: LibraryRepository,
    private val settings: SharedPreferences,
) {
    private val mutableLooking = MutableStateFlow(false)
    val looking = mutableLooking.asStateFlow()
    private val mutableListed = MutableStateFlow<List<NetworkRenderer>>(emptyList())
    val listed = mutableListed.asStateFlow()
    private val searchesHeard: WifiManager.MulticastLock? by lazy {
        context.applicationContext
            .getSystemService(WifiManager::class.java)
            ?.createMulticastLock("resonate-renderers")
            ?.apply { setReferenceCounted(false) }
    }

    fun begin(native: MobilePlayer) {
        if (settings.getBoolean(LOOKS_FOR_RENDERERS, false)) {
            try {
                native.lookForRenderers(true)
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
        settle(native)
    }

    fun look(on: Boolean) {
        library.request { native ->
            hearSearches(on)
            try {
                native.lookForRenderers(on)
                settings.edit().putBoolean(LOOKS_FOR_RENDERERS, on).apply()
            } finally {
                settle(native)
            }
        }
    }

    fun refresh() {
        library.request { native -> mutableListed.value = native.renderers() }
    }

    fun stop() {
        hearSearches(false)
        mutableLooking.value = false
        mutableListed.value = emptyList()
    }

    private fun settle(native: MobilePlayer) {
        val looking = native.lookingForRenderers()
        hearSearches(looking)
        mutableLooking.value = looking
        mutableListed.value = native.renderers()
    }

    private fun hearSearches(hear: Boolean) {
        val lock = searchesHeard ?: return
        if (hear && !lock.isHeld) lock.acquire()
        if (!hear && lock.isHeld) lock.release()
    }
}
