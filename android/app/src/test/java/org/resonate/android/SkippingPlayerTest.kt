package org.resonate.android

import androidx.media3.common.Player
import java.lang.reflect.Proxy
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SkippingPlayerTest {
    private val sent = mutableListOf<String>()

    private fun wrapping(mediaItems: Int, refusing: MutableList<String> = mutableListOf()) =
        SkippingPlayer(
            Proxy.newProxyInstance(
                Player::class.java.classLoader,
                arrayOf(Player::class.java),
            ) { _, method, _ ->
                when (method.name) {
                    "getMediaItemCount" -> mediaItems
                    "hasNextMediaItem",
                    "hasPreviousMediaItem" -> false
                    else -> {
                        refusing += method.name
                        null
                    }
                }
            } as Player,
            previous = { sent += "previous" },
            next = { sent += "next" },
        )

    @Test
    fun nextAtTheLastRowOfTheShownSliceReachesTheBoundary() {
        val player = wrapping(mediaItems = 3)

        player.seekToNext()
        player.seekToNextMediaItem()

        assertEquals(listOf("next", "next"), sent)
    }

    @Test
    fun previousAtTheFirstRowReachesTheBoundary() {
        val player = wrapping(mediaItems = 3)

        player.seekToPrevious()
        player.seekToPreviousMediaItem()

        assertEquals(listOf("previous", "previous"), sent)
    }

    @Test
    fun noSkipIsPassedOnToTheShownSlicesOwnReckoning() {
        val refusing = mutableListOf<String>()
        val player = wrapping(mediaItems = 3, refusing = refusing)

        player.seekToNext()
        player.seekToPrevious()

        assertTrue(refusing.none { it.startsWith("seekTo") })
    }

    @Test
    fun aStandingQueueAlwaysOffersBothSkips() {
        val standing = wrapping(mediaItems = 1)
        val empty = wrapping(mediaItems = 0)

        assertTrue(standing.hasNextMediaItem())
        assertTrue(standing.hasPreviousMediaItem())
        assertFalse(empty.hasNextMediaItem())
        assertFalse(empty.hasPreviousMediaItem())
    }
}
