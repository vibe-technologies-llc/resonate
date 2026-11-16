package org.resonate.android

import android.content.Context
import android.content.SharedPreferences
import android.net.wifi.WifiManager
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.resonate.android.nativecore.MobilePlayer

private const val SHARES_THE_CATALOG = "share-catalog-on-network"
private const val NAMED_ON_THE_NETWORK = "offer-name"

class CatalogOffering(
    context: Context,
    private val library: LibraryRepository,
    private val settings: SharedPreferences,
) {
    private val mutableShared = MutableStateFlow(false)
    val shared = mutableShared.asStateFlow()
    private val mutableName = MutableStateFlow(settings.getString(NAMED_ON_THE_NETWORK, "") ?: "")
    val name = mutableName.asStateFlow()
    private val searchesHeard: WifiManager.MulticastLock? by lazy {
        context.applicationContext
            .getSystemService(WifiManager::class.java)
            ?.createMulticastLock("resonate-catalog")
            ?.apply { setReferenceCounted(false) }
    }

    fun begin(native: MobilePlayer) {
        val kept = mutableName.value
        if (kept.isNotEmpty()) {
            try {
                mutableName.value = native.nameCatalogOnNetwork(kept)
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
        if (settings.getBoolean(SHARES_THE_CATALOG, false)) {
            try {
                hearSearches(true)
                native.shareCatalogOnNetwork(true)
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
        settle(native)
    }

    fun share(on: Boolean) {
        library.request { native ->
            try {
                if (on) hearSearches(true)
                native.shareCatalogOnNetwork(on)
                settings.edit().putBoolean(SHARES_THE_CATALOG, on).apply()
            } finally {
                settle(native)
            }
        }
    }

    fun rename(typed: String) {
        library.request { native ->
            try {
                val kept = native.nameCatalogOnNetwork(typed)
                settings.edit().putString(NAMED_ON_THE_NETWORK, kept).apply()
                mutableName.value = kept
            } finally {
                settle(native)
            }
        }
    }

    fun stop() {
        hearSearches(false)
        mutableShared.value = false
    }

    private fun settle(native: MobilePlayer) {
        val shared = native.catalogSharedOnNetwork()
        hearSearches(shared)
        mutableShared.value = shared
    }

    private fun hearSearches(hear: Boolean) {
        val lock = searchesHeard ?: return
        if (hear && !lock.isHeld) lock.acquire()
        if (!hear && lock.isHeld) lock.release()
    }
}
