package org.resonate.android

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

    @Test
    fun aKeystoreThatThrowsLeavesSyncUnsetAndNeverABlankCodeThatWouldMintANewGroup() {
        val said = mutableListOf<Exception>()
        val reading = SyncSettingsReading({ throw KeyStoreException() }, { said += it })

        val read = reading.read()

        assertNull(read)
        assertEquals(1, said.size)
    }

    @Test
    fun aFailedReadIsSaidOnceAndTheSettingsAreTakenOnceTheyCanBeRead() {
        val said = mutableListOf<Exception>()
        var readable = false
        val reading =
            SyncSettingsReading({ if (readable) kept else throw AEADBadTagException() }, { said += it })

        val first = reading.read()
        val again = reading.read()
        readable = true
        val later = reading.read()

        assertNull(first)
        assertNull(again)
        assertEquals(1, said.size)
        assertEquals("KEPT-CODE", later?.key)
    }

    @Test
    fun aReadFailingAgainAfterOneSucceededIsSaidAgain() {
        val said = mutableListOf<Exception>()
        var readable = false
        val reading =
            SyncSettingsReading({ if (readable) kept else throw KeyStoreException() }, { said += it })

        reading.read()
        readable = true
        reading.read()
        readable = false
        reading.read()

        assertEquals(2, said.size)
    }
}
