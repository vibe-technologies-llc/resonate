package org.resonate.android

import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.net.Uri
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import org.resonate.android.nativecore.Exception as NativeException
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.SearchSong

class DownloadQueue(
    private val context: Context,
    private val library: LibraryRepository,
    private val scope: CoroutineScope,
    private val settings: SharedPreferences,
    private val seams: DownloadSeams,
) {
    private val keys = AtomicLong()
    private val mutableJobs = MutableStateFlow<List<DownloadJob>>(emptyList())
    private val mutableRemembered = MutableStateFlow(settings.getString(FOLDER_KEY, null))
    private val mutableAutomatic =
        MutableStateFlow(settings.getBoolean(AUTOMATIC_KEY, true))
    private var serviceReady = CompletableDeferred<Unit>()
    private var asking: Job? = null
    private val askedOnItsOwn = mutableMapOf<String, Long>()

    val jobs: StateFlow<List<DownloadJob>> = mutableJobs.asStateFlow()
    val rememberedFolder: StateFlow<String?> = mutableRemembered.asStateFlow()
    val automatic: StateFlow<Boolean> = mutableAutomatic.asStateFlow()

    val active: Boolean
        get() = mutableJobs.value.any { !it.state.settled }

    fun serviceStarted() {
        serviceReady.complete(Unit)
    }

    fun holds(song: SearchSong): Boolean = song.heldKey() in DownloadPlan.held(mutableJobs.value)

    fun enqueue(songs: List<SearchSong>, folder: Uri): Int {
        val seen = DownloadPlan.held(mutableJobs.value).toMutableSet()
        val fresh = songs.filter { seen.add(it.heldKey()) }
        if (fresh.isEmpty()) return 0
        if (!active) serviceReady = CompletableDeferred()
        mutableJobs.value =
            DownloadPlan.nextBatch(mutableJobs.value) +
                fresh.map { DownloadJob(keys.incrementAndGet(), it, folder.toString()) }
        pump()
        return fresh.size
    }

    fun cancel(key: Long) {
        val job = mutableJobs.value.firstOrNull { it.key == key } ?: return
        if (job.state.settled) return
        if (job.state == DownloadState.WAITING) change(key) { it.copy(state = DownloadState.CANCELLED) }
        job.native?.let { id -> library.request { it.cancelDownload(id) } }
    }

    fun cancelAll() {
        mutableJobs.value.filter { !it.state.settled }.forEach { cancel(it.key) }
    }

    fun clearSettled() {
        mutableJobs.value = mutableJobs.value.filter { !it.state.settled }
    }

    fun follow(native: MobilePlayer) {
        for (job in mutableJobs.value.filter { it.state.inFlight }) {
            val id = job.native ?: continue
            val progress = native.downloadProgress(id) ?: continue
            if (progress.received != job.received) change(job.key) { it.copy(received = progress.received) }
        }
    }

    fun remember(folder: Uri?) {
        settings.edit().apply {
            if (folder == null) remove(FOLDER_KEY) else putString(FOLDER_KEY, folder.toString())
        }.apply()
        mutableRemembered.value = folder?.toString()
        wantsChanged()
    }

    fun automatically(enabled: Boolean) {
        settings.edit().putBoolean(AUTOMATIC_KEY, enabled).apply()
        mutableAutomatic.value = enabled
        wantsChanged()
    }

    fun wantsChanged() {
        asking?.cancel()
        asking = null
        if (seams.foreground() && mayAsk()) asking = scope.launch { keepAsking() }
    }

    private fun mayAsk(): Boolean =
        mutableAutomatic.value &&
            seams.online() &&
            seams.unmetered() &&
            seams.accounts().isNotEmpty() &&
            seams.writable(mutableRemembered.value)

    private suspend fun keepAsking() {
        while (scope.isActive && mayAsk()) {
            try {
                val native = library.native()
                val next = native.nextWantedDue() ?: return
                delay(DownloadPlan.waitForWants(next.toLong(), System.currentTimeMillis() / 1000))
                if (mayAsk()) askDueWants(native)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                library.report(failure)
                return
            }
        }
    }

    private fun askDueWants(native: MobilePlayer) {
        val folder = mutableRemembered.value?.let(Uri::parse) ?: return
        val now = System.currentTimeMillis()
        askedOnItsOwn.values.removeAll { now - it >= DownloadPlan.WANTS_ASKED_EVERY_MS }
        val songs =
            native.pollWantedDue().map { it.song }.filter { !holds(it) && it.heldKey() !in askedOnItsOwn }
        songs.forEach { askedOnItsOwn[it.heldKey()] = now }
        enqueue(songs, folder)
    }

    private fun pump() {
        val starting = DownloadPlan.startable(mutableJobs.value)
        starting.forEach { key -> change(key) { it.copy(state = DownloadState.FINDING) } }
        starting.forEach { key -> scope.launch { run(key) } }
    }

    private fun change(key: Long, edit: (DownloadJob) -> DownloadJob) {
        mutableJobs.value = mutableJobs.value.map { if (it.key == key) edit(it) else it }
    }

    private fun job(key: Long): DownloadJob? = mutableJobs.value.firstOrNull { it.key == key }

    private suspend fun run(key: Long) {
        var created: Uri? = null
        try {
            awaitService()
            val accounts = seams.accounts()
            check(accounts.isNotEmpty()) { "Set up a download provider first" }
            val native = library.native()
            val song = checkNotNull(job(key)).song
            val folder = Uri.parse(checkNotNull(job(key)).folder)
            val id = native.beginDownload(song)
            change(key) { it.copy(native = id) }
            if (job(key)?.state == DownloadState.CANCELLED) native.cancelDownload(id)
            val offer = native.findDownload(id, accounts)
            native.refreshedTidalToken()?.let(seams::renewed)
            change(key) { it.copy(state = DownloadState.DOWNLOADING, name = offer.name) }
            val uri = MusicDocuments.create(context, folder, mimeOf(offer.extension), offer.name)
            created = uri
            journal(uri)
            val descriptor =
                requireNotNull(context.contentResolver.openFileDescriptor(uri, "w")).detachFd()
            val count = native.downloadOffer(id, descriptor)
            change(key) { it.copy(state = DownloadState.VALIDATING, received = count) }
            native.indexDownload(id, uri.toString(), offer.name)
            created = null
            MusicDocuments.publish(context, uri)
            forgetJournal(uri)
            change(key) { it.copy(state = DownloadState.DONE, received = count) }
            seams.landed()
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            val said = failure is NativeException.DownloadCancelled
            change(key) {
                it.copy(
                    state = if (said) DownloadState.CANCELLED else DownloadState.FAILED,
                    failure = failure.message ?: "Download failed",
                )
            }
            if (!said) library.report(failure)
        } finally {
            created?.let(::discard)
            pump()
        }
    }

    private suspend fun awaitService() {
        if (library.features.transferServiceRunning.value) return
        try {
            context.startForegroundService(Intent(context, DownloadService::class.java))
        } catch (failure: Exception) {
            throw IllegalStateException("The download service could not start", failure)
        }
        checkNotNull(withTimeoutOrNull(SERVICE_STARTS_WITHIN_MS) { serviceReady.await() }) {
            "The download service did not start"
        }
    }

    private fun journal(uri: Uri) {
        val left = settings.getStringSet(JOURNAL_KEY, emptySet()).orEmpty() + uri.toString()
        check(settings.edit().putStringSet(JOURNAL_KEY, left).commit()) {
            "The download could not be recorded"
        }
    }

    private fun forgetJournal(uri: Uri): Boolean {
        val left = settings.getStringSet(JOURNAL_KEY, emptySet()).orEmpty() - uri.toString()
        return settings.edit().putStringSet(JOURNAL_KEY, left).commit()
    }

    private fun discard(uri: Uri) {
        try {
            MusicDocuments.delete(context, uri)
            check(forgetJournal(uri)) { JOURNAL_NOT_CLEARED }
        } catch (failure: Exception) {
            library.report(failure)
        }
    }

    fun recover(native: MobilePlayer) {
        if (active) return
        val indexed =
            try {
                native.songUris().toSet()
            } catch (failure: Exception) {
                library.report(failure)
                return
            }
        val legacy = settings.getString(LEGACY_JOURNAL_KEY, null)
        val left = settings.getStringSet(JOURNAL_KEY, emptySet()).orEmpty() + listOfNotNull(legacy)
        val stranded = DownloadPlan.stranded(left, indexed)
        left.forEach { value ->
            try {
                val uri = Uri.parse(value)
                MusicDocuments.settleLeftover(context, uri, value in stranded)
                if (value == legacy) {
                    check(settings.edit().remove(LEGACY_JOURNAL_KEY).commit()) {
                        JOURNAL_NOT_CLEARED
                    }
                }
                check(forgetJournal(uri)) { JOURNAL_NOT_CLEARED }
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
    }

    private companion object {
        const val FOLDER_KEY = "download-folder"
        const val AUTOMATIC_KEY = "download-wants-automatically"
        const val JOURNAL_KEY = "unfinished-downloads"
        const val LEGACY_JOURNAL_KEY = "unfinished-download"
        const val SERVICE_STARTS_WITHIN_MS = 10_000L
        const val JOURNAL_NOT_CLEARED = "The download journal could not be cleared"
    }
}

interface DownloadSeams {
    fun accounts(): List<org.resonate.android.nativecore.DownloadAccount>

    fun renewed(token: String)

    fun online(): Boolean

    fun unmetered(): Boolean

    fun foreground(): Boolean

    fun writable(folder: String?): Boolean

    suspend fun landed()
}

internal fun mimeOf(extension: String) =
    when (extension) {
        "flac" -> "audio/flac"
        "mp3" -> "audio/mpeg"
        "m4a" -> "audio/mp4"
        "ogg",
        "opus" -> "audio/ogg"
        else -> "application/octet-stream"
    }
