package org.resonate.android

import android.content.Context
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import org.junit.runner.RunWith
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.nativecore.ScrobbleAccount

@RunWith(AndroidJUnit4::class)
class ProviderAccountStoreTest {
    @Test
    fun credentialsSurviveReopeningWithoutPlaintextPreferences() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val storage = context.getSharedPreferences("provider-accounts", Context.MODE_PRIVATE)
        storage.edit().clear().commit()
        try {
            val book =
                ProviderAccountBook()
                    .saved(
                        1L,
                        DownloadAccount(
                            MusicProvider.SUBSONIC,
                            "https://music.invalid",
                            "fixture-user",
                            "fixture-password",
                            "fixture-client",
                            "fixture-secret",
                            "fixture-refresh",
                        ),
                    )
                    .saved(
                        2L,
                        DownloadAccount(
                            MusicProvider.SUBSONIC,
                            "https://other.invalid",
                            "fixture-second-user",
                            "fixture-second-password",
                            "",
                            "",
                            "",
                        ),
                    )
                    .turned(2L, false)
            val scrobbling =
                ScrobbleAccount(
                    "fixture-listenbrainz",
                    "fixture-lastfm",
                    "fixture-application",
                    "fixture-session",
                )
            ProviderAccountStore(context).apply {
                writeAccounts(book)
                writeScrobbling(scrobbling)
                writeRecognitionKey("fixture-acoustid")
            }
            ProviderAccountStore(context).apply {
                assertEquals(book, readAccounts())
                assertEquals(scrobbling, readScrobbling())
                assertEquals("fixture-acoustid", readRecognitionKey())
            }
            storage.all
                .filterKeys { !it.endsWith("-server") && !it.endsWith("-provider") }
                .values
                .forEach { value ->
                    assertFalse(value.toString().contains("fixture"))
                }
        } finally {
            storage.edit().clear().commit()
        }
    }
}
