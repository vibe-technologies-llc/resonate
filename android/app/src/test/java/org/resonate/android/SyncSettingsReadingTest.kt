package org.resonate.android

import java.io.IOException
import java.security.KeyStoreException
import javax.crypto.AEADBadTagException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.SyncControl

class SyncSettingsReadingTest {
    private val kept =
        SyncConfig(true, false, "", "KEPT-CODE", "", false, false, false, false, SyncControl.EVERYTHING, "", "", true)

    private fun reading(
        stored: () -> SyncConfig,
        said: MutableList<SyncSettingsTrouble> = mutableListOf(),
        written: (SyncConfig) -> Unit = {},
        minted: () -> String = { "MINTED-CODE" },
        forgotten: () -> Unit = {},
    ) = SyncSettingsReading(stored, written, minted, { trouble, _ -> said += trouble }, forgotten)

    @Test
    fun aKeystoreThatThrowsLeavesSyncUnsetAndNeverABlankCodeThatWouldMintANewGroup() {
        val said = mutableListOf<SyncSettingsTrouble>()
        var minted = 0
        val reading = reading({ throw KeyStoreException() }, said, minted = { "NEW-${++minted}" })

        val read = reading.read()

        assertNull(read)
        assertEquals(listOf(SyncSettingsTrouble.UNREAD), said)
        assertEquals(SyncSettingsTrouble.UNREAD, reading.trouble.value)
        assertEquals(0, minted)
    }

    @Test
    fun aFailedReadIsSaidOnceAndTheSettingsAreTakenOnceTheyCanBeRead() {
        val said = mutableListOf<SyncSettingsTrouble>()
        var readable = false
        val reading = reading({ if (readable) kept else throw AEADBadTagException() }, said)

        val first = reading.read()
        val again = reading.read()
        readable = true
        val later = reading.read()

        assertNull(first)
        assertNull(again)
        assertEquals(1, said.size)
        assertEquals("KEPT-CODE", later?.key)
        assertNull(reading.trouble.value)
    }

    @Test
    fun aReadFailingAgainAfterOneSucceededIsSaidAgain() {
        val said = mutableListOf<SyncSettingsTrouble>()
        var readable = false
        val reading = reading({ if (readable) kept else throw KeyStoreException() }, said)

        reading.read()
        readable = true
        reading.read()
        readable = false
        reading.read()

        assertEquals(2, said.size)
    }

    @Test
    fun aBlankCodeIsMintedAndWrittenBeforeTheSettingsAreHandedOn() {
        val written = mutableListOf<SyncConfig>()
        val reading = reading({ written.lastOrNull() ?: kept.copy(key = "") }, written = { written += it })

        val read = reading.read()
        val again = reading.read()

        assertEquals("MINTED-CODE", read?.key)
        assertEquals(listOf(read), written)
        assertEquals(read, again)
    }

    @Test
    fun aMintedCodeThatCannotBeWrittenIsNeverHandedOnAndTheNextReadMintsAndKeepsAnother() {
        val said = mutableListOf<SyncSettingsTrouble>()
        val written = mutableListOf<SyncConfig>()
        var writable = false
        var minted = 0
        val reading =
            reading(
                { written.lastOrNull() ?: kept.copy(key = "") },
                said,
                written = { if (writable) written += it else throw IOException() },
                minted = { "NEW-${++minted}" },
            )

        val refused = reading.read()
        val troubled = reading.trouble.value
        val refusedAgain = reading.read()
        writable = true
        val read = reading.read()
        val afterARelaunch = reading.read()

        assertNull(refused)
        assertNull(refusedAgain)
        assertEquals(SyncSettingsTrouble.CODE_UNKEPT, troubled)
        assertEquals(listOf(SyncSettingsTrouble.CODE_UNKEPT), said)
        assertEquals("NEW-3", read?.key)
        assertEquals(listOf(read), written)
        assertEquals(read?.key, afterARelaunch?.key)
        assertNull(reading.trouble.value)
    }

    @Test
    fun aKeyLostForGoodStartsOverWithItsSealedCodeForgottenAndANewOneMintedAndWritten() {
        val said = mutableListOf<SyncSettingsTrouble>()
        val written = mutableListOf<SyncConfig>()
        var sealed = true
        val reading =
            reading(
                { if (sealed) throw AEADBadTagException() else kept.copy(key = "") },
                said,
                written = { written += it },
                minted = { "NEW-CODE" },
                forgotten = { sealed = false },
            )

        val first = reading.read()
        val unreadableFirst = reading.trouble.value
        val started = reading.startOver()

        assertNull(first)
        assertEquals(SyncSettingsTrouble.UNREAD, unreadableFirst)
        assertEquals("NEW-CODE", started?.key)
        assertEquals(true, started?.enabled)
        assertEquals(listOf(started), written)
        assertNull(reading.trouble.value)
        assertEquals(1, said.size)
    }

    @Test
    fun aStoreThatStillCannotBeReadAfterForgettingStartsNothing() {
        var forgotten = 0
        val reading = reading({ throw KeyStoreException() }, forgotten = { forgotten += 1 })

        val started = reading.startOver()

        assertNull(started)
        assertEquals(1, forgotten)
        assertEquals(SyncSettingsTrouble.UNREAD, reading.trouble.value)
    }

    @Test
    fun aKeptCodeIsNeverMintedOver() {
        val reading = reading({ kept }, written = { throw IOException() })

        val read = reading.read()

        assertEquals(kept, read)
        assertNull(reading.trouble.value)
    }
}
