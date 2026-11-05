package org.resonate.android

import org.resonate.android.nativecore.ListeningParty
import org.resonate.android.nativecore.PartyConnection
import org.resonate.android.nativecore.PartyFollowing
import org.resonate.android.nativecore.SharedPlaylist
import org.resonate.android.nativecore.SharedPlaylistReach
import org.resonate.android.nativecore.TogetherPerson

private const val YOU = "You"
private const val A_NAMELESS_PERSON = "Someone"
private const val NOT_HELD_NAMED_AT_MOST = 5

object TogetherWords {
    const val SHARED = "Shared"
    const val FOLLOWED = "Followed"
    const val NOT_AN_INVITATION = "That is not a Resonate link"
    const val LINK_NOT_OPENED = "That link could not be opened"
    const val JOIN_THIS_PARTY_ASKS = "Join this listening party?"
    const val JOIN_THIS_PARTY_SAYS =
        "A link asks this phone to stop what it plays and follow someone's party instead. Join only if you were invited."
    const val NOTHING_TO_PASTE = "The clipboard holds no Resonate link"
    const val NOT_FOUND_TO_PLAY_NEXT = "That song could not be found here"

    fun connection(connection: PartyConnection): String? =
        when (connection) {
            PartyConnection.HERE -> null
            PartyConnection.CONNECTING -> "Reaching the server…"
            PartyConnection.RECONNECTING -> "Lost the server; trying again…"
            PartyConnection.ENDED -> "The party is over"
            PartyConnection.NO_SUCH_PARTY -> "That party has ended, or was never begun"
            PartyConnection.TURNED_AWAY -> "The server turned that link away"
            PartyConnection.SERVER_TOO_OLD -> "The server is too old to hold parties"
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

    fun following(following: PartyFollowing): String =
        when (following) {
            PartyFollowing.WAITING_FOR_THE_HOST -> "Waiting for the host to play something"
            PartyFollowing.FINDING -> "Looking for this song"
            PartyFollowing.NOT_HELD -> "You don't have this song, so you sit it out"
            PartyFollowing.CANNOT_START_MID_SONG ->
                "This song can't start part way through here, so you join at the next"
            PartyFollowing.SAT_OUT -> "Paused here; press play to rejoin"
            PartyFollowing.HOST_PAUSED -> "The host has paused"
            PartyFollowing.IN_STEP -> "In step with the host"
        }

    fun partyLine(party: ListeningParty): String =
        connection(party.connection)
            ?: if (party.hosting) "You are hosting" else following(party.following)

    fun roundTrip(milliseconds: ULong): String = "The server answers in $milliseconds ms"

    fun person(person: TogetherPerson): String =
        when {
            person.isMe -> YOU
            person.name.isBlank() -> A_NAMELESS_PERSON
            else -> person.name
        }

    fun reach(reach: SharedPlaylistReach): String? =
        when (reach) {
            SharedPlaylistReach.REACHED -> null
            SharedPlaylistReach.WAITING -> "Reaching the server…"
            SharedPlaylistReach.UNREACHED ->
                "The server cannot be reached. Edits are kept and go when it can."
            SharedPlaylistReach.SERVER_TOO_OLD -> "The server is too old to keep shared playlists."
            SharedPlaylistReach.TURNED_AWAY -> "The server turned this playlist away."
            SharedPlaylistReach.NO_SERVER ->
                "This device is set to use no server, so nothing can be shared."
        }

    fun badge(list: SharedPlaylist?, followed: Boolean = false): String? =
        when {
            list == null -> if (followed) FOLLOWED else null
            list.mayEdit -> SHARED
            else -> FOLLOWED
        }

    fun mayEdit(list: SharedPlaylist?, followed: Boolean): Boolean = list?.mayEdit ?: !followed

    fun notHeld(titles: List<String>): String? {
        if (titles.isEmpty()) return null
        val named = titles.take(NOT_HELD_NAMED_AT_MOST).joinToString(", ")
        val more = titles.size - NOT_HELD_NAMED_AT_MOST
        return if (more > 0) "Not in your library: $named and $more more"
        else "Not in your library: $named"
    }

    fun sharingNeedsSync(enabled: Boolean, nearbyOnly: Boolean, online: Boolean): String? =
        when {
            !enabled -> "Sharing a playlist needs device sync. Turn it on in Devices."
            nearbyOnly -> "Nothing can be shared while sync stays on this network."
            !online -> "Sharing a playlist needs online features. Turn them on in Settings."
            else -> null
        }
}
