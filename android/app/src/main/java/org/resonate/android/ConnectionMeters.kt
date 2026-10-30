package org.resonate.android

import org.resonate.android.nativecore.ConnectionMeter

object ConnectionMeters {
    fun of(notMetered: Boolean?): ConnectionMeter =
        when (notMetered) {
            null -> ConnectionMeter.UNKNOWN
            true -> ConnectionMeter.UNMETERED
            false -> ConnectionMeter.METERED
        }
}
