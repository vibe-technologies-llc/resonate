package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.nativecore.ServedService
import org.resonate.android.nativecore.Track

class ServedTest {
    private fun subsonic(server: String, user: String = "me", password: String = "pw") =
        DownloadAccount(MusicProvider.SUBSONIC, server, user, password, "", "", "")

    private fun tidal(refreshToken: String = "token") =
        DownloadAccount(MusicProvider.TIDAL, "", "", "", "client", "", refreshToken)

    private fun hosted(provider: MusicProvider) = emptyAccount(provider)

    private fun named(provider: MusicProvider) = emptyAccount(provider).copy(server = "https://mine")

    private fun track(id: ULong, uri: String, queueId: ULong? = null) =
        Track(
            id = id,
            uri = uri,
            name = "",
            title = "",
            artist = "",
            album = "",
            durationMs = 0uL,
            rate = 0u,
            codec = "",
            favourite = false,
            albumArtist = "",
            genre = "",
            date = "",
            trackNumber = null,
            discNumber = null,
            bits = 0u,
            channels = 0u,
            recording = null,
            release = null,
            artistMbid = null,
            artRevision = 0uL,
            queueId = queueId,
        )

    @Test
    fun everyReadyAccountIsServedInTheBooksOrder() {
        val book =
            ProviderAccountBook()
                .saved(1, hosted(MusicProvider.MONOCHROME))
                .saved(2, subsonic("https://a", "ann", "one"))
                .saved(3, tidal())
                .saved(4, subsonic("https://b", "bob", "two"))

        assertEquals(
            listOf(
                hosted(MusicProvider.MONOCHROME),
                subsonic("https://a", "ann", "one"),
                tidal(),
                subsonic("https://b", "bob", "two"),
            ),
            book.servedAccounts(online = true, hostedServices = true),
        )
    }

    @Test
    fun anAccountSwitchedOffOrNotReadyIsNotServed() {
        val book =
            ProviderAccountBook()
                .saved(1, subsonic("https://a"))
                .saved(2, subsonic("https://b", password = ""))
                .saved(3, tidal(refreshToken = ""))
                .saved(4, subsonic("https://c"))
                .turned(1, false)

        assertEquals(listOf(subsonic("https://c")), book.servedAccounts(online = true, hostedServices = true))
    }

    @Test
    fun nothingIsServedWhileOfflineOrBeforeTheAccountsAreRead() {
        val book = ProviderAccountBook().saved(1, subsonic("https://a"))

        assertEquals(emptyList<DownloadAccount>(), book.servedAccounts(online = false, hostedServices = true))
        assertEquals(emptyList<DownloadAccount>(), null.servedAccounts(online = true, hostedServices = true))
    }

    @Test
    fun declinedHostedServicesLeaveOnlyTheNamedServers() {
        val book =
            ProviderAccountBook()
                .saved(1, hosted(MusicProvider.MONOCHROME))
                .saved(2, hosted(MusicProvider.HIFI_API))
                .saved(3, named(MusicProvider.HIFI_API))
                .saved(4, subsonic("https://a"))

        assertEquals(
            listOf(named(MusicProvider.HIFI_API), subsonic("https://a")),
            book.servedAccounts(online = true, hostedServices = false),
        )
    }

    @Test
    fun everyServicesSchemeIsAServedMediaIdAndNothingElse() {
        listOf(
                "subsonic:track/42",
                "tidal:track/42",
                "resonate:track/42",
                "hifi-api:track/42",
                "monochrome:track/42",
            )
            .forEach { uri ->
                assertEquals(uri, servedMediaId(uri))
                assertNull(BrowseIds.trackOf(uri))
            }
        assertNull(servedMediaId("12"))
        assertNull(servedMediaId("12@albums"))
        assertNull(servedMediaId("content://media/external/audio/1"))
        assertNull(servedMediaId("tidal"))
    }

    @Test
    fun aServedRowHasNoIdAndAServedScheme() {
        val row = track(0uL, "monochrome:track/7", queueId = 3uL)
        val document = track(9uL, "content://songs/9")

        assertTrue(row.served)
        assertFalse(document.served)
        assertFalse(track(0uL, "content://songs/9").served)
        assertEquals("monochrome:track/7", row.pictureKey)
        assertEquals(3uL, row.rowKey)
        assertEquals(9uL, document.rowKey)
    }

    @Test
    fun aServedRowIsDrawnAStarOnlyWhereItsServiceKeepsFavourites() {
        val services =
            listOf(ServedService("subsonic", "Subsonic", true), ServedService("monochrome", "Monochrome", false))

        assertTrue(services.marksFavouritesOf(track(0uL, "subsonic:track/1")))
        assertFalse(services.marksFavouritesOf(track(0uL, "monochrome:track/7")))
        assertFalse(services.marksFavouritesOf(track(0uL, "tidal:track/7")))
        assertFalse(emptyList<ServedService>().marksFavouritesOf(track(0uL, "subsonic:track/1")))
    }
}
