package org.resonate.android

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class BrowsingHostsTest {
    private val androidAutoRelease =
        BrowsingHosts.digestOf("fdb00c43dbde8b51cb312aa81d3b5fa17713adb94b28f598d77f8eb89daceedf")
    private val androidAutoRotated =
        BrowsingHosts.digestOf("1ca8dcc0bed3cbd872d2cb791200c0292ca9975768a82d676b8b424fb65b5295")
    private val googleRelease =
        BrowsingHosts.digestOf("f0fd6c5b410f25cb25c3b53346c8972fae30f8ee7411df910480ad6b2d60db83")
    private val stranger = ByteArray(32) { 7 }

    private fun signedWith(vararg digests: ByteArray): (ByteArray) -> Boolean = { asked ->
        digests.any { it.contentEquals(asked) }
    }

    @Test
    fun androidAutoSignedByGoogleBrowsesUnderEitherOfItsKeys() {
        assertTrue(
            BrowsingHosts.admits("com.google.android.projection.gearhead", signedWith(androidAutoRelease))
        )
        assertTrue(
            BrowsingHosts.admits("com.google.android.projection.gearhead", signedWith(androidAutoRotated))
        )
        assertTrue(
            BrowsingHosts.admits("com.google.android.googlequicksearchbox", signedWith(googleRelease))
        )
    }

    @Test
    fun anAppTakingAHostsNameUnderAnotherKeyIsRefused() {
        for (name in
            listOf(
                "com.google.android.projection.gearhead",
                "com.google.android.googlequicksearchbox",
                "com.google.android.carassistant",
                "com.google.android.autosimulator",
            )) {
            assertFalse(name, BrowsingHosts.admits(name, signedWith(stranger)))
        }
    }

    @Test
    fun oneHostsKeyAdmitsNoOtherHostsName() {
        assertFalse(
            BrowsingHosts.admits("com.google.android.carassistant", signedWith(androidAutoRelease))
        )
        assertFalse(
            BrowsingHosts.admits("com.google.android.projection.gearhead", signedWith(googleRelease))
        )
    }

    @Test
    fun anUnknownPackageIsRefusedWhateverItIsSignedBy() {
        assertFalse(BrowsingHosts.admits("org.stranger.app") { true })
    }

    @Test
    fun aDigestIsReadFromItsHexadecimal() {
        assertArrayEquals(byteArrayOf(0, 15, -1, 16), BrowsingHosts.digestOf("000fff10"))
        assertTrue(androidAutoRelease.size == 32)
    }
}
