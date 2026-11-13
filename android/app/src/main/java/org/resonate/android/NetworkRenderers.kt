package org.resonate.android

import android.content.Context
import android.content.SharedPreferences
import android.net.wifi.WifiManager
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.NetworkRenderer
import org.resonate.android.nativecore.RendererLearnt
import org.resonate.android.nativecore.TrialHeard

private const val LOOKS_FOR_RENDERERS = "renderers"
private const val RENDERER_ADDRESSES = "renderer-addresses"
private const val RECEIVER_ADDRESSES = "receiver-addresses"

data class RendererHeard(val node: String, val trials: List<TrialHeard>)

class NetworkRenderers(
    context: Context,
    private val library: LibraryRepository,
    private val settings: SharedPreferences,
    private val scope: CoroutineScope,
) {
    private val mutableLearnt = MutableStateFlow<List<RendererLearnt>>(emptyList())
    val learnt = mutableLearnt.asStateFlow()
    private val mutableTrying = MutableStateFlow<String?>(null)
    val trying = mutableTrying.asStateFlow()
    private val mutableHeard = MutableStateFlow<RendererHeard?>(null)
    val heard = mutableHeard.asStateFlow()
    private val mutableLooking = MutableStateFlow(false)
    val looking = mutableLooking.asStateFlow()
    private val mutableListed = MutableStateFlow<List<NetworkRenderer>>(emptyList())
    val listed = mutableListed.asStateFlow()
    private val mutableAddresses = MutableStateFlow<List<String>>(emptyList())
    val addresses = mutableAddresses.asStateFlow()
    private val mutableReceiverAddresses = MutableStateFlow<List<String>>(emptyList())
    val receiverAddresses = mutableReceiverAddresses.asStateFlow()
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
        for (address in savedReceiverAddresses()) {
            try {
                native.addReceiverAddress(address)
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

    fun addReceiver(address: String) {
        library.request { native ->
            try {
                native.addReceiverAddress(address.trim())
            } finally {
                keep(native)
            }
        }
    }

    fun forgetReceiver(address: String) {
        library.request { native ->
            try {
                native.removeReceiverAddress(address)
            } finally {
                keep(native)
            }
        }
    }

    fun refresh() {
        library.request { native -> mutableListed.value = native.renderers() }
    }

    fun learn(node: String) {
        if (!mutableTrying.compareAndSet(null, node)) return
        mutableHeard.value = null
        scope.launch {
            try {
                val trials = library.holding { it.learnWhatRendererPlays(node) }
                mutableHeard.value = RendererHeard(node, trials)
                mutableLearnt.value = library.holding { it.renderersLearnt() }
            } finally {
                mutableTrying.value = null
            }
        }
    }

    fun forgetWhatItPlays(node: String) {
        library.request { native ->
            native.forgetWhatRendererPlays(node)
            if (mutableHeard.value?.node == node) mutableHeard.value = null
            mutableLearnt.value = native.renderersLearnt()
        }
    }

    fun stop() {
        hearSearches(false)
        mutableLooking.value = false
        mutableListed.value = emptyList()
        mutableAddresses.value = emptyList()
        mutableReceiverAddresses.value = emptyList()
        mutableLearnt.value = emptyList()
        mutableHeard.value = null
    }

    private fun savedAddresses(): List<String> =
        settings.getStringSet(RENDERER_ADDRESSES, emptySet()).orEmpty().sorted()

    private fun savedReceiverAddresses(): List<String> =
        settings.getStringSet(RECEIVER_ADDRESSES, emptySet()).orEmpty().sorted()

    private fun keep(native: MobilePlayer) {
        val held = native.rendererAddresses()
        val receivers = native.receiverAddresses()
        settings
            .edit()
            .putStringSet(RENDERER_ADDRESSES, held.toSet())
            .putStringSet(RECEIVER_ADDRESSES, receivers.toSet())
            .apply()
        mutableAddresses.value = held
        mutableReceiverAddresses.value = receivers
        mutableListed.value = native.renderers()
    }

    private fun settle(native: MobilePlayer) {
        val looking = native.lookingForRenderers()
        hearSearches(looking)
        mutableLooking.value = looking
        mutableListed.value = native.renderers()
        mutableAddresses.value = native.rendererAddresses()
        mutableReceiverAddresses.value = native.receiverAddresses()
        mutableLearnt.value = native.renderersLearnt()
    }

    private fun hearSearches(hear: Boolean) {
        val lock = searchesHeard ?: return
        if (hear && !lock.isHeld) lock.acquire()
        if (!hear && lock.isHeld) lock.release()
    }
}
