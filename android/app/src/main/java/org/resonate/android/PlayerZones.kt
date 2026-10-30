package org.resonate.android

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.PlayerZone
import org.resonate.android.nativecore.Snapshot
import org.resonate.android.nativecore.Transport

private const val ZONES_READ_EVERY_TICKS = 8

const val FIRST_ZONE = 1u

object ZonePlan {
    fun deviceFor(route: String, outputs: List<AudioOutput>): Int? =
        if (route.isBlank()) 0 else outputs.firstOrNull { it.route?.id == route }?.id

    fun routeOf(device: Int, outputs: List<AudioOutput>): String =
        outputs.firstOrNull { it.id == device }?.route?.id.orEmpty()

    fun nameFor(title: String, taken: Collection<String>): String {
        if (title !in taken) return title
        return generateSequence(2) { it + 1 }.map { "$title ($it)" }.first { it !in taken }
    }

    fun mayHandOverTo(zone: UInt, shown: UInt, shownHolds: Boolean): Boolean =
        zone != shown && shownHolds

    fun mayRemove(zone: UInt): Boolean = zone != FIRST_ZONE

    fun heardOn(rendererName: String?, device: String): String = rendererName ?: device

    fun line(output: String, transport: Transport, playing: String?): String {
        val doing =
            when (transport) {
                Transport.PLAYING -> playing?.let { "Playing $it" } ?: "Playing"
                Transport.LOADING -> "Opening a song"
                Transport.PAUSED -> playing?.let { "Paused on $it" } ?: "Paused"
                Transport.STOPPED -> "Stopped"
            }
        return "$output · $doing"
    }

    fun due(read: Read?, zone: UInt, revision: ULong, zones: Int, ticks: Int): Boolean =
        read == null ||
            read.zone != zone ||
            read.revision != revision ||
            (zones > 1 && ticks >= ZONES_READ_EVERY_TICKS)

    data class Read(val zone: UInt, val revision: ULong)
}

class PlayerZones(private val library: LibraryRepository) {
    private val mutableListed = MutableStateFlow<List<PlayerZone>>(emptyList())
    val listed = mutableListed.asStateFlow()
    private var read: ZonePlan.Read? = null
    private var ticks = 0

    fun tick(native: MobilePlayer, snapshot: Snapshot) {
        ticks++
        if (!ZonePlan.due(read, snapshot.zone, snapshot.zonesRevision, mutableListed.value.size, ticks)) return
        ticks = 0
        read = ZonePlan.Read(snapshot.zone, snapshot.zonesRevision)
        mutableListed.value = native.zones()
    }

    fun bindRoutes(native: MobilePlayer, outputs: List<AudioOutput>) {
        native.zones().forEach { zone ->
            val device = ZonePlan.deviceFor(zone.route, outputs) ?: return@forEach
            if (device != zone.device) native.selectZoneOutput(zone.id, device, zone.route)
        }
        reread(native)
    }

    fun reread(native: MobilePlayer) {
        mutableListed.value = native.zones()
    }

    fun show(zone: UInt) = changed { it.showZone(zone) }

    fun add(title: String, output: AudioOutput) = changed { native ->
        val name = ZonePlan.nameFor(title, native.zones().map { it.name })
        native.addZone(name, output.id, output.route?.id.orEmpty())
    }

    fun rename(zone: UInt, name: String) = changed { it.renameZone(zone, name) }

    fun remove(zone: UInt) = changed { it.removeZone(zone) }

    fun handOverTo(zone: UInt) = changed { it.handOverTo(zone) }

    fun playOn(zone: UInt, renderer: String?) = changed { it.selectZoneRenderer(zone, renderer) }

    private fun changed(change: (MobilePlayer) -> Unit) {
        library.request { native ->
            try {
                change(native)
            } finally {
                reread(native)
            }
        }
    }
}
