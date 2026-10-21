package org.resonate.android.ui

import kotlinx.serialization.Serializable
import org.resonate.android.nativecore.TrackFilter
import org.resonate.android.nativecore.TrackOrder
import org.resonate.android.nativecore.defaultTrackFilter

@Serializable
enum class Mix(val title: String, val subtitle: String) {
    NEVER_HEARD("Never heard", "There is always something new to find."),
    MOST_PLAYED("Most played", "The soundtrack to your days."),
    RECENTLY_ADDED("Recently added", "Fresh arrivals in your library."),
    HIGH_RESOLUTION("High resolution", "A little more detail.");

    fun filter(): TrackFilter =
        when (this) {
            NEVER_HEARD -> defaultTrackFilter().copy(unplayed = true)
            MOST_PLAYED -> defaultTrackFilter().copy(order = TrackOrder.PLAYS, descending = true)
            RECENTLY_ADDED ->
                defaultTrackFilter().copy(order = TrackOrder.ADDED, descending = true)
            HIGH_RESOLUTION -> defaultTrackFilter().copy(minimumRate = HIGH_RESOLUTION_RATE)
        }
}

private const val HIGH_RESOLUTION_RATE = 88_200u
