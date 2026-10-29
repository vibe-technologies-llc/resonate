package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.nativecore.ServedAccount

class ServedTest {
    private fun subsonic(server: String, user: String = "me", password: String = "pw") =
        DownloadAccount(MusicProvider.SUBSONIC, server, user, password, "", "", "")

    @Test
    fun theFirstSubsonicAccountAskedIsTheOneServed() {
        val book =
            ProviderAccountBook()
                .saved(1, emptyAccount(MusicProvider.MONOCHROME))
                .saved(2, subsonic("https://a", "ann", "one"))
                .saved(3, subsonic("https://b", "bob", "two"))

        assertEquals(ServedAccount("https://a", "ann", "one"), book.servedAccount())
    }

    @Test
    fun anAccountSwitchedOffOrIncompleteServesNothing() {
        val off = ProviderAccountBook().saved(1, subsonic("https://a")).turned(1, false)
        val blank = ProviderAccountBook().saved(1, subsonic("https://a", password = ""))

        assertNull(off.servedAccount())
        assertNull(blank.servedAccount())
        assertNull(ProviderAccountBook().servedAccount())
    }

    @Test
    fun aSecondSubsonicAccountIsServedOnceTheFirstIsSwitchedOff() {
        val book =
            ProviderAccountBook()
                .saved(1, subsonic("https://a"))
                .saved(2, subsonic("https://b", "bob", "two"))
                .turned(1, false)

        assertEquals(ServedAccount("https://b", "bob", "two"), book.servedAccount())
    }

    @Test
    fun onlyTheServersSchemeIsAServedMediaId() {
        assertEquals("subsonic:track/42", servedMediaId("subsonic:track/42"))
        assertNull(servedMediaId("12"))
        assertNull(servedMediaId("12@albums"))
        assertNull(BrowseIds.trackOf("subsonic:track/42"))
    }
}
