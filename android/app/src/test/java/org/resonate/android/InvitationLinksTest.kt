package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test
import org.resonate.android.nativecore.InvitationKind

class InvitationLinksTest {
    @Test
    fun aResonateLinkIsAnInvitationOnlyForAListOrAParty() {
        assertEquals(OpenedLink.INVITATION, InvitationLinks.of("resonate", "shared", "/E-ABCDE"))
        assertEquals(OpenedLink.INVITATION, InvitationLinks.of("resonate", "party", "/ABCDE"))
        assertEquals(OpenedLink.PAIRING, InvitationLinks.of("resonate", "pair", "/ABCDE"))
        assertEquals(OpenedLink.PAIRING, InvitationLinks.of("resonate", null, null))
    }

    @Test
    fun aWebLinkIsAnInvitationOnlyOnTheServersOwnPagesOfItsOwnHost() {
        assertEquals(OpenedLink.INVITATION, InvitationLinks.of("https", RESONATE_WEB_HOST, "/s/"))
        assertEquals(OpenedLink.INVITATION, InvitationLinks.of("HTTPS", "RESONATE.tecnio.dev", "/p/"))
        assertEquals(OpenedLink.SOMETHING_ELSE, InvitationLinks.of("https", RESONATE_WEB_HOST, "/search/"))
        assertEquals(OpenedLink.SOMETHING_ELSE, InvitationLinks.of("https", "example.org", "/p/"))
        assertEquals(OpenedLink.SOMETHING_ELSE, InvitationLinks.of("http", RESONATE_WEB_HOST, "/p/"))
    }

    @Test
    fun aDocumentIsLeftToBePlayed() {
        assertEquals(OpenedLink.SOMETHING_ELSE, InvitationLinks.of("content", "media", "/audio/1"))
    }

    @Test
    fun aPartyLandsOnThePartyAndAListOnThePlaylists() {
        assertEquals(ArrivalPage.PARTY, InvitationLinks.pageFor(InvitationKind.PARTY))
        assertEquals(ArrivalPage.PLAYLISTS, InvitationLinks.pageFor(InvitationKind.PLAYLIST))
    }
}
