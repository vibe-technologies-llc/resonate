package org.resonate.android.ui.tools

import kotlin.math.abs
import org.resonate.android.nativecore.LyricDetail
import org.resonate.android.nativecore.LyricFrame
import org.resonate.android.nativecore.LyricSheet
import org.resonate.android.nativecore.LyricWipe

data class WipeSpans(val sung: Int, val blendStart: Int, val blendEnd: Int, val share: Float)

fun wipeSpans(length: Int, wipe: LyricWipe): WipeSpans {
    val sung = wipe.sungUtf16.toInt().coerceIn(0, length)
    val start = (wipe.blend?.startUtf16?.toInt() ?: sung).coerceIn(sung, length)
    val end = (wipe.blend?.endUtf16?.toInt() ?: start).coerceIn(start, length)
    val share = (wipe.blend?.share ?: 0f).coerceIn(0f, 1f)
    return WipeSpans(sung, start, end, if (start == end) 0f else share)
}

enum class LineTone {
    PLAIN,
    LIT,
    WAITING,
    DISTANT,
}

data class LineLook(val tone: LineTone, val spans: WipeSpans?, val breath: Float?)

const val NEAR_LINES = 2

class FrameReading(private val sheet: LyricSheet, private val frame: LyricFrame?, val following: Boolean) {
    val unsynced: Boolean
        get() = sheet.detail == LyricDetail.UNSYNCED

    private val active: Set<Int> = frame?.active.orEmpty().map { it.toInt() }.toSet()

    private val wipes: Map<Int, LyricWipe> = frame?.wipes.orEmpty().associateBy { it.line.toInt() }

    val followed: Int?
        get() =
            if (!following || unsynced) null
            else frame?.readAt?.toInt()?.coerceIn(0, sheet.lines.size)

    private val breathing: Pair<Int, Float>? =
        frame?.waiting
            ?.takeIf { following && !unsynced }
            ?.let { wait -> wait.next.toInt() to wait.through.coerceIn(0f, 1f) }
            ?.takeIf { (next, _) -> sheet.lines.getOrNull(next)?.breathes == true }

    fun look(index: Int): LineLook {
        val row = sheet.lines[index]
        val breath = breathing?.takeIf { it.first == index }?.second
        if (!following || unsynced) return LineLook(LineTone.PLAIN, null, null)
        if (index in active) {
            val spans = wipes[index]?.let { wipeSpans(row.text.length, it) }
            return LineLook(LineTone.LIT, spans, breath)
        }
        val anchor = followed
        val distant = anchor != null && abs(index - anchor) > NEAR_LINES
        return LineLook(if (distant) LineTone.DISTANT else LineTone.WAITING, null, breath)
    }

    fun seekAt(index: Int): Long? =
        if (!following || unsynced) null else sheet.lines.getOrNull(index)?.atMs?.toLong()
}

fun rightVoice(voice: UInt): Boolean = voice == 2u
