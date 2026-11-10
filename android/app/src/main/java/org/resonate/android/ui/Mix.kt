package org.resonate.android.ui

import androidx.annotation.StringRes
import kotlinx.serialization.Serializable
import org.resonate.android.R
import org.resonate.android.nativecore.TrackFilter
import org.resonate.android.nativecore.TrackOrder
import org.resonate.android.nativecore.defaultTrackFilter

@Serializable
enum class Mix(@StringRes val title: Int, @StringRes val subtitle: Int) {
    NEVER_HEARD(R.string.settings_inner_never_heard, R.string.mix_never_heard_subtitle),
    MOST_PLAYED(R.string.settings_inner_most_played, R.string.mix_most_played_subtitle),
    RECENTLY_ADDED(R.string.settings_inner_recently_added, R.string.mix_recently_added_subtitle),
    HIGH_RESOLUTION(R.string.mix_high_resolution, R.string.mix_high_resolution_subtitle);

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
