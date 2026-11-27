package org.resonate.android

import android.content.Context
import android.content.SharedPreferences
import android.net.Uri
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.text.format.Formatter
import androidx.annotation.StringRes
import androidx.core.content.edit
import java.io.File
import java.io.FileNotFoundException
import java.io.FileOutputStream
import java.io.InputStream
import java.io.OutputStream
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.RecompressCeiling
import org.resonate.android.nativecore.RecompressStaged
import org.resonate.android.nativecore.SizedSongAfter
import org.resonate.android.nativecore.Track
import org.resonate.android.ui.shownTitle

enum class RecompressTo(
    val stored: String,
    val ceiling: RecompressCeiling,
    @StringRes val label: Int,
    @StringRes val detail: Int,
) {
    LOSSLESS("kept", RecompressCeiling.KEPT, R.string.recompress_to_kept, R.string.recompress_to_kept_detail),
    STUDIO("studio", RecompressCeiling.STUDIO, R.string.recompress_to_studio, R.string.recompress_to_studio_detail),
    HIGH("high", RecompressCeiling.HIGH, R.string.recompress_to_high, R.string.recompress_to_high_detail),
    CD("cd", RecompressCeiling.CD, R.string.recompress_to_cd, R.string.recompress_to_cd_detail);

    companion object {
        const val KEY = "recompress-to"

        fun read(stored: (String) -> String?): RecompressTo =
            stored(KEY)?.let { kept -> entries.firstOrNull { it.stored == kept } } ?: LOSSLESS
    }
}

object Recompressible {
    private const val FLAC = "FLAC"

    fun isFlac(track: Track): Boolean = track.codec.equals(FLAC, ignoreCase = true)
}

sealed interface RecompressOutcome {
    data class Shrunk(val before: ULong, val after: ULong) : RecompressOutcome

    data class Kept(val said: String) : RecompressOutcome

    data class Failed(val reason: String?) : RecompressOutcome

    data object PutBack : RecompressOutcome

    data class KeptAside(val aside: String) : RecompressOutcome

    data object NotAllowed : RecompressOutcome

    data object Stopped : RecompressOutcome

    val failure: Boolean
        get() = this is Failed || this is PutBack || this is KeptAside || this is NotAllowed
}

data class RecompressTold(val title: String, val outcome: RecompressOutcome)

data class RecompressRun(val done: Int, val of: Int)

data class RecompressNotice(val text: String, val failure: Boolean)

object RecompressSentences {
    fun told(words: Words, told: List<RecompressTold>, asked: Int, size: (Long) -> String): RecompressNotice? {
        if (asked == 0) return RecompressNotice(words.string(R.string.recompress_none), failure = false)
        val failure = told.any { it.outcome.failure }
        val stopped = told.any { it.outcome == RecompressOutcome.Stopped } || told.size < asked
        val weighed = told.filter { it.outcome != RecompressOutcome.Stopped }
        if (asked == 1 && weighed.size == 1) return RecompressNotice(one(words, weighed.single(), size), failure)
        if (weighed.isEmpty())
            return if (stopped) RecompressNotice(words.string(R.string.recompress_told_stopped_at_once), failure)
            else null
        val parts = parts(words, weighed, size)
        val text =
            if (stopped) words.string(R.string.recompress_told_stopped, weighed.size, asked, parts)
            else parts
        return RecompressNotice(text, failure)
    }

    fun saved(told: List<RecompressTold>): Long =
        told.sumOf { (it.outcome as? RecompressOutcome.Shrunk)?.let { shrunk -> (shrunk.before - shrunk.after).toLong() } ?: 0L }

    private fun one(words: Words, told: RecompressTold, size: (Long) -> String): String =
        when (val outcome = told.outcome) {
            is RecompressOutcome.Shrunk ->
                words.string(
                    R.string.recompress_shrunk_one,
                    told.title,
                    size(outcome.after.toLong()),
                    size((outcome.before - outcome.after).toLong()),
                )
            is RecompressOutcome.Kept -> words.string(R.string.recompress_kept_one, told.title, outcome.said)
            is RecompressOutcome.Failed ->
                words.string(
                    R.string.recompress_failed_one,
                    told.title,
                    outcome.reason ?: words.string(R.string.recompress_reason_unknown),
                )
            RecompressOutcome.PutBack -> words.string(R.string.recompress_put_back_one, told.title)
            is RecompressOutcome.KeptAside ->
                words.string(R.string.recompress_kept_aside_one, told.title, outcome.aside)
            RecompressOutcome.NotAllowed -> words.string(R.string.recompress_not_allowed_one, told.title)
            RecompressOutcome.Stopped -> words.string(R.string.recompress_told_stopped_at_once)
        }

    private fun parts(words: Words, told: List<RecompressTold>, size: (Long) -> String): String {
        val shrunk = told.count { it.outcome is RecompressOutcome.Shrunk }
        val kept = told.count { it.outcome is RecompressOutcome.Kept }
        val failed =
            told.count {
                it.outcome is RecompressOutcome.Failed ||
                    it.outcome == RecompressOutcome.PutBack ||
                    it.outcome is RecompressOutcome.KeptAside
            }
        val notAllowed = told.count { it.outcome == RecompressOutcome.NotAllowed }
        val aside = told.mapNotNull { (it.outcome as? RecompressOutcome.KeptAside)?.aside }
        return listOfNotNull(
                words
                    .plural(R.plurals.recompress_told_shrunk, shrunk, shrunk, size(saved(told)))
                    .takeIf { shrunk > 0 },
                words.plural(R.plurals.recompress_told_kept, kept, kept).takeIf { kept > 0 },
                words.plural(R.plurals.recompress_told_failed, failed, failed).takeIf { failed > 0 },
                words
                    .plural(R.plurals.recompress_told_not_allowed, notAllowed, notAllowed)
                    .takeIf { notAllowed > 0 },
                words
                    .plural(R.plurals.recompress_told_kept_aside, aside.size, aside.joinToString())
                    .takeIf { aside.isNotEmpty() },
            )
            .joinToString("; ")
    }
}

interface WrittenDocuments {
    fun open(uri: String): InputStream

    fun replace(uri: String): OutputStream

    fun modifiedMs(uri: String): Long
}

sealed interface StageWritten {
    data class Written(val modifiedMs: Long, val aside: File) : StageWritten

    data class Untouched(val failure: Exception) : StageWritten

    data class PutBack(val failure: Exception) : StageWritten

    data class KeptAside(val failure: Exception, val aside: File) : StageWritten
}

class StageWriting(private val documents: WrittenDocuments, private val asideIn: File) {
    fun over(uri: String, stage: File, id: ULong): StageWritten {
        val aside = File(asideIn, "$id-${System.currentTimeMillis()}$ASIDE_SUFFIX")
        try {
            asideIn.mkdirs()
            documents.open(uri).use { from -> aside.outputStream().use { into -> copied(from, into) } }
        } catch (failure: Exception) {
            aside.delete()
            return StageWritten.Untouched(failure)
        }
        return try {
            stage.inputStream().use { from -> documents.replace(uri).use { into -> copied(from, into) } }
            StageWritten.Written(documents.modifiedMs(uri), aside)
        } catch (failure: Exception) {
            if (putBack(uri, aside)) StageWritten.PutBack(failure)
            else StageWritten.KeptAside(failure, aside)
        }
    }

    fun putBack(uri: String, aside: File): Boolean =
        try {
            aside.inputStream().use { from -> documents.replace(uri).use { into -> copied(from, into) } }
            settled(aside)
            true
        } catch (failure: Exception) {
            false
        }

    fun settled(aside: File) {
        aside.delete()
    }

    private fun copied(from: InputStream, into: OutputStream) {
        from.copyTo(into)
        into.flush()
        (into as? FileOutputStream)?.fd?.sync()
    }

    companion object {
        const val ASIDE_FOLDER = "recompress-aside"
        const val ASIDE_SUFFIX = ".flac"
    }
}

class ResolvedDocuments(private val context: Context) : WrittenDocuments {
    private fun local(uri: String): File? =
        OpenedNames.localName(uri)?.let { File(File(context.filesDir, OpenedNames.FOLDER), it) }

    override fun open(uri: String): InputStream =
        local(uri)?.inputStream()
            ?: context.contentResolver.openInputStream(Uri.parse(uri))
            ?: throw FileNotFoundException(uri)

    override fun replace(uri: String): OutputStream =
        local(uri)?.outputStream()
            ?: context.contentResolver.openOutputStream(Uri.parse(TagFiles.writeUri(uri)), "wt")
            ?: throw FileNotFoundException(uri)

    override fun modifiedMs(uri: String): Long {
        local(uri)?.let { return it.lastModified() }
        val parsed = Uri.parse(uri)
        val media = parsed.authority == MediaStore.AUTHORITY
        val column =
            if (media) MediaStore.MediaColumns.DATE_MODIFIED
            else DocumentsContract.Document.COLUMN_LAST_MODIFIED
        val read =
            try {
                context.contentResolver.query(parsed, arrayOf(column), null, null, null)?.use { cursor ->
                    if (cursor.moveToFirst() && !cursor.isNull(0)) cursor.getLong(0) else null
                }
            } catch (failure: Exception) {
                null
            } ?: return 0L
        return if (media) read * 1000L else read
    }
}

class Recompressing(
    private val context: Context,
    private val library: LibraryRepository,
    private val scope: CoroutineScope,
    private val settings: SharedPreferences,
    private val writable: (Track) -> Boolean,
    private val reread: suspend (MobilePlayer) -> Unit,
) {
    private val mutableTo = MutableStateFlow(RecompressTo.read { settings.getString(it, null) })
    private val mutableRun = MutableStateFlow<RecompressRun?>(null)
    private val busy = AtomicBoolean(false)
    @Volatile private var stopping = false

    val to: StateFlow<RecompressTo> = mutableTo.asStateFlow()
    val run: StateFlow<RecompressRun?> = mutableRun.asStateFlow()

    fun recompressible(track: Track): Boolean = Recompressible.isFlac(track) && writable(track)

    fun choose(to: RecompressTo) {
        mutableTo.value = to
        settings.edit { putString(RecompressTo.KEY, to.stored) }
    }

    fun one(track: Track, consent: suspend (List<Uri>) -> Boolean) = start(consent) { listOf(track) }

    fun every(consent: suspend (List<Uri>) -> Boolean) = start(consent, ::candidates)

    fun stop() {
        stopping = true
    }

    private fun start(consent: suspend (List<Uri>) -> Boolean, chosen: suspend (MobilePlayer) -> List<Track>) {
        if (!busy.compareAndSet(false, true)) return
        stopping = false
        mutableRun.value = RecompressRun(0, 0)
        scope.launch {
            try {
                val songs = library.holding { native -> withContext(Dispatchers.IO) { chosen(native) } }
                val told = over(songs, consent)
                if (told.any { it.outcome is RecompressOutcome.Shrunk }) library.holding { reread(it) }
                RecompressSentences.told(library.words, told, songs.size) {
                    Formatter.formatShortFileSize(context, it)
                }
                    ?.let { notice ->
                        if (notice.failure) library.report(IllegalStateException(notice.text))
                        else library.notice(notice.text)
                    }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                library.report(failure)
            } finally {
                mutableRun.value = null
                busy.set(false)
            }
        }
    }

    private suspend fun over(songs: List<Track>, consent: suspend (List<Uri>) -> Boolean): List<RecompressTold> {
        if (songs.isEmpty()) return emptyList()
        mutableRun.value = RecompressRun(0, songs.size)
        val allowed = WriteConsent.allowed(context, songs.map { it.uri }, consent).toSet()
        val ceiling = to.value.ceiling
        val told = mutableListOf<RecompressTold>()
        for ((at, track) in songs.withIndex()) {
            if (stopping) break
            mutableRun.value = RecompressRun(at, songs.size)
            val title = track.shownTitle(library.words)
            val outcome =
                if (track.uri !in allowed) RecompressOutcome.NotAllowed
                else library.afterScans { library.holding { native -> recompressed(native, track, ceiling) } }
            told += RecompressTold(title, outcome)
            if (outcome == RecompressOutcome.Stopped) break
        }
        return told
    }

    private suspend fun candidates(native: MobilePlayer): List<Track> {
        val found = mutableListOf<Track>()
        var after: SizedSongAfter? = null
        while (true) {
            val page = native.largestSongs(after, CANDIDATES_READ_AT_ONCE.toUInt())
            page.filter { recompressible(it.track) }.mapTo(found) { it.track }
            if (page.size < CANDIDATES_READ_AT_ONCE) return found
            after = page.last().let { SizedSongAfter(it.bytes, it.track.id) }
        }
    }

    private suspend fun recompressed(
        native: MobilePlayer,
        track: Track,
        ceiling: RecompressCeiling,
    ): RecompressOutcome {
        val staged =
            try {
                withContext(Dispatchers.IO) { native.recompressSong(track.id, ceiling) }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                return RecompressOutcome.Failed(failure.message)
            }
        return when (staged) {
            is RecompressStaged.KeptOriginal -> RecompressOutcome.Kept(staged.said)
            is RecompressStaged.Staged ->
                withContext(NonCancellable + Dispatchers.IO) { taken(native, track, staged) }
        }
    }

    private fun taken(native: MobilePlayer, track: Track, staged: RecompressStaged.Staged): RecompressOutcome {
        if (stopping) {
            native.forgetRecompressStage(staged.stagePath)
            return RecompressOutcome.Stopped
        }
        val writing = StageWriting(ResolvedDocuments(context), File(context.filesDir, StageWriting.ASIDE_FOLDER))
        return when (val written = writing.over(track.uri, File(staged.stagePath), track.id)) {
            is StageWritten.Untouched -> {
                native.forgetRecompressStage(staged.stagePath)
                RecompressOutcome.Failed(written.failure.message)
            }
            is StageWritten.PutBack -> {
                native.forgetRecompressStage(staged.stagePath)
                RecompressOutcome.PutBack
            }
            is StageWritten.KeptAside -> {
                native.forgetRecompressStage(staged.stagePath)
                RecompressOutcome.KeptAside(written.aside.name)
            }
            is StageWritten.Written ->
                try {
                    native.songRecompressed(track.id, staged.stagePath, written.modifiedMs)
                    writing.settled(written.aside)
                    RecompressOutcome.Shrunk(staged.before, staged.after)
                } catch (failure: Exception) {
                    native.forgetRecompressStage(staged.stagePath)
                    if (writing.putBack(track.uri, written.aside)) RecompressOutcome.PutBack
                    else RecompressOutcome.KeptAside(written.aside.name)
                }
        }
    }

    private companion object {
        const val CANDIDATES_READ_AT_ONCE = 500
    }
}
