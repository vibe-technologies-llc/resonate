package org.resonate.android

import org.resonate.android.nativecore.Track

internal fun sameSongs(before: List<Track>, after: List<Track>): Boolean =
    before === after ||
        before.size == after.size &&
            before.indices.all { index ->
                val was = before[index]
                val now = after[index]
                was == now || was.copy(favourite = now.favourite) == now
            }
