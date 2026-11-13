package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
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
            apart = null,
            chat = emptyList(),
            handing = null,
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
        assertNull(TogetherWords.connection(English, PartyConnection.HERE))
        assertEquals("Reaching the server…", TogetherWords.connection(English, PartyConnection.CONNECTING))
        assertEquals("Lost the server; trying again…", TogetherWords.connection(English, PartyConnection.RECONNECTING))
        assertEquals("The party is over", TogetherWords.connection(English, PartyConnection.ENDED))
        assertEquals(
            "You handed the party over. It goes on without you",
            TogetherWords.connection(English, PartyConnection.HANDED_OVER),
        )
        assertEquals(
            "That party has ended, or was never begun",
            TogetherWords.connection(English, PartyConnection.NO_SUCH_PARTY),
        )
        assertEquals("The server turned that link away", TogetherWords.connection(English, PartyConnection.TURNED_AWAY))
        assertEquals(
            "The server is too old to hold parties",
            TogetherWords.connection(English, PartyConnection.SERVER_TOO_OLD),
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
            assertEquals(sentence, TogetherWords.following(English, following))
        }
        assertEquals(PartyFollowing.entries.size, said.size)
    }

    @Test
    fun aPartysLineSaysTheConnectionFirstThenWhatThisSeatIsDoing() {
        assertEquals("In step with the host", TogetherWords.partyLine(English, party))
        assertEquals("You are hosting", TogetherWords.partyLine(English, party.copy(hosting = true)))
        assertEquals(
            "Reaching the server…",
            TogetherWords.partyLine(English, party.copy(hosting = true, connection = PartyConnection.CONNECTING)),
        )
    }

    @Test
    fun theGapFromTheHostsClockIsSaidSignedToATenthAndWhetherItWasPutRight() {
        assertEquals(
            "Your clock was -30.0 ms off the host's and is put right",
            TogetherWords.apart(English, PartyClockApart(-30_040_000L, true)),
        )
        assertEquals(
            "Your clock is +1.3 ms off the host's",
            TogetherWords.apart(English, PartyClockApart(1_260_000L, false)),
        )
    }

    @Test
    fun aLineInTheChatIsHeardAsWhoSaidWhatAndEveryReactionIsDrawnAndNamed() {
        val line = PartyChatLine(1uL, "Ada", isMe = false, saying = PartySaying.Words("Turn it up"))

        assertEquals("Ada: Turn it up", TogetherWords.lineHeard(English, line))
        assertEquals("You: Turn it up", TogetherWords.lineHeard(English, line.copy(isMe = true)))
        assertEquals(
            "Someone reacted with fire",
            TogetherWords.lineHeard(English, line.copy(name = "", saying = PartySaying.Reaction(PartyReactionKind.FIRE))),
        )
        assertEquals(
            PartyReactionKind.entries.size,
            PartyReactionKind.entries.map { TogetherWords.reactionDrawn(it) }.toSet().size,
        )
        PartyReactionKind.entries.forEach { assertTrue(TogetherWords.reactionNamed(English, it).isNotBlank()) }
    }

    @Test
    fun aHandingSaysWhoTheHostIsHandingToAndWhoNeverTookIt() {
        assertEquals(
            "Handing the party to Ada…",
            TogetherWords.handing(English, PartyHandingOver.Handing("Ada")),
        )
        assertEquals(
            "Someone did not take the party over; their app may need an update",
            TogetherWords.handing(English, PartyHandingOver.NotTaken("")),
        )
        assertTrue(TogetherWords.isOver(PartyConnection.HANDED_OVER))
    }

    @Test
    fun aPersonIsYouTheirNameOrSomeone() {
        val person = TogetherPerson(ByteArray(16), "Ada", isMe = false, isHost = false)

        assertEquals("Ada", TogetherWords.person(English, person))
        assertEquals("You", TogetherWords.person(English, person.copy(isMe = true)))
        assertEquals("Someone", TogetherWords.person(English, person.copy(name = " ")))
    }

    @Test
    fun aListWeMayEditIsSharedAndOneWeMayNotIsFollowed() {
        assertEquals("Shared", TogetherWords.badge(English, list))
        assertEquals("Followed", TogetherWords.badge(English, list.copy(mayEdit = false)))
        assertNull(TogetherWords.badge(English, null))
    }

    @Test
    fun aFollowedListIsToldFromTheOthersWithSyncOffToo() {
        assertEquals("Followed", TogetherWords.badge(English, null, followed = true))
        assertNull(TogetherWords.badge(English, null, followed = false))
        assertEquals("Shared", TogetherWords.badge(English, list, followed = true))
        assertEquals(true, TogetherWords.mayEdit(null, followed = false))
        assertEquals(false, TogetherWords.mayEdit(null, followed = true))
        assertEquals(true, TogetherWords.mayEdit(list, followed = true))
        assertEquals(false, TogetherWords.mayEdit(list.copy(mayEdit = false), followed = false))
    }

    @Test
    fun aListTheCatalogFlagsSharedStaysInItsShareWhileSharingIsStopped() {
        assertTrue(TogetherWords.inShare(list, sharedHere = false))
        assertTrue(TogetherWords.inShare(null, sharedHere = true))
        assertEquals(false, TogetherWords.inShare(null, sharedHere = false))
    }

    @Test
    fun aListReachedSaysNothingOfItsReach() {
        assertNull(TogetherWords.reach(English, SharedPlaylistReach.REACHED))
        SharedPlaylistReach.entries.filter { it != SharedPlaylistReach.REACHED }.forEach {
            assertTrue(TogetherWords.reach(English, it).orEmpty().isNotBlank())
        }
    }

    @Test
    fun songsNotHeldAreNamedUpToFiveAndCountedPastThat() {
        assertNull(TogetherWords.notHeld(English, emptyList()))
        assertEquals("Not in your library: A, B", TogetherWords.notHeld(English, listOf("A", "B")))
        assertEquals(
            "Not in your library: A, B, C, D, E and 2 more",
            TogetherWords.notHeld(English, listOf("A", "B", "C", "D", "E", "F", "G")),
        )
    }

    @Test
    fun sharingSaysPlainlyWhatItLacks() {
        assertEquals(
            "Sharing a playlist needs device sync. Turn it on in Devices.",
            TogetherWords.sharingNeedsSync(English, enabled = false, nearbyOnly = false, online = true),
        )
        assertEquals(
            "Nothing can be shared while sync stays on this network.",
            TogetherWords.sharingNeedsSync(English, enabled = true, nearbyOnly = true, online = true),
        )
        assertEquals(
            "Sharing a playlist needs online features. Turn them on in Settings.",
            TogetherWords.sharingNeedsSync(English, enabled = true, nearbyOnly = false, online = false),
        )
        assertNull(TogetherWords.sharingNeedsSync(English, enabled = true, nearbyOnly = false, online = true))
    }

    @Test
    fun whatBlocksSharingIsNamedSoTheButtonGoesWhereTheSentenceSays() {
        assertEquals(
            SharingBlock.SYNC_OFF,
            TogetherWords.sharingBlock(enabled = false, nearbyOnly = true, online = false),
        )
        assertEquals(
            SharingBlock.NEARBY_ONLY,
            TogetherWords.sharingBlock(enabled = true, nearbyOnly = true, online = false),
        )
        assertEquals(
            SharingBlock.ONLINE_OFF,
            TogetherWords.sharingBlock(enabled = true, nearbyOnly = false, online = false),
        )
        assertNull(TogetherWords.sharingBlock(enabled = true, nearbyOnly = false, online = true))
    }
}
