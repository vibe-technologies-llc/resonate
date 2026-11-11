package org.resonate.android

import android.content.pm.PackageManager

internal object BrowsingHosts {
    private val SIGNED_BY: Map<String, Set<String>> =
        mapOf(
            "com.google.android.projection.gearhead" to
                setOf(
                    "fdb00c43dbde8b51cb312aa81d3b5fa17713adb94b28f598d77f8eb89daceedf",
                    "1ca8dcc0bed3cbd872d2cb791200c0292ca9975768a82d676b8b424fb65b5295",
                ),
            "com.google.android.googlequicksearchbox" to
                setOf("f0fd6c5b410f25cb25c3b53346c8972fae30f8ee7411df910480ad6b2d60db83"),
            "com.google.android.carassistant" to
                setOf("74b6fbf710e8d90d44d340125889b42306a62c4379d0e5a66220e3a68abf90e2"),
            "com.google.android.autosimulator" to
                setOf("1975b2f17177bc89a5dff31f9e64a6cae281a53dc1d1d59b1d147fe1c82afa00"),
        )

    fun admits(packageName: String, signedBy: (ByteArray) -> Boolean): Boolean =
        SIGNED_BY[packageName].orEmpty().any { signedBy(digestOf(it)) }

    fun digestOf(hex: String): ByteArray =
        hex.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
}

internal fun PackageManager.isBrowsingHost(uid: Int, packageName: String): Boolean =
    packageName in getPackagesForUid(uid).orEmpty() &&
        BrowsingHosts.admits(packageName) {
            hasSigningCertificate(packageName, it, PackageManager.CERT_INPUT_SHA256)
        }
