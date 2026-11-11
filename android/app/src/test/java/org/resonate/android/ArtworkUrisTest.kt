package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ArtworkUrisTest {
    private val authority = ArtworkUris.authority("org.resonate.android")

    @Test
    fun aTrackUriReadsBackAsItsTrack() {
        assertEquals(42uL, ArtworkUris.trackOf(authority, ArtworkUris.uri(authority, 42uL)))
    }

    @Test
    fun anotherAuthorityOrAGarbledPathNamesNoTrack() {
        assertNull(ArtworkUris.trackOf(authority, ArtworkUris.uri("org.other.art", 42uL)))
        assertNull(ArtworkUris.trackOf(authority, "content://$authority/track/forty"))
        assertNull(ArtworkUris.trackOf(authority, "content://$authority/track/"))
        assertNull(ArtworkUris.trackOf(authority, "content://$authority/album/42"))
        assertNull(ArtworkUris.trackOf(authority, "file:///track/42"))
    }

    @Test
    fun theAuthorityFollowsThePackage() {
        assertEquals("org.resonate.android.art", authority)
    }

    @Test
    fun onlyTheAppTheBrowsingHostsAndAMediaControllerMayReadACover() {
        assertTrue(ArtworkReaders.mayRead(true, emptyList(), false))
        assertTrue(
            ArtworkReaders.mayRead(false, listOf("com.google.android.projection.gearhead"), false)
        )
        assertTrue(ArtworkReaders.mayRead(false, listOf("com.android.systemui"), true))
        assertFalse(ArtworkReaders.mayRead(false, listOf("org.stranger.app"), false))
        assertFalse(ArtworkReaders.mayRead(false, emptyList(), false))
    }
}
