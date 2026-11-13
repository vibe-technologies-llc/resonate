package org.resonate.android

import org.resonate.android.nativecore.ListeningParty
import org.resonate.android.nativecore.PartyConnection
import org.resonate.android.nativecore.PartyChatLine
import org.resonate.android.nativecore.PartyClockApart
import org.resonate.android.nativecore.PartyFollowing
import org.resonate.android.nativecore.PartyHandingOver
import org.resonate.android.nativecore.PartyReactionKind
import org.resonate.android.nativecore.PartySaying
import org.resonate.android.nativecore.SharedPlaylist
import org.resonate.android.nativecore.SharedPlaylistReach
import org.resonate.android.nativecore.TogetherPerson

private const val NOT_HELD_NAMED_AT_MOST = 5

enum class SharingBlock {
    SYNC_OFF,
    NEARBY_ONLY,
    ONLINE_OFF,
}

object TogetherWords {
    fun connection(words: Words, connection: PartyConnection): String? =
        when (connection) {
            PartyConnection.HERE -> null
            PartyConnection.CONNECTING -> words.string(R.string.together_reaching_the_server)
            PartyConnection.RECONNECTING -> words.string(R.string.together_party_reconnecting)
            PartyConnection.ENDED -> words.string(R.string.together_party_ended)
            PartyConnection.HANDED_OVER -> words.string(R.string.together_party_handed_over)
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
            PartyConnection.HANDED_OVER,
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

    fun apart(words: Words, apart: PartyClockApart): String =
        words.string(
            if (apart.corrected) R.string.together_apart_corrected else R.string.together_apart,
            String.format(java.util.Locale.ROOT, "%+.1f", apart.nanos / 1_000_000.0),
        )

    fun reactionDrawn(reaction: PartyReactionKind): String =
        when (reaction) {
            PartyReactionKind.LOVE -> drawn(0x2764, 0xFE0F)
            PartyReactionKind.FIRE -> drawn(0x1F525)
            PartyReactionKind.LAUGH -> drawn(0x1F602)
            PartyReactionKind.CLAP -> drawn(0x1F44F)
            PartyReactionKind.DANCE -> drawn(0x1F483)
            PartyReactionKind.WOW -> drawn(0x1F62E)
        }

    private fun drawn(vararg codePoints: Int): String = String(codePoints, 0, codePoints.size)

    fun reactionNamed(words: Words, reaction: PartyReactionKind): String =
        words.string(
            when (reaction) {
                PartyReactionKind.LOVE -> R.string.party_reaction_love
                PartyReactionKind.FIRE -> R.string.party_reaction_fire
                PartyReactionKind.LAUGH -> R.string.party_reaction_laugh
                PartyReactionKind.CLAP -> R.string.party_reaction_clap
                PartyReactionKind.DANCE -> R.string.party_reaction_dance
                PartyReactionKind.WOW -> R.string.party_reaction_wow
            }
        )

    fun saidBy(words: Words, line: PartyChatLine): String =
        when {
            line.isMe -> words.string(R.string.together_person_you)
            line.name.isBlank() -> words.string(R.string.together_person_nameless)
            else -> line.name
        }

    fun lineHeard(words: Words, line: PartyChatLine): String =
        when (val saying = line.saying) {
            is PartySaying.Words -> words.string(R.string.party_said_by, saidBy(words, line), saying.text)
            is PartySaying.Reaction ->
                words.string(
                    R.string.party_reacted_with,
                    saidBy(words, line),
                    reactionNamed(words, saying.reaction),
                )
        }

    fun handing(words: Words, handing: PartyHandingOver): String {
        fun named(name: String) = name.ifBlank { words.string(R.string.together_person_nameless) }
        return when (handing) {
            is PartyHandingOver.Handing ->
                words.string(R.string.party_handing_to, named(handing.name))
            is PartyHandingOver.NotTaken ->
                words.string(R.string.party_not_taken, named(handing.name))
        }
    }

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

    fun sharingBlock(enabled: Boolean, nearbyOnly: Boolean, online: Boolean): SharingBlock? =
        when {
            !enabled -> SharingBlock.SYNC_OFF
            nearbyOnly -> SharingBlock.NEARBY_ONLY
            !online -> SharingBlock.ONLINE_OFF
            else -> null
        }

    fun sharingNeeds(words: Words, block: SharingBlock): String =
        words.string(
            when (block) {
                SharingBlock.SYNC_OFF -> R.string.together_sharing_needs_sync
                SharingBlock.NEARBY_ONLY -> R.string.together_sharing_nearby_only
                SharingBlock.ONLINE_OFF -> R.string.together_sharing_needs_online
            }
        )

    fun sharingNeedsSync(
        words: Words,
        enabled: Boolean,
        nearbyOnly: Boolean,
        online: Boolean,
    ): String? = sharingBlock(enabled, nearbyOnly, online)?.let { sharingNeeds(words, it) }
}
