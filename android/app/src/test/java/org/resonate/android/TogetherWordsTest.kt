package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.ListeningParty
import org.resonate.android.nativecore.PartyConnection
import org.resonate.android.nativecore.PartyFollowing
import org.resonate.android.nativecore.SharedPlaylist
import org.resonate.android.nativecore.SharedPlaylistReach
import org.resonate.android.nativecore.TogetherPerson

class TogetherWordsTest {
    private val party =
        ListeningParty(
            hosting = false,
            link = "resonate://party/ABCDE",
            connection = PartyConnection.HERE,
            people = emptyList(),
            following = PartyFollowing.IN_STEP,
            song = null,
            requests = emptyList(),
            roundTripMs = null,
        )

    private val list =
        SharedPlaylist(
            playlist = 1uL,
            name = "Road Trip",
            mayEdit = true,
            editLink = "edit",
            followLink = "follow",
            people = emptyList(),
            notHeld = emptyList(),
            reach = SharedPlaylistReach.REACHED,
        )

    @Test
    fun aConnectionSaysNothingOnlyWhileHere() {
        assertNull(TogetherWords.connection(PartyConnection.HERE))
        assertEquals("Reaching the server…", TogetherWords.connection(PartyConnection.CONNECTING))
        assertEquals("Lost the server; trying again…", TogetherWords.connection(PartyConnection.RECONNECTING))
        assertEquals("The party is over", TogetherWords.connection(PartyConnection.ENDED))
        assertEquals(
            "That party has ended, or was never begun",
            TogetherWords.connection(PartyConnection.NO_SUCH_PARTY),
        )
        assertEquals("The server turned that link away", TogetherWords.connection(PartyConnection.TURNED_AWAY))
        assertEquals(
            "The server is too old to hold parties",
            TogetherWords.connection(PartyConnection.SERVER_TOO_OLD),
        )
    }

    @Test
    fun aPartyStillReachableIsNotOver() {
        assertFalse(TogetherWords.isOver(PartyConnection.CONNECTING))
        assertFalse(TogetherWords.isOver(PartyConnection.HERE))
        assertFalse(TogetherWords.isOver(PartyConnection.RECONNECTING))
        assertTrue(TogetherWords.isOver(PartyConnection.ENDED))
        assertTrue(TogetherWords.isOver(PartyConnection.NO_SUCH_PARTY))
        assertTrue(TogetherWords.isOver(PartyConnection.TURNED_AWAY))
        assertTrue(TogetherWords.isOver(PartyConnection.SERVER_TOO_OLD))
    }

    @Test
    fun everyStandingOfAGuestHasItsSentence() {
        val said =
            mapOf(
                PartyFollowing.WAITING_FOR_THE_HOST to "Waiting for the host to play something",
                PartyFollowing.FINDING to "Looking for this song",
                PartyFollowing.NOT_HELD to "You don't have this song, so you sit it out",
                PartyFollowing.CANNOT_START_MID_SONG to
                    "This song can't start part way through here, so you join at the next",
                PartyFollowing.SAT_OUT to "Paused here; press play to rejoin",
                PartyFollowing.HOST_PAUSED to "The host has paused",
                PartyFollowing.IN_STEP to "In step with the host",
            )

        said.forEach { (following, sentence) ->
            assertEquals(sentence, TogetherWords.following(following))
        }
        assertEquals(PartyFollowing.entries.size, said.size)
    }

    @Test
    fun aPartysLineSaysTheConnectionFirstThenWhatThisSeatIsDoing() {
        assertEquals("In step with the host", TogetherWords.partyLine(party))
        assertEquals("You are hosting", TogetherWords.partyLine(party.copy(hosting = true)))
        assertEquals(
            "Reaching the server…",
            TogetherWords.partyLine(party.copy(hosting = true, connection = PartyConnection.CONNECTING)),
        )
    }

    @Test
    fun aPersonIsYouTheirNameOrSomeone() {
        val person = TogetherPerson(ByteArray(16), "Ada", isMe = false, isHost = false)

        assertEquals("Ada", TogetherWords.person(person))
        assertEquals("You", TogetherWords.person(person.copy(isMe = true)))
        assertEquals("Someone", TogetherWords.person(person.copy(name = " ")))
    }

    @Test
    fun aListWeMayEditIsSharedAndOneWeMayNotIsFollowed() {
        assertEquals("Shared", TogetherWords.badge(list))
        assertEquals("Followed", TogetherWords.badge(list.copy(mayEdit = false)))
        assertNull(TogetherWords.badge(null))
    }

    @Test
    fun aListReachedSaysNothingOfItsReach() {
        assertNull(TogetherWords.reach(SharedPlaylistReach.REACHED))
        SharedPlaylistReach.entries.filter { it != SharedPlaylistReach.REACHED }.forEach {
            assertTrue(TogetherWords.reach(it).orEmpty().isNotBlank())
        }
    }

    @Test
    fun songsNotHeldAreNamedUpToFiveAndCountedPastThat() {
        assertNull(TogetherWords.notHeld(emptyList()))
        assertEquals("Not in your library: A, B", TogetherWords.notHeld(listOf("A", "B")))
        assertEquals(
            "Not in your library: A, B, C, D, E and 2 more",
            TogetherWords.notHeld(listOf("A", "B", "C", "D", "E", "F", "G")),
        )
    }

    @Test
    fun sharingSaysPlainlyWhatItLacks() {
        assertEquals(
            "Sharing a playlist needs device sync. Turn it on in Devices.",
            TogetherWords.sharingNeedsSync(enabled = false, nearbyOnly = false, online = true),
        )
        assertEquals(
            "Nothing can be shared while sync stays on this network.",
            TogetherWords.sharingNeedsSync(enabled = true, nearbyOnly = true, online = true),
        )
        assertEquals(
            "Sharing a playlist needs online features. Turn them on in Settings.",
            TogetherWords.sharingNeedsSync(enabled = true, nearbyOnly = false, online = false),
        )
        assertNull(TogetherWords.sharingNeedsSync(enabled = true, nearbyOnly = false, online = true))
    }
}
