package org.resonate.android

import android.content.Context
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import org.resonate.android.nativecore.OutputFormat

data class OutputRoute(val id: String, val name: String)

private const val ROUTE_FORM = "v1"

private val namedByKind = setOf("Built-in speaker", "Earpiece")

fun outputName(kind: String, productName: String): String =
    if (kind in namedByKind) kind else productName.ifBlank { kind }

private fun routeKind(type: Int): String =
    when (type) {
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER,
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER_SAFE -> "speaker"
        AudioDeviceInfo.TYPE_BUILTIN_EARPIECE -> "earpiece"
        AudioDeviceInfo.TYPE_WIRED_HEADPHONES -> "wired-headphones"
        AudioDeviceInfo.TYPE_WIRED_HEADSET -> "wired-headset"
        AudioDeviceInfo.TYPE_BLUETOOTH_A2DP,
        AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
        AudioDeviceInfo.TYPE_BLE_HEADSET,
        AudioDeviceInfo.TYPE_BLE_SPEAKER,
        AudioDeviceInfo.TYPE_BLE_BROADCAST,
        AudioDeviceInfo.TYPE_HEARING_AID -> "bluetooth"
        AudioDeviceInfo.TYPE_USB_DEVICE,
        AudioDeviceInfo.TYPE_USB_HEADSET,
        AudioDeviceInfo.TYPE_USB_ACCESSORY -> "usb"
        AudioDeviceInfo.TYPE_HDMI,
        AudioDeviceInfo.TYPE_HDMI_ARC,
        AudioDeviceInfo.TYPE_HDMI_EARC -> "hdmi"
        else -> "type-$type"
    }

fun outputRouteOf(type: Int, address: String, productName: String, title: String): OutputRoute {
    val kind = routeKind(type)
    val which = if (kind == "bluetooth" && address.isNotBlank()) address else productName
    return OutputRoute("$ROUTE_FORM:$kind:$which", title)
}

fun routeInUse(outputs: List<AudioOutput>, actual: Int, expected: List<Int>): OutputRoute? {
    fun routeOf(id: Int) = outputs.firstOrNull { it.id == id }?.route
    return routeOf(actual) ?: expected.firstNotNullOfOrNull(::routeOf)
}

fun audioOutputs(manager: AudioManager): List<AudioOutput> =
    manager.getDevices(AudioManager.GET_DEVICES_OUTPUTS).map { device ->
        val kind = outputKind(device.type)
        val name = device.productName.toString()
        AudioOutput(
            device.id,
            name,
            kind,
            device.sampleRates.toList(),
            device.channelCounts.toList(),
            device.encodings.toList(),
            outputRouteOf(device.type, device.address, name, outputName(kind, name)),
        )
    }

fun deviceInUse(outputs: List<AudioOutput>, actual: Int, expected: List<Int>): Int? =
    outputs.firstOrNull { it.id == actual }?.id
        ?: expected.firstOrNull { id -> outputs.any { it.id == id } }

class OutputWatch(
    private val context: Context,
    private val offered: (Int, List<OutputFormat>) -> Unit,
    private val changed: (OutputRoute?) -> Unit,
) {
    private var registered: AudioDeviceCallback? = null
    @Volatile private var actual = 0

    private val manager: AudioManager?
        get() = context.getSystemService(AudioManager::class.java)

    fun routeNow(): OutputRoute? {
        val manager = manager ?: return null
        val expected = manager.getAudioDevicesForAttributes(mediaAttributes()).map { it.id }
        return routeInUse(audioOutputs(manager), actual, expected)
    }

    private fun offersNow() {
        val manager = manager ?: return
        val expected = manager.getAudioDevicesForAttributes(mediaAttributes()).map { it.id }
        val device = deviceInUse(audioOutputs(manager), actual, expected)
        val info =
            device?.let { id ->
                manager.getDevices(AudioManager.GET_DEVICES_OUTPUTS).firstOrNull { it.id == id }
            }
        offered(device ?: 0, info?.let { bitPerfectFormats(manager, it) }.orEmpty())
    }

    @Synchronized
    fun start() {
        if (registered != null) return
        val callback =
            object : AudioDeviceCallback() {
                override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>) =
                    follow()

                override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>) =
                    follow()
            }
        manager?.registerAudioDeviceCallback(callback, Handler(Looper.getMainLooper()))
        registered = callback
        follow()
    }

    @Synchronized
    fun stop() {
        registered?.let { manager?.unregisterAudioDeviceCallback(it) }
        registered = null
        actual = 0
    }

    fun streamOn(device: Int) {
        if (device == actual) return
        actual = device
        follow()
    }

    private fun follow() {
        offersNow()
        changed(routeNow())
    }
}
