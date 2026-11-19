package org.resonate.android

import org.resonate.android.nativecore.SyncControl

object SyncControls {
    private const val EVERYTHING = "everything"
    private const val TRANSPORT = "transport"
    private const val NOTHING = "nothing"

    fun of(stored: String?): SyncControl =
        when (stored) {
            TRANSPORT -> SyncControl.TRANSPORT
            NOTHING -> SyncControl.NOTHING
            else -> SyncControl.EVERYTHING
        }

    fun spelt(control: SyncControl): String =
        when (control) {
            SyncControl.EVERYTHING -> EVERYTHING
            SyncControl.TRANSPORT -> TRANSPORT
            SyncControl.NOTHING -> NOTHING
        }
}
