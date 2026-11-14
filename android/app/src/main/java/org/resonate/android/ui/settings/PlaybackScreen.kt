package org.resonate.android.ui.settings

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Album
import androidx.compose.material.icons.rounded.History
import androidx.compose.material.icons.rounded.People
import androidx.compose.material.icons.rounded.Repeat
import androidx.compose.material.icons.rounded.Shuffle
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import org.resonate.android.R
import org.resonate.android.Words
import org.resonate.android.nativecore.PlaybackPreferences
import org.resonate.android.nativecore.ShuffleStyle
import org.resonate.android.ui.Resonate
import org.resonate.android.ui.components.Page
import org.resonate.android.ui.components.SectionHeader
import org.resonate.android.ui.resonateViewModel
import org.resonate.android.words

class PlaybackViewModel(resonate: Resonate) : ViewModel() {
    private val library = resonate.library

    val preferences = library.preferences

    private fun change(edit: (PlaybackPreferences) -> PlaybackPreferences) =
        library.updatePreferences(edit)

    fun carryOver(carried: Boolean) = change { it.copy(carriesTheQueueOver = carried) }

    fun skipRepeatsTheQueue(repeats: Boolean) = change { it.copy(skipRepeatsTheQueue = repeats) }

    fun shuffle(style: ShuffleStyle) = change { it.copy(shuffle = style) }

    fun spreadArtists(spread: Boolean) = change { it.copy(shuffleSpreadsArtists = spread) }

    fun spreadAlbums(spread: Boolean) = change { it.copy(shuffleSpreadsAlbums = spread) }

    fun freshFirst(fresh: Boolean) = change { it.copy(shuffleFreshFirst = fresh) }

    fun previousRestarts(restarts: Boolean) = change { it.copy(previousRestarts = restarts) }
}

fun shuffleChoices(words: Words): List<Choice<ShuffleStyle>> =
    listOf(
        Choice(
            ShuffleStyle.RANDOM,
            words.string(R.string.playback_shuffle_random),
            words.string(R.string.playback_shuffle_random_detail),
        ),
        Choice(
            ShuffleStyle.SMART,
            words.string(R.string.playback_shuffle_smart),
            words.string(R.string.playback_shuffle_smart_detail),
        ),
        Choice(
            ShuffleStyle.ALBUMS,
            words.string(R.string.playback_shuffle_albums),
            words.string(R.string.playback_shuffle_albums_detail),
        ),
    )

@Composable
fun PlaybackScreen() {
    val model = resonateViewModel { PlaybackViewModel(it) }
    val preferences by model.preferences.collectAsStateWithLifecycle()
    val words = words()
    Page(stringResource(R.string.settings_playback_title)) { padding ->
        val current = preferences
        if (current == null) {
            Loading(padding)
            return@Page
        }
        SettingsList(padding, Modifier.testTag("playback-list")) {
            item(key = "resuming") { SectionHeader(stringResource(R.string.playback_resuming)) }
            item(key = "carry-over") {
                SwitchRow(
                    title = stringResource(R.string.settings_inner_carry_the_queue_over),
                    icon = Icons.Rounded.History,
                    supporting = stringResource(R.string.playback_carry_over_off),
                    checked = current.carriesTheQueueOver,
                    onCheckedChange = model::carryOver,
                    modifier = Modifier.testTag("carry-over"),
                )
            }
            item(key = "resuming-note") { Footnote(stringResource(R.string.playback_resuming_note)) }
            item(key = "shuffling") { SectionHeader(stringResource(R.string.playback_shuffling)) }
            item(key = "shuffle-style") {
                ChoiceRow(
                    title = stringResource(R.string.settings_inner_shuffle_style),
                    icon = Icons.Rounded.Shuffle,
                    choices = shuffleChoices(words),
                    selected = current.shuffle,
                    onChoose = model::shuffle,
                    modifier = Modifier.testTag("shuffle-style"),
                )
            }
            if (current.shuffle == ShuffleStyle.SMART) {
                item(key = "spread-artists") {
                    SwitchRow(
                        title = stringResource(R.string.playback_spread_artists),
                        icon = Icons.Rounded.People,
                        supporting = stringResource(R.string.playback_spread_artists_detail),
                        checked = current.shuffleSpreadsArtists,
                        onCheckedChange = model::spreadArtists,
                    )
                }
                item(key = "spread-albums") {
                    SwitchRow(
                        title = stringResource(R.string.playback_spread_albums),
                        icon = Icons.Rounded.Album,
                        supporting = stringResource(R.string.playback_spread_albums_detail),
                        checked = current.shuffleSpreadsAlbums,
                        onCheckedChange = model::spreadAlbums,
                    )
                }
                item(key = "fresh-first") {
                    SwitchRow(
                        title = stringResource(R.string.playback_fresh_first),
                        icon = Icons.AutoMirrored.Rounded.QueueMusic,
                        supporting = stringResource(R.string.playback_fresh_first_detail),
                        checked = current.shuffleFreshFirst,
                        onCheckedChange = model::freshFirst,
                    )
                }
            }
            item(key = "repeating") { SectionHeader(stringResource(R.string.playback_repeating)) }
            item(key = "skip-repeats") {
                SwitchRow(
                    title = stringResource(R.string.settings_inner_skip_repeats_the_queue),
                    icon = Icons.Rounded.Repeat,
                    supporting = stringResource(R.string.playback_skip_repeats_off),
                    checked = current.skipRepeatsTheQueue,
                    onCheckedChange = model::skipRepeatsTheQueue,
                    modifier = Modifier.testTag("skip-repeats"),
                )
            }
            item(key = "previous") { SectionHeader(stringResource(R.string.playback_previous)) }
            item(key = "previous-restarts") {
                SwitchRow(
                    title = stringResource(R.string.settings_inner_previous_restarts),
                    icon = Icons.Rounded.SkipPrevious,
                    supporting = stringResource(R.string.playback_previous_restarts_off),
                    checked = current.previousRestarts,
                    onCheckedChange = model::previousRestarts,
                    modifier = Modifier.testTag("previous-restarts"),
                )
            }
        }
    }
}
