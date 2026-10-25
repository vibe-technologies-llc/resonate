package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider

class ProviderAccountBookTest {
    private fun subsonic(server: String, user: String = "me", password: String = "pw") =
        DownloadAccount(MusicProvider.SUBSONIC, server, user, password, "", "", "")

    private val tidal = DownloadAccount(MusicProvider.TIDAL, "", "", "", "client", "", "token")

    @Test
    fun severalAccountsOfOneKindAreKeptInTheOrderTheyWereAdded() {
        val book =
            ProviderAccountBook()
                .saved(1, subsonic("https://a"))
                .saved(2, subsonic("https://b"))
                .saved(3, emptyAccount(MusicProvider.MONOCHROME))

        assertEquals(listOf(1L, 2L, 3L), book.kept.map { it.id })
        assertEquals(listOf("https://a", "https://b", ""), book.asked.map { it.server })
        assertEquals(4L, book.nextId())
    }

    @Test
    fun savingAnExistingIdReplacesTheAccountAndKeepsItsSwitch() {
        val book =
            ProviderAccountBook()
                .saved(1, subsonic("https://a"))
                .turned(1, false)
                .saved(1, subsonic("https://changed"))

        assertEquals(1, book.kept.size)
        assertEquals("https://changed", book.account(1)?.account?.server)
        assertFalse(book.account(1)?.on == true)
    }

    @Test
    fun anAccountSwitchedOffOrNotReadyIsNotAsked() {
        val book =
            ProviderAccountBook()
                .saved(1, subsonic("https://a"))
                .saved(2, subsonic("https://b"))
                .saved(3, subsonic(""))
                .saved(4, subsonic("https://d", password = ""))
                .turned(2, false)

        assertEquals(listOf("https://a"), book.asked.map { it.server })
        assertTrue(book.ready)
        assertFalse(book.turned(1, false).ready)
    }

    @Test
    fun readinessFollowsWhatEachKindNeeds() {
        assertEquals(Readiness.READY, emptyAccount(MusicProvider.MONOCHROME).readiness())
        assertEquals(Readiness.READY, emptyAccount(MusicProvider.HIFI_API).readiness())
        assertEquals(Readiness.MISSING_SERVER, emptyAccount(MusicProvider.SUBSONIC).readiness())
        assertEquals(Readiness.MISSING_LOGIN, subsonic("https://a", user = " ").readiness())
        assertEquals(Readiness.MISSING_CLIENT, emptyAccount(MusicProvider.TIDAL).readiness())
        assertEquals(
            Readiness.NEEDS_SIGN_IN,
            emptyAccount(MusicProvider.TIDAL).copy(clientId = "client").readiness(),
        )
        assertEquals(Readiness.READY, tidal.readiness())
    }

    @Test
    fun removingAnAccountLeavesTheOthers() {
        val book =
            ProviderAccountBook().saved(1, subsonic("https://a")).saved(2, subsonic("https://b"))
        val removed = book.removed(1)

        assertNull(removed.account(1))
        assertEquals(listOf(2L), removed.kept.map { it.id })
        assertTrue(removed.removed(2).kept.isEmpty())
        assertFalse(removed.removed(2).ready)
    }

    @Test
    fun onlyOneTidalAccountIsKeptSinceARenewedTokenCannotSayWhichItBelongsTo() {
        val book = ProviderAccountBook().saved(1, tidal)

        assertFalse(book.accepts(MusicProvider.TIDAL))
        assertTrue(book.accepts(MusicProvider.SUBSONIC))
        assertEquals(book, book.saved(2, tidal.copy(refreshToken = "other")))
        assertTrue(book.removed(1).accepts(MusicProvider.TIDAL))
    }

    @Test
    fun aRenewedTokenReachesTheTidalAccountAlone() {
        val book = ProviderAccountBook().saved(1, subsonic("https://a")).saved(2, tidal)
        val renewed = book.renewed("fresh")

        assertEquals("fresh", renewed.account(2)?.account?.refreshToken)
        assertEquals(book.account(1), renewed.account(1))
    }
}
