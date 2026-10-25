package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider

class ProviderAccountRecordsTest {
    private val seal = { value: String -> if (value.isEmpty()) "" else "sealed($value)" }
    private val open = { value: String ->
        if (value.isEmpty()) "" else value.removePrefix("sealed(").removeSuffix(")")
    }

    private val legacy: Map<String, Any?> =
        mapOf(
            "provider" to "SUBSONIC",
            "server" to "https://music.example",
            "user" to "sealed(me)",
            "password" to "sealed(pw)",
            "client-id" to "",
            "client-secret" to "",
            "refresh-token" to "",
            "listenbrainz" to "sealed(token)",
            "sync-server" to "https://sync.example",
        )

    private fun subsonic(server: String, user: String, password: String) =
        DownloadAccount(MusicProvider.SUBSONIC, server, user, password, "", "", "")

    private fun stored(base: Map<String, Any?>, changes: Changes): Map<String, Any?> {
        val applied = base.toMutableMap()
        changes.forEach { (name, value) ->
            if (value == null) applied.remove(name) else applied[name] = value
        }
        return applied
    }

    private fun migrated(before: Map<String, Any?>) =
        stored(before, ProviderAccountRecords.migration(before, seal))

    @Test
    fun theSingleAccountOfEarlierVersionsBecomesTheFirstOfTheList() {
        val after = migrated(legacy)

        assertTrue(ProviderAccountRecords.isCurrent(after))
        assertEquals(
            ProviderAccountBook(
                listOf(KeptAccount(1, true, subsonic("https://music.example", "me", "pw")))
            ),
            ProviderAccountRecords.decode(after, open),
        )
    }

    @Test
    fun migrationCopiesTheSealedSecretsVerbatimAndLeavesOtherSettingsAlone() {
        val after = migrated(legacy)

        assertEquals("sealed(me)", after["account-1-user"])
        assertEquals("sealed(pw)", after["account-1-password"])
        assertEquals("sealed(token)", after["listenbrainz"])
        assertEquals("https://sync.example", after["sync-server"])
        listOf("provider", "server", "user", "password", "client-id", "client-secret", "refresh-token")
            .forEach { assertFalse(it, after.containsKey(it)) }
    }

    @Test
    fun migrationKeepsATidalAccountWithItsTokens() {
        val tidal =
            legacy +
                mapOf(
                    "provider" to "TIDAL",
                    "server" to "",
                    "user" to "",
                    "password" to "",
                    "client-id" to "sealed(client)",
                    "client-secret" to "sealed(secret)",
                    "refresh-token" to "sealed(token)",
                )
        val book = ProviderAccountRecords.decode(migrated(tidal), open)

        assertEquals(
            DownloadAccount(MusicProvider.TIDAL, "", "", "", "client", "secret", "token"),
            book.account(1)?.account,
        )
    }

    @Test
    fun migrationReadsAnUnknownProviderNameAsTheDefaultAsEarlierVersionsDid() {
        val book = ProviderAccountRecords.decode(migrated(legacy + ("provider" to "GRAMOPHONE")), open)

        assertEquals(MusicProvider.MONOCHROME, book.account(1)?.account?.provider)
    }

    @Test
    fun aFreshInstallStartsWithTheHostedMonochromeAsEarlierVersionsDid() {
        val after = migrated(mapOf("sync-enabled" to true))
        val book = ProviderAccountRecords.decode(after, open)

        assertEquals(listOf(emptyAccount(MusicProvider.MONOCHROME)), book.asked)
        assertEquals(true, after["sync-enabled"])
    }

    @Test
    fun aMigratedStoreIsNotMigratedAgain() {
        assertTrue(ProviderAccountRecords.isCurrent(migrated(legacy)))
        assertFalse(ProviderAccountRecords.isCurrent(legacy))
    }

    @Test
    fun severalAccountsRoundTripWithTheirSwitchesAndSealedSecrets() {
        val book =
            ProviderAccountBook()
                .saved(1, subsonic("https://a", "u1", "p1"))
                .saved(2, subsonic("https://b", "u2", "p2"))
                .saved(3, DownloadAccount(MusicProvider.HIFI_API, "", "", "key", "", "", ""))
                .turned(2, false)
        val written = stored(emptyMap(), ProviderAccountRecords.encode(book, seal, emptySet()))

        assertEquals(book, ProviderAccountRecords.decode(written, open))
        assertEquals("sealed(p2)", written["account-2-password"])
        assertEquals("false", written["account-2-on"])
    }

    @Test
    fun writingAShorterListDropsTheKeysOfTheAccountsRemoved() {
        val book =
            ProviderAccountBook()
                .saved(1, subsonic("https://a", "u1", "p1"))
                .saved(2, subsonic("https://b", "u2", "p2"))
        val written =
            stored(
                mapOf("sync-key" to "kept"),
                ProviderAccountRecords.encode(book, seal, emptySet()),
            )
        val shorter =
            stored(written, ProviderAccountRecords.encode(book.removed(1), seal, written.keys))

        assertTrue(shorter.keys.none { it.startsWith("account-1-") })
        assertEquals("sealed(p2)", shorter["account-2-password"])
        assertEquals("kept", shorter["sync-key"])
        assertEquals(book.removed(1), ProviderAccountRecords.decode(shorter, open))
    }

    @Test
    fun anEmptyListIsStoredAsEmptyAndIsNotReplacedByTheDefault() {
        val written =
            stored(legacy, ProviderAccountRecords.encode(ProviderAccountBook(), seal, emptySet()))

        assertTrue(ProviderAccountRecords.isCurrent(written))
        assertTrue(ProviderAccountRecords.decode(written, open).kept.isEmpty())
        assertNull(written["account-1-provider"])
    }

    @Test
    fun anAccountWhoseKindIsUnknownIsLeftOutOfTheList() {
        val book = ProviderAccountBook().saved(1, emptyAccount(MusicProvider.MONOCHROME))
        val written = stored(emptyMap(), ProviderAccountRecords.encode(book, seal, emptySet()))

        assertTrue(
            ProviderAccountRecords.decode(written + ("account-1-provider" to "GRAMOPHONE"), open)
                .kept
                .isEmpty()
        )
    }
}
