package org.resonate.android

import java.io.IOException
import java.security.KeyStoreException
import java.util.concurrent.CountDownLatch
import javax.crypto.AEADBadTagException
import kotlin.concurrent.thread
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.SyncControl

class SyncSettingsKeepingTest {
    private val kept =
        SyncConfig(true, false, "", "KEPT-CODE", "", false, false, false, false, SyncControl.EVERYTHING, "", "", true)

    private fun keeping(
        stored: () -> SyncConfig,
        said: MutableList<SyncSettingsTrouble> = mutableListOf(),
        written: (SyncConfig) -> Unit = {},
        minted: () -> String = { "MINTED-CODE" },
        forgotten: () -> Unit = {},
        unsaved: (Exception) -> Unit = {},
    ) = SyncSettingsKeeping(stored, written, minted, { trouble, _ -> said += trouble }, unsaved, forgotten)

    private fun blockedOnTheKeeper(waiting: Thread) {
        val deadline = System.nanoTime() + 5_000_000_000L
        while (waiting.state != Thread.State.BLOCKED) {
            check(System.nanoTime() < deadline) { "never waited on the keeper" }
            Thread.sleep(1)
        }
    }

    @Test
    fun aKeystoreThatThrowsLeavesSyncUnsetAndNeverABlankCodeThatWouldMintANewGroup() {
        val said = mutableListOf<SyncSettingsTrouble>()
        var minted = 0
        val keeping = keeping({ throw KeyStoreException() }, said, minted = { "NEW-${++minted}" })

        val read = keeping.read()

        assertNull(read)
        assertEquals(listOf(SyncSettingsTrouble.UNREAD), said)
        assertEquals(SyncSettingsTrouble.UNREAD, keeping.trouble.value)
        assertEquals(0, minted)
    }

    @Test
    fun aFailedReadIsSaidOnceAndTheSettingsAreTakenOnceTheyCanBeRead() {
        val said = mutableListOf<SyncSettingsTrouble>()
        var readable = false
        val keeping = keeping({ if (readable) kept else throw AEADBadTagException() }, said)

        val first = keeping.read()
        val again = keeping.read()
        readable = true
        val later = keeping.read()

        assertNull(first)
        assertNull(again)
        assertEquals(1, said.size)
        assertEquals("KEPT-CODE", later?.key)
        assertNull(keeping.trouble.value)
    }

    @Test
    fun settingsOnceReadAreHeldAndNeverReadOverAgain() {
        val said = mutableListOf<SyncSettingsTrouble>()
        var readable = true
        var reads = 0
        val keeping =
            keeping({ if (readable) kept.also { reads += 1 } else throw KeyStoreException() }, said)

        val first = keeping.read()
        readable = false
        val again = keeping.read()

        assertEquals(kept, first)
        assertEquals(kept, again)
        assertEquals(1, reads)
        assertEquals(emptyList<SyncSettingsTrouble>(), said)
    }

    @Test
    fun aBlankCodeIsMintedAndWrittenBeforeTheSettingsAreHandedOn() {
        val written = mutableListOf<SyncConfig>()
        val keeping = keeping({ written.lastOrNull() ?: kept.copy(key = "") }, written = { written += it })

        val read = keeping.read()
        val again = keeping.read()

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
        val keeping =
            keeping(
                { written.lastOrNull() ?: kept.copy(key = "") },
                said,
                written = { if (writable) written += it else throw IOException() },
                minted = { "NEW-${++minted}" },
            )

        val refused = keeping.read()
        val troubled = keeping.trouble.value
        val refusedAgain = keeping.read()
        writable = true
        val read = keeping.read()
        val afterARelaunch = keeping.read()

        assertNull(refused)
        assertNull(refusedAgain)
        assertEquals(SyncSettingsTrouble.CODE_UNKEPT, troubled)
        assertEquals(listOf(SyncSettingsTrouble.CODE_UNKEPT), said)
        assertEquals("NEW-3", read?.key)
        assertEquals(listOf(read), written)
        assertEquals(read?.key, afterARelaunch?.key)
        assertNull(keeping.trouble.value)
    }

    @Test
    fun aKeyLostForGoodStartsOverWithItsSealedCodeForgottenAndANewOneMintedAndWritten() {
        val said = mutableListOf<SyncSettingsTrouble>()
        val written = mutableListOf<SyncConfig>()
        var sealed = true
        val keeping =
            keeping(
                { if (sealed) throw AEADBadTagException() else kept.copy(key = "") },
                said,
                written = { written += it },
                minted = { "NEW-CODE" },
                forgotten = { sealed = false },
            )

        val first = keeping.read()
        val unreadableFirst = keeping.trouble.value
        val started = keeping.startOver()

        assertNull(first)
        assertEquals(SyncSettingsTrouble.UNREAD, unreadableFirst)
        assertEquals("NEW-CODE", started?.key)
        assertEquals(true, started?.enabled)
        assertEquals(listOf(started), written)
        assertNull(keeping.trouble.value)
        assertEquals(1, said.size)
    }

    @Test
    fun aStoreThatStillCannotBeReadAfterForgettingStartsNothing() {
        var forgotten = 0
        val keeping = keeping({ throw KeyStoreException() }, forgotten = { forgotten += 1 })

        val started = keeping.startOver()

        assertNull(started)
        assertEquals(1, forgotten)
        assertEquals(SyncSettingsTrouble.UNREAD, keeping.trouble.value)
    }

    @Test
    fun aKeptCodeIsNeverMintedOver() {
        val keeping = keeping({ kept }, written = { throw IOException() })

        val read = keeping.read()

        assertEquals(kept, read)
        assertNull(keeping.trouble.value)
    }

    @Test
    fun twoRapidChangesReachTheStoreAndTheServiceInOrderAndTheLastWinsOnBoth() = runBlocking {
        val written = mutableListOf<SyncConfig>()
        val served = mutableListOf<SyncConfig>()
        val serving = CompletableDeferred<Unit>()
        val release = CompletableDeferred<Unit>()
        val keeping = keeping({ kept }, written = { written += it })
        keeping.read()

        keeping.ask { it.copy(favourites = true) }
        val first = keeping.keepAsked()
        val firstApplied =
            launch(Dispatchers.Default) {
                keeping.applying(first) {
                    served += it
                    serving.complete(Unit)
                    release.await()
                }
            }
        serving.await()
        keeping.ask { it.copy(favourites = false, playlists = true) }
        val second = keeping.keepAsked()
        val secondApplied = launch(Dispatchers.Default) { keeping.applying(second) { served += it } }
        release.complete(Unit)
        joinAll(firstApplied, secondApplied)
        keeping.applying(first) { served += it }

        val one = kept.copy(favourites = true)
        val two = kept.copy(favourites = false, playlists = true)
        assertEquals(listOf(one, two), written)
        assertEquals(listOf(one, two), served)
        assertEquals(two, keeping.config.value)
    }

    @Test
    fun changesAskedInARowAreKeptInTheOrderAskedWhicheverWorkerTakesThem() {
        val written = mutableListOf<SyncConfig>()
        val keeping = keeping({ kept }, written = { written += it })
        keeping.read()

        keeping.ask { it.copy(favourites = true) }
        keeping.ask { it.copy(favourites = false) }
        val workers = List(2) { thread { keeping.keepAsked() } }
        workers.forEach(Thread::join)

        assertEquals(listOf(kept.copy(favourites = true), kept), written)
        assertEquals(kept, keeping.config.value)
    }

    @Test
    fun aFailedWriteLeavesTheSavedSettingsShownAndTheServiceOnThem() = runBlocking {
        val written = mutableListOf<SyncConfig>()
        val served = mutableListOf<SyncConfig>()
        val unsaved = mutableListOf<Exception>()
        var writable = true
        val keeping =
            keeping(
                { kept },
                written = { if (writable) written += it else throw IllegalStateException() },
                unsaved = { unsaved += it },
            )
        keeping.read()

        keeping.ask { it.copy(favourites = true) }
        val saved = keeping.keepAsked()
        keeping.applying(saved) { served += it }
        writable = false
        keeping.ask { it.copy(playlists = true, server = "elsewhere.example") }
        val refused = keeping.keepAsked()
        val shownAfterTheFailure = keeping.config.value
        keeping.applying(null) { served += it }
        writable = true
        keeping.ask { it.copy(everything = true) }
        keeping.keepAsked()

        val savedConfig = kept.copy(favourites = true)
        assertNull(refused)
        assertEquals(1, unsaved.size)
        assertEquals(savedConfig, shownAfterTheFailure)
        assertEquals(listOf(savedConfig, savedConfig), served)
        assertEquals(listOf(savedConfig, savedConfig.copy(everything = true)), written)
    }

    @Test
    fun aChangeAskedWhileStartupMintsTheCodeIsKeptOnTopOfItAndNeverWrittenOver() {
        var store = kept.copy(key = "")
        val written = mutableListOf<SyncConfig>()
        val minting = CountDownLatch(1)
        val mint = CountDownLatch(1)
        val keeping =
            keeping(
                { store },
                written = {
                    store = it
                    written += it
                },
                minted = {
                    minting.countDown()
                    mint.await()
                    "MINTED-CODE"
                },
            )

        val startup = thread { keeping.read() }
        minting.await()
        val change = thread {
            keeping.ask { it.copy(favourites = true) }
            keeping.keepAsked()
        }
        blockedOnTheKeeper(change)
        mint.countDown()
        startup.join()
        change.join()
        val reopened = keeping.read()

        val canonical = kept.copy(key = "MINTED-CODE")
        val changed = canonical.copy(favourites = true)
        assertEquals(listOf(canonical, changed), written)
        assertEquals(changed, store)
        assertEquals(changed, keeping.config.value)
        assertEquals(changed, reopened)
    }

    @Test
    fun aStartupReadWhileAChangeIsBeingWrittenWaitsForItAndShowsIt() {
        var store = kept
        val writing = CountDownLatch(1)
        val write = CountDownLatch(1)
        val keeping =
            keeping(
                { store },
                written = {
                    writing.countDown()
                    write.await()
                    store = it
                },
            )
        keeping.read()

        val change = thread {
            keeping.ask { it.copy(nearbyOnly = true) }
            keeping.keepAsked()
        }
        writing.await()
        var reopened: SyncConfig? = null
        val startup = thread { reopened = keeping.read() }
        blockedOnTheKeeper(startup)
        write.countDown()
        change.join()
        startup.join()

        val changed = kept.copy(nearbyOnly = true)
        assertEquals(changed, reopened)
        assertEquals(changed, keeping.config.value)
        assertEquals(changed, store)
    }
}
