package org.resonate.android

import android.util.Log
import org.resonate.android.nativecore.LogLevel
import org.resonate.android.nativecore.Logbook
import org.resonate.android.nativecore.keepLog

object Logcat : Logbook {
    private const val TAG = "ResonateNative"

    fun install() {
        keepLog(this)
    }

    override fun write(level: LogLevel, line: String) {
        when (level) {
            LogLevel.ERROR -> Log.e(TAG, line)
            LogLevel.WARN -> Log.w(TAG, line)
            LogLevel.INFO -> Log.i(TAG, line)
            LogLevel.DEBUG -> Log.d(TAG, line)
        }
    }
}
