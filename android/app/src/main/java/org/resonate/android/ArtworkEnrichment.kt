package org.resonate.android

import android.content.SharedPreferences
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.resonate.android.nativecore.EnrichmentOptions
import org.resonate.android.nativecore.EnrichmentStatus
import org.resonate.android.nativecore.MobilePlayer

class ArtworkEnrichment(
    private val library: LibraryRepository,
    private val settings: SharedPreferences,
    private val scope: CoroutineScope,
    private val artworkChanged: () -> Unit,
) {
    val automatic = MutableStateFlow(settings.getBoolean("enrich-automatically", true))
    val unmeteredOnly = MutableStateFlow(settings.getBoolean("enrich-unmetered", false))
    val status = MutableStateFlow<EnrichmentStatus?>(null)
    val coversAskedAgain = MutableStateFlow<UInt?>(null)
    private val access = Mutex()
    private var watching: Job? = null
    private var artRevision: ULong? = null
    @Volatile private var online = false
    @Volatile private var unmetered = false
    @Volatile private var foreground = true
    @Volatile private var sought: ULong? = null

    private val mayAsk: Boolean
        get() = automatic.value && online && (!unmeteredOnly.value || unmetered)

    fun initialize(enabled: Boolean) {
        online = enabled
        artRevision = null
        sought = null
        read()
        begin()
    }

    fun setAutomatic(enabled: Boolean): Job {
        automatic.value = enabled
        return work { native ->
            settings.edit().putBoolean("enrich-automatically", automatic.value).commit()
            if (mayAsk) start(native, refresh = false, stopFirst = false)
        }
    }

    fun setUnmeteredOnly(enabled: Boolean): Job {
        unmeteredOnly.value = enabled
        return work { native ->
            settings.edit().putBoolean("enrich-unmetered", unmeteredOnly.value).commit()
            if (mayAsk) {
                start(native, refresh = false, stopFirst = false)
            } else if (automatic.value && unmeteredOnly.value && !unmetered) {
                stop(native)
            }
        }
    }

    fun onlineChanged(enabled: Boolean) {
        online = enabled
        if (enabled) begin() else read()
    }

    fun networkChanged(free: Boolean) {
        unmetered = free
        work { native ->
            if (mayAsk) {
                start(native, refresh = false, stopFirst = false)
            } else if (automatic.value && unmeteredOnly.value && !free) {
                stop(native)
            }
        }
    }

    fun foreground(visible: Boolean) {
        if (foreground == visible) return
        foreground = visible
        if (visible) read() else close()
    }

    fun begin() = work { native ->
        if (mayAsk) start(native, refresh = false, stopFirst = false)
    }

    fun refresh() = work { native ->
        if (online) start(native, refresh = true, stopFirst = true)
    }

    fun retryMissingCovers() = work { native ->
        if (online) {
            coversAskedAgain.value = native.askAgainForCovers()
            start(native, refresh = false, stopFirst = true)
        }
    }

    fun stop() = work { native -> stop(native) }

    fun read() = work { native ->
        update(native)
        if (status.value?.running == true) watch(native)
    }

    fun playing(id: ULong?) {
        if (id == sought) return
        sought = id
        if (id != null && status.value?.running == true) {
            work { native -> native.seekEnrichment(id) }
        }
    }

    fun close() {
        watching?.cancel()
        watching = null
    }

    private suspend fun start(native: MobilePlayer, refresh: Boolean, stopFirst: Boolean) {
        if (stopFirst) native.stopEnrichment()
        native.startEnrichment(EnrichmentOptions(refresh = refresh, atMost = null))
        library.snapshot.value?.current?.id?.let(native::seekEnrichment)
        update(native)
        if (status.value?.running == true) watch(native)
    }

    private suspend fun stop(native: MobilePlayer) {
        watching?.cancel()
        watching = null
        native.stopEnrichment()
        update(native)
    }

    private suspend fun update(native: MobilePlayer) {
        val previous = status.value
        val read = native.enrichmentStatus()
        status.value = read
        val changed = native.artChangedSince(artRevision ?: 0uL)
        val hadRevision = artRevision != null
        artRevision = changed.revision
        val picturesChanged = hadRevision && changed.tracks.isNotEmpty()
        val portraitsChanged = previous != null && previous.portraits != read.portraits
        if (picturesChanged || portraitsChanged) artworkChanged()
        if (picturesChanged || previous?.running == true && !read.running) library.refresh()
    }

    private fun watch(native: MobilePlayer) {
        if (watching != null || !foreground) return
        watching = scope.launch {
            try {
                while (isActive) {
                    delay(2000L)
                    access.withLock { update(native) }
                    if (status.value?.running != true) break
                }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                library.report(failure)
            } finally {
                if (watching === currentCoroutineContext()[Job]) watching = null
            }
        }
    }

    private fun work(action: suspend (MobilePlayer) -> Unit): Job = scope.launch {
        try {
            val native = library.native()
            access.withLock { action(native) }
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            library.report(failure)
        }
    }
}
