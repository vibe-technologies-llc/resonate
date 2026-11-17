package org.resonate.android

import android.content.Context
import android.content.SharedPreferences
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
    private val searchesHeard = multicastHearing<MobilePlayer>(context, "resonate-catalog")

    fun begin(native: MobilePlayer) {
        searchesHeard.begin(native)
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
                searchesHeard.hear(native, true)
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
                if (on) searchesHeard.hear(native, true)
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
        searchesHeard.stop()
        mutableShared.value = false
    }

    private fun settle(native: MobilePlayer) {
        val shared = native.catalogSharedOnNetwork()
        searchesHeard.hear(native, shared)
        mutableShared.value = shared
    }
}
