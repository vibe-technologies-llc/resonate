package org.resonate.android

import java.io.IOException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class CoverAnswersTest {
    private class Pipe {
        var sent: ByteArray? = null
        var refusals = 0

        fun send(jpeg: ByteArray) {
            sent = jpeg
        }

        fun refuse() {
            refusals++
        }
    }

    @Test
    fun aCoverFoundIsSentAndTheEndNotRefused() = runBlocking {
        val pipe = Pipe()

        CoverAnswers.answer({ byteArrayOf(1, 2, 3) }, pipe::send, pipe::refuse)

        assertEquals(listOf<Byte>(1, 2, 3), pipe.sent?.toList())
        assertEquals(0, pipe.refusals)
    }

    @Test
    fun noCoverRefusesTheEnd() = runBlocking {
        val pipe = Pipe()

        CoverAnswers.answer({ null }, pipe::send, pipe::refuse)

        assertNull(pipe.sent)
        assertEquals(1, pipe.refusals)
    }

    @Test
    fun anEngineClosingMidReadStillRefusesTheEnd() = runBlocking {
        val pipe = Pipe()

        CoverAnswers.answer({ throw EngineClosed() }, pipe::send, pipe::refuse)

        assertEquals(1, pipe.refusals)
    }

    @Test
    fun anyFailureOfTheReadRefusesTheEnd() = runBlocking {
        val pipe = Pipe()

        CoverAnswers.answer({ throw IllegalStateException() }, pipe::send, pipe::refuse)

        assertEquals(1, pipe.refusals)
    }

    @Test
    fun aReaderGoneMidSendLeavesTheEndClosedOnce() = runBlocking {
        val pipe = Pipe()

        CoverAnswers.answer(
            { byteArrayOf(1) },
            { throw IOException() },
            pipe::refuse,
        )

        assertEquals(1, pipe.refusals)
    }

    @Test
    fun anEngineNobodyOpensIsWaitedForNoLongerThanTheBound() = runBlocking {
        val pipe = Pipe()
        val never = CompletableDeferred<ByteArray?>()

        CoverAnswers.answer({ never.await() }, pipe::send, pipe::refuse, within = 50L)

        assertNull(pipe.sent)
        assertEquals(1, pipe.refusals)
    }
}
