package org.resonate.android

import android.content.Context
import android.content.SharedPreferences
import android.net.wifi.WifiManager
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.NetworkRenderer

private const val LOOKS_FOR_RENDERERS = "renderers"
private const val RENDERER_ADDRESSES = "renderer-addresses"

class NetworkRenderers(
    context: Context,
    private val library: LibraryRepository,
    private val settings: SharedPreferences,
) {
    private val mutableLooking = MutableStateFlow(false)
    val looking = mutableLooking.asStateFlow()
    private val mutableListed = MutableStateFlow<List<NetworkRenderer>>(emptyList())
    val listed = mutableListed.asStateFlow()
    private val mutableAddresses = MutableStateFlow<List<String>>(emptyList())
    val addresses = mutableAddresses.asStateFlow()
    private val searchesHeard: WifiManager.MulticastLock? by lazy {
        context.applicationContext
            .getSystemService(WifiManager::class.java)
            ?.createMulticastLock("resonate-renderers")
            ?.apply { setReferenceCounted(false) }
    }

    fun begin(native: MobilePlayer) {
        for (address in savedAddresses()) {
            try {
                native.addRendererAddress(address)
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
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

    fun add(address: String) {
        library.request { native ->
            try {
                native.addRendererAddress(address.trim())
            } finally {
                keep(native)
            }
        }
    }

    fun forget(address: String) {
        library.request { native ->
            try {
                native.removeRendererAddress(address)
            } finally {
                keep(native)
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
        mutableAddresses.value = emptyList()
    }

    private fun savedAddresses(): List<String> =
        settings.getStringSet(RENDERER_ADDRESSES, emptySet()).orEmpty().sorted()

    private fun keep(native: MobilePlayer) {
        val held = native.rendererAddresses()
        settings.edit().putStringSet(RENDERER_ADDRESSES, held.toSet()).apply()
        mutableAddresses.value = held
        mutableListed.value = native.renderers()
    }

    private fun settle(native: MobilePlayer) {
        val looking = native.lookingForRenderers()
        hearSearches(looking)
        mutableLooking.value = looking
        mutableListed.value = native.renderers()
        mutableAddresses.value = native.rendererAddresses()
    }

    private fun hearSearches(hear: Boolean) {
        val lock = searchesHeard ?: return
        if (hear && !lock.isHeld) lock.acquire()
        if (!hear && lock.isHeld) lock.release()
    }
}
