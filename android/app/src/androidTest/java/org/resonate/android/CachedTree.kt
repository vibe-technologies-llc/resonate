package org.resonate.android

import android.app.UiAutomation
import android.os.Build

fun UiAutomation.forgetTheCachedTree() {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) clearCache()
}
