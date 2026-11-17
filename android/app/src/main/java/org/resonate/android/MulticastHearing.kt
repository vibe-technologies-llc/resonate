package org.resonate.android

import android.content.Context
import android.net.wifi.WifiManager

interface HeldLock {
    val isHeld: Boolean

    fun acquire()

    fun release()
}

class MulticastHearing<E : Any>(private val lock: () -> HeldLock?) {
    private var hearingFor: E? = null

    @Synchronized
    fun begin(engine: E) {
        hearingFor = engine
    }

    @Synchronized
    fun hear(engine: E, wanted: Boolean) {
        if (engine !== hearingFor) return
        val held = lock() ?: return
        if (wanted && !held.isHeld) held.acquire()
        if (!wanted && held.isHeld) held.release()
    }

    @Synchronized
    fun stop() {
        hearingFor = null
        val held = lock() ?: return
        if (held.isHeld) held.release()
    }
}

private class WifiMulticastLock(private val lock: WifiManager.MulticastLock) : HeldLock {
    override val isHeld: Boolean
        get() = lock.isHeld

    override fun acquire() = lock.acquire()

    override fun release() = lock.release()
}

fun <E : Any> multicastHearing(context: Context, tag: String): MulticastHearing<E> {
    val lock: HeldLock? by lazy {
        context.applicationContext
            .getSystemService(WifiManager::class.java)
            ?.createMulticastLock(tag)
            ?.apply { setReferenceCounted(false) }
            ?.let(::WifiMulticastLock)
    }
    return MulticastHearing { lock }
}
