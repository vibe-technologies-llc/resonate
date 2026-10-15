package org.resonate.android

import android.appwidget.AppWidgetHost
import android.appwidget.AppWidgetHostView
import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Intent
import android.view.View
import android.widget.ImageButton
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])
@RunWith(AndroidJUnit4::class)
class WidgetRenderingTest {
    @Test
    fun bothWidgetsBindRenderAndKeepAccessibleTransportTargets() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        val manager = AppWidgetManager.getInstance(context)
        val host = AppWidgetHost(context, 724)
        val providers =
            manager.installedProviders.filter {
                it.provider.packageName == context.packageName
            }
        assertEquals(2, providers.size)
        try {
            for (provider in providers) {
                val id = host.allocateAppWidgetId()
                instrumentation.uiAutomation.adoptShellPermissionIdentity(
                    "android.permission.BIND_APPWIDGET"
                )
                try {
                    assertTrue(manager.bindAppWidgetIdIfAllowed(id, provider.provider))
                } finally {
                    instrumentation.uiAutomation.dropShellPermissionIdentity()
                }
                lateinit var view: AppWidgetHostView
                instrumentation.runOnMainSync {
                    host.startListening()
                    view = host.createView(context, id, provider)
                }
                context.sendBroadcast(
                    Intent(AppWidgetManager.ACTION_APPWIDGET_UPDATE)
                        .setComponent(
                            ComponentName(context.packageName, provider.provider.className)
                        )
                        .putExtra(AppWidgetManager.EXTRA_APPWIDGET_IDS, intArrayOf(id))
                )
                val deadline = System.nanoTime() + 10_000_000_000L
                var rendered = false
                while (!rendered && System.nanoTime() < deadline) {
                    instrumentation.runOnMainSync {
                        rendered =
                            view
                                .findViewById<ImageButton>(R.id.widget_play_pause)
                                ?.hasOnClickListeners() == true
                    }
                    if (!rendered) Thread.sleep(50)
                }
                assertTrue("The launcher can inflate the widget", rendered)
                instrumentation.runOnMainSync {
                    val density = context.resources.displayMetrics.density
                    view.measure(
                        View.MeasureSpec.makeMeasureSpec(
                            (360 * density).toInt(),
                            View.MeasureSpec.EXACTLY,
                        ),
                        View.MeasureSpec.makeMeasureSpec(
                            (220 * density).toInt(),
                            View.MeasureSpec.EXACTLY,
                        ),
                    )
                    view.layout(0, 0, view.measuredWidth, view.measuredHeight)
                    for (control in
                        listOf(R.id.widget_prev, R.id.widget_play_pause, R.id.widget_next)) {
                        val button = requireNotNull(view.findViewById<ImageButton>(control))
                        assertTrue(button.contentDescription.isNotBlank())
                        assertTrue(button.measuredWidth >= 48 * density)
                        assertTrue(button.hasOnClickListeners())
                    }
                }
                host.deleteAppWidgetId(id)
            }
        } finally {
            instrumentation.runOnMainSync { host.stopListening() }
            host.deleteHost()
        }
    }
}
