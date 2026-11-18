package org.resonate.android

import androidx.media3.common.ForwardingPlayer
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi

@UnstableApi
class SkippingPlayer(
    player: Player,
    private val previous: () -> Unit,
    private val next: () -> Unit,
) : ForwardingPlayer(player) {
    override fun seekToPrevious() = previous()

    override fun seekToPreviousMediaItem() = previous()

    override fun seekToNext() = next()

    override fun seekToNextMediaItem() = next()

    override fun hasPreviousMediaItem(): Boolean = mediaItemCount > 0

    override fun hasNextMediaItem(): Boolean = mediaItemCount > 0
}
