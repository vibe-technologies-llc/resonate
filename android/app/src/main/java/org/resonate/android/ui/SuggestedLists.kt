package org.resonate.android.ui

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.TrendingUp
import androidx.compose.material.icons.rounded.Album
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.CalendarMonth
import androidx.compose.material.icons.rounded.Category
import androidx.compose.material.icons.rounded.Explore
import androidx.compose.material.icons.rounded.Favorite
import androidx.compose.material.icons.rounded.Groups
import androidx.compose.material.icons.rounded.HighQuality
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.NewReleases
import androidx.compose.material.icons.rounded.Person
import androidx.compose.material.icons.rounded.Repeat
import androidx.compose.material.icons.rounded.Replay
import androidx.compose.material.icons.rounded.StarBorder
import androidx.compose.material.icons.rounded.SurroundSound
import androidx.compose.material.icons.rounded.Timer
import androidx.compose.ui.graphics.vector.ImageVector
import java.util.TimeZone
import kotlinx.serialization.Serializable
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.SavedSearch
import org.resonate.android.nativecore.SearchOrder
import org.resonate.android.nativecore.SuggestedList
import org.resonate.android.nativecore.SuggestionReason
import org.resonate.android.nativecore.SuggestionShelf
import org.resonate.android.ui.navigation.Screen

@Serializable
data class SuggestedSearch(
    val text: String,
    val order: String,
    val descending: Boolean,
    val rows: Int,
) {
    fun native(): SavedSearch =
        SavedSearch(
            text = text,
            order = SearchOrder.valueOf(order),
            descending = descending,
            rows = rows.takeIf { it > 0 }?.toUInt(),
        )

    companion object {
        fun of(search: SavedSearch): SuggestedSearch =
            SuggestedSearch(
                text = search.text,
                order = search.order.name,
                descending = search.descending,
                rows = search.rows?.toInt() ?: 0,
            )
    }
}

val SuggestedList.screen: Screen.Suggested
    get() = Screen.Suggested(name, detail, SuggestedSearch.of(search))

fun MobilePlayer.suggestions(): List<SuggestedList> {
    val zone = TimeZone.getDefault()
    return suggestedLists(zone.id, zone.getOffset(System.currentTimeMillis()) / MILLIS_PER_SECOND)
}

fun List<SuggestedList>.onHome(): List<SuggestedList> =
    filter { it.shelf in HOME_SHELVES }.take(HOME_AT_MOST)

val SuggestionReason.icon: ImageVector
    get() =
        when (this) {
            SuggestionReason.DECADE,
            SuggestionReason.GENRE_OF_A_DECADE -> Icons.Rounded.CalendarMonth
            SuggestionReason.GENRE -> Icons.Rounded.Category
            SuggestionReason.ARTIST -> Icons.Rounded.Person
            SuggestionReason.NEVER_HEARD -> Icons.Rounded.AutoAwesome
            SuggestionReason.MOST_PLAYED -> Icons.AutoMirrored.Rounded.TrendingUp
            SuggestionReason.RECENTLY_ADDED,
            SuggestionReason.FRESH_ARRIVALS -> Icons.Rounded.NewReleases
            SuggestionReason.BACK_CATALOGUE,
            SuggestionReason.OLD_FLAMES,
            SuggestionReason.ARTIST_LEFT_BEHIND -> Icons.Rounded.History
            SuggestionReason.ON_REPEAT -> Icons.Rounded.Repeat
            SuggestionReason.KINDRED_ARTISTS -> Icons.Rounded.Groups
            SuggestionReason.DEEP_CUTS -> Icons.Rounded.Album
            SuggestionReason.ARTIST_TO_DISCOVER -> Icons.Rounded.Explore
            SuggestionReason.UNHEARD_GENRE -> Icons.Rounded.MusicNote
            SuggestionReason.FORGOTTEN_FAVOURITES -> Icons.Rounded.Favorite
            SuggestionReason.SECOND_CHANCES -> Icons.Rounded.Replay
            SuggestionReason.STARRED_UNHEARD -> Icons.Rounded.StarBorder
            SuggestionReason.HI_RES -> Icons.Rounded.HighQuality
            SuggestionReason.LONG_PLAYERS,
            SuggestionReason.SLOW_BURNERS,
            SuggestionReason.SHORT_AND_SHARP -> Icons.Rounded.Timer
            SuggestionReason.SURROUND,
            SuggestionReason.MONO -> Icons.Rounded.SurroundSound
        }

private const val MILLIS_PER_SECOND = 1_000

private const val HOME_AT_MOST = 12

private val HOME_SHELVES =
    setOf(SuggestionShelf.FOR_YOU, SuggestionShelf.REDISCOVER, SuggestionShelf.LISTENING)
