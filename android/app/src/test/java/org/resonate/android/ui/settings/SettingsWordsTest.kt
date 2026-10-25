package org.resonate.android.ui.settings

import java.time.ZoneOffset
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.AudioOutput
import org.resonate.android.nativecore.DevicePlatform
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.EnrichmentStatus
import org.resonate.android.nativecore.EnrichmentStop
import org.resonate.android.nativecore.ListenedOnDay
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.nativecore.ScrobbleAccount
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus
import org.resonate.android.nativecore.SyncTransfer
import org.resonate.android.nativecore.SyncTransferState
import org.resonate.android.nativecore.SyncedDevice

class SettingsWordsTest {
    private fun enrichment(
        running: Boolean = false,
        due: UInt = 0u,
        asked: UInt = 0u,
        stopped: EnrichmentStop? = null,
        matched: UInt = 0u,
        covers: UInt = 0u,
    ) = EnrichmentStatus(running, due, asked, matched, covers, 0u, 0u, 0u, stopped)

    @Test
    fun enrichmentLineSaysWhatThePassIsDoingOrHowItStopped() {
        assertEquals("Not looked up yet", enrichmentLine(null))
        assertEquals("Finding artwork and details", enrichmentLine(enrichment(running = true)))
        assertEquals(
            "Finding artwork and details · 3 of 12",
            enrichmentLine(enrichment(running = true, due = 12u, asked = 3u)),
        )
        assertEquals(
            "Artwork and details up to date",
            enrichmentLine(enrichment(stopped = EnrichmentStop.FINISHED)),
        )
        assertEquals("Stopped: offline", enrichmentLine(enrichment(stopped = EnrichmentStop.OFFLINE)))
        assertEquals(
            "Stopped: MusicBrainz could not be reached",
            enrichmentLine(enrichment(stopped = EnrichmentStop.UNREACHABLE)),
        )
        assertEquals("Not looked up yet", enrichmentLine(enrichment()))
    }

    @Test
    fun enrichmentProgressExistsOnlyWhileARunningPassKnowsItsTotal() {
        assertNull(enrichmentProgress(enrichment(running = true)))
        assertNull(enrichmentProgress(enrichment(due = 4u, asked = 2u)))
        assertEquals(0.5f, enrichmentProgress(enrichment(running = true, due = 4u, asked = 2u)))
        assertEquals(1f, enrichmentProgress(enrichment(running = true, due = 4u, asked = 9u)))
    }

    @Test
    fun enrichmentFoundCountsWhatArrived() {
        assertNull(enrichmentFound(enrichment()))
        assertEquals(
            "1 song matched · 2 covers",
            enrichmentFound(enrichment(matched = 1u, covers = 2u)),
        )
    }

    @Test
    fun coversLineSaysHowManyAlbumsAreAskedAgain() {
        assertEquals("Ask again for albums still without a cover", coversLine(null))
        assertEquals("Asking again for 1 album", coversLine(1u))
        assertEquals("Asking again for 4 albums", coversLine(4u))
    }

    @Test
    fun durationFiguresGrowTheirUnit() {
        val english = Locale.UK

        assertEquals(Figure("0", "minutes"), durationFigure(0uL, english))
        assertEquals(Figure("<1", "minute"), durationFigure(59_999uL, english))
        assertEquals(Figure("1", "minute"), durationFigure(60_000uL, english))
        assertEquals(Figure("59", "minutes"), durationFigure(3_599_999uL, english))
        assertEquals(Figure("1", "hours"), durationFigure(3_600_000uL, english))
        assertEquals(Figure("2.5", "hours"), durationFigure(9_000_000uL, english))
        assertEquals(Figure("2,5", "hours"), durationFigure(9_000_000uL, Locale.GERMANY))
        assertEquals(Figure("1,235", "hours"), durationFigure(1234uL * 3_600_000uL + 1_800_000uL, english))
    }

    @Test
    fun sharesArePercentagesOfTheWhole() {
        assertEquals("0%", percent(0uL, 10uL))
        assertEquals("0%", percent(3uL, 0uL))
        assertEquals("<1%", percent(1uL, 1000uL))
        assertEquals("50%", percent(5uL, 10uL))
        assertEquals(0.25f, shareOf(1uL, 4uL))
        assertEquals(1f, shareOf(9uL, 4uL))
    }

    @Test
    fun theBusiestDayIsTheLongestHeard() {
        val days =
            listOf(
                ListenedOnDay(0uL, 1uL, 100uL),
                ListenedOnDay(86_400uL, 4uL, 900uL),
                ListenedOnDay(172_800uL, 2uL, 300uL),
            )

        assertEquals(86_400uL, busiestDay(days)?.starts)
        assertNull(busiestDay(listOf(ListenedOnDay(0uL, 0uL, 0uL))))
        assertEquals(2, daysShown(days, 2).size)
        assertEquals("2 Jan 1970", dayLabel(86_400uL, ZoneOffset.UTC, Locale.UK))
    }

    @Test
    fun anOutputIsNamedByKindOnlyWhereItsNameSaysNothing() {
        val speaker = AudioOutput(2, "sdk_gphone64", "Built-in speaker", listOf(48_000), emptyList(), emptyList())
        val dac = AudioOutput(7, "FiiO K5", "USB audio", listOf(44_100, 192_000, 96_000), emptyList(), emptyList())

        assertEquals("Built-in speaker", outputTitle(speaker))
        assertEquals("sdk_gphone64", outputSubtitle(speaker))
        assertEquals("FiiO K5", outputTitle(dac))
        assertEquals("USB audio", outputSubtitle(dac))
        assertEquals("44.1–192 kHz", outputRates(dac))
        assertEquals("48 kHz", outputRates(speaker))
        assertEquals("Android chooses for you", outputSubtitle(systemDefaultOutput))
    }

    @Test
    fun clampedSamplesAreToldOnlyOnceSomeWereHeldAtFullScale() {
        assertNull(clampedSamplesWords(0uL))
        assertEquals("1", clampedSamplesWords(1uL))
        assertEquals("18446744073709551615", clampedSamplesWords(ULong.MAX_VALUE))
    }

    @Test
    fun theSystemDefaultLeadsTheOutputsAndUnknownIdsAreSaidPlainly() {
        val dac = AudioOutput(7, "FiiO K5", "USB audio", emptyList(), emptyList(), emptyList())

        assertEquals(listOf(SYSTEM_DEFAULT_OUTPUT, 7), withSystemDefault(listOf(dac)).map { it.id })
        assertEquals("System default", outputNamed(listOf(dac), 0))
        assertEquals("FiiO K5", outputNamed(listOf(dac), 7))
        assertEquals("An unavailable device", outputNamed(listOf(dac), 3))
    }

    @Test
    fun theBufferSnapsToTenMillisecondsWithinItsBounds() {
        assertEquals(50, bufferStep(12f))
        assertEquals(250, bufferStep(254.9f))
        assertEquals(260, bufferStep(255f))
        assertEquals(2000, bufferStep(5000f))
    }

    @Test
    fun aFolderSaysWhatAccessItHas() {
        assertEquals(FolderAccess.DOWNLOADS, folderAccess(true, false, false))
        assertEquals(FolderAccess.UNAVAILABLE, folderAccess(false, false, true))
        assertEquals(FolderAccess.READ_AND_WRITE, folderAccess(false, true, true))
        assertEquals(FolderAccess.READ_ONLY, folderAccess(false, true, false))
        assertTrue(removalText("Music", false).contains("untouched"))
        assertFalse(removalText("Downloads", true).contains("leave your library"))
    }

    @Test
    fun eachProviderAsksOnlyForWhatItUses() {
        assertEquals(listOf(ProviderField.SERVER), providerFields(MusicProvider.MONOCHROME))
        assertEquals(
            listOf(ProviderField.SERVER, ProviderField.USER, ProviderField.PASSWORD),
            providerFields(MusicProvider.SUBSONIC),
        )
        assertEquals(
            listOf(ProviderField.SERVER, ProviderField.PASSWORD),
            providerFields(MusicProvider.HIFI_API),
        )
        assertFalse(ProviderField.SERVER in providerFields(MusicProvider.TIDAL))
        assertEquals("hifi-api", providerName(MusicProvider.HIFI_API))
    }

    @Test
    fun aProviderDraftEditsOneFieldAtATime() {
        val account = DownloadAccount(MusicProvider.SUBSONIC, " https://a ", " me ", "pw", "", "", "")
        val edited = account.with(ProviderField.PASSWORD, "new")

        assertEquals("new", edited.valueOf(ProviderField.PASSWORD))
        assertEquals("https://a", edited.trimmed().server)
        assertEquals("me", edited.trimmed().user)
        assertEquals("SUBSONIC:", accountIdentity(account))
    }

    @Test
    fun lastFmSignInNeedsTheApiAccountAndTheListener() {
        val account = ScrobbleAccount("", "key", "secret", "")

        assertTrue(canSignInToLastFm(ScrobbleDraft(account, user = "me", password = "pw")))
        assertFalse(canSignInToLastFm(ScrobbleDraft(account, user = "me")))
        assertFalse(canSignInToLastFm(ScrobbleDraft(account.copy(lastfmSecret = ""), user = "me", password = "pw")))
    }

    private val config = SyncConfig(true, false, "", "", "", false, false, false, false)

    private fun device(
        name: String,
        online: Boolean = false,
        nearby: Boolean = false,
        here: Boolean = false,
    ) = SyncedDevice(name, name, DevicePlatform.LINUX, online, nearby, here)

    private fun status(standing: SyncStanding, vararg devices: SyncedDevice, owed: UInt = 0u) =
        SyncStatus(standing, "code", devices.toList(), 0u, 0u, 0u, owed, 0uL, true, emptyList())

    @Test
    fun theTransfersHeaderSaysHowManyAreDownloadingAndHowManyWait() {
        assertEquals("Transfers", transfersHeader(0u, 0u))
        assertEquals("Transfers · 2 downloading · 340 waiting", transfersHeader(2u, 340u))
        assertEquals("Transfers · 3 waiting", transfersHeader(0u, 3u))
        assertEquals("Transfers · 1 downloading", transfersHeader(1u, 0u))
    }

    @Test
    fun syncWordsFollowTheStandingAndTheDevicesReached() {
        assertEquals("Off", syncHeadline(null, config.copy(enabled = false), true))
        assertEquals("Waiting for online features", syncHeadline(null, config, false))
        assertEquals("Up to date", syncHeadline(status(SyncStanding.CONNECTED), config, true))
        assertEquals(
            "Sending 2 changes",
            syncHeadline(status(SyncStanding.CONNECTED, owed = 2u), config, true),
        )
        assertEquals(
            "Catching up",
            syncHeadline(status(SyncStanding.NEARBY, device("desk", nearby = true)), config, true),
        )
        assertEquals(
            "1 device online",
            syncStanding(
                status(SyncStanding.CONNECTED, device("me", online = true, here = true), device("desk", online = true)),
                config,
                true,
            ),
        )
        assertEquals(
            "Server unreachable · 1 on this network",
            syncStanding(status(SyncStanding.UNREACHABLE, device("desk", nearby = true)), config, true),
        )
        assertEquals(
            "Looking for devices",
            syncHeadline(status(SyncStanding.NEARBY), config.copy(nearbyOnly = true), false),
        )
    }

    @Test
    fun thisDeviceLeadsThenTheReachableThenByName() {
        val ordered =
            orderedDevices(
                listOf(
                    device("zed"),
                    device("bee", online = true),
                    device("me", here = true),
                    device("ant"),
                    device("cat", nearby = true),
                )
            )

        assertEquals(listOf("me", "bee", "cat", "ant", "zed"), ordered.map { it.name })
        assertEquals("This device", deviceState(ordered[0]))
        assertEquals("On this network", deviceState(ordered[2]))
        assertFalse(mayForget(ordered[0]))
        assertFalse(mayForget(ordered[1]))
        assertTrue(mayForget(ordered[3]))
    }

    @Test
    fun aTransferSaysWhoSendsItAndHowFarItHasCome() {
        val moving =
            SyncTransfer("Song", "Artist", "desk", 1_048_576uL, 4_194_304uL, SyncTransferState.MOVING)

        assertEquals(
            String.format(Locale.getDefault(), "Artist · from desk · %.1f of %.1f MB", 1.0, 4.0),
            transferLine(moving),
        )
        assertEquals(0.25f, transferProgress(moving))
        assertNull(transferProgress(moving.copy(size = 0uL)))
        assertEquals("Waiting", transferLine(moving.copy(artist = "", from = "", state = SyncTransferState.WAITING)))
    }

    @Test
    fun deviceNameAndServerAreUnsavedOnlyWhenTheyDiffer() {
        val stored = config.copy(device = "Phone", server = "https://sync")

        assertFalse(namesUnsaved(stored, " Phone ", "https://sync"))
        assertTrue(namesUnsaved(stored, "Tablet", "https://sync"))
    }

    @Test
    fun aLongCodeBreaksBetweenItsGroups() {
        assertEquals("AAAAA-BBBBB-CCCCC-\nDDDDD-EEEEE", codeLines("AAAAA-BBBBB-CCCCC-DDDDD-EEEEE"))
        assertEquals("ABCD", codeLines("ABCD"))
    }
}
