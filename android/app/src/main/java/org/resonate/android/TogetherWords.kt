package org.resonate.android

import org.resonate.android.nativecore.ListeningParty
import org.resonate.android.nativecore.PartyConnection
import org.resonate.android.nativecore.PartyFollowing
import org.resonate.android.nativecore.SharedPlaylist
import org.resonate.android.nativecore.SharedPlaylistReach
import org.resonate.android.nativecore.TogetherPerson

private const val NOT_HELD_NAMED_AT_MOST = 5

object TogetherWords {
    fun connection(words: Words, connection: PartyConnection): String? =
        when (connection) {
            PartyConnection.HERE -> null
            PartyConnection.CONNECTING -> words.string(R.string.together_reaching_the_server)
            PartyConnection.RECONNECTING -> words.string(R.string.together_party_reconnecting)
            PartyConnection.ENDED -> words.string(R.string.together_party_ended)
            PartyConnection.NO_SUCH_PARTY -> words.string(R.string.together_party_no_such)
            PartyConnection.TURNED_AWAY -> words.string(R.string.together_party_turned_away)
            PartyConnection.SERVER_TOO_OLD -> words.string(R.string.together_party_server_too_old)
        }

    fun isOver(connection: PartyConnection): Boolean =
        when (connection) {
            PartyConnection.CONNECTING,
            PartyConnection.HERE,
            PartyConnection.RECONNECTING -> false
            PartyConnection.ENDED,
            PartyConnection.NO_SUCH_PARTY,
            PartyConnection.TURNED_AWAY,
            PartyConnection.SERVER_TOO_OLD -> true
        }

    fun following(words: Words, following: PartyFollowing): String =
        words.string(
            when (following) {
                PartyFollowing.WAITING_FOR_THE_HOST -> R.string.together_following_waiting
                PartyFollowing.FINDING -> R.string.together_following_finding
                PartyFollowing.NOT_HELD -> R.string.together_following_not_held
                PartyFollowing.CANNOT_START_MID_SONG -> R.string.together_following_not_mid_song
                PartyFollowing.SAT_OUT -> R.string.together_following_sat_out
                PartyFollowing.HOST_PAUSED -> R.string.together_following_host_paused
                PartyFollowing.IN_STEP -> R.string.together_following_in_step
            }
        )

    fun partyLine(words: Words, party: ListeningParty): String =
        connection(words, party.connection)
            ?: if (party.hosting) words.string(R.string.together_hosting)
            else following(words, party.following)

    fun roundTrip(words: Words, milliseconds: ULong): String =
        words.string(R.string.together_round_trip, milliseconds.toLong())

    fun person(words: Words, person: TogetherPerson): String =
        when {
            person.isMe -> words.string(R.string.together_person_you)
            person.name.isBlank() -> words.string(R.string.together_person_nameless)
            else -> person.name
        }

    fun reach(words: Words, reach: SharedPlaylistReach): String? =
        when (reach) {
            SharedPlaylistReach.REACHED -> null
            SharedPlaylistReach.WAITING -> words.string(R.string.together_reaching_the_server)
            SharedPlaylistReach.UNREACHED -> words.string(R.string.together_reach_unreached)
            SharedPlaylistReach.SERVER_TOO_OLD ->
                words.string(R.string.together_reach_server_too_old)
            SharedPlaylistReach.TURNED_AWAY -> words.string(R.string.together_reach_turned_away)
            SharedPlaylistReach.NO_SERVER -> words.string(R.string.together_reach_no_server)
        }

    fun badge(words: Words, list: SharedPlaylist?, followed: Boolean = false): String? =
        when {
            list == null -> if (followed) words.string(R.string.together_badge_followed) else null
            list.mayEdit -> words.string(R.string.together_badge_shared)
            else -> words.string(R.string.together_badge_followed)
        }

    fun mayEdit(list: SharedPlaylist?, followed: Boolean): Boolean = list?.mayEdit ?: !followed

    fun notHeld(words: Words, titles: List<String>): String? {
        if (titles.isEmpty()) return null
        val named = titles.take(NOT_HELD_NAMED_AT_MOST).joinToString(", ")
        val more = titles.size - NOT_HELD_NAMED_AT_MOST
        return if (more > 0) words.plural(R.plurals.together_not_held_and_more, more, named, more)
        else words.string(R.string.together_not_held, named)
    }

    fun sharingNeedsSync(
        words: Words,
        enabled: Boolean,
        nearbyOnly: Boolean,
        online: Boolean,
    ): String? =
        when {
            !enabled -> words.string(R.string.together_sharing_needs_sync)
            nearbyOnly -> words.string(R.string.together_sharing_nearby_only)
            !online -> words.string(R.string.together_sharing_needs_online)
            else -> null
        }
}
