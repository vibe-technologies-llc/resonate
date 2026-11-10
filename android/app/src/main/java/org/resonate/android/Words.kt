package org.resonate.android

import android.content.res.Resources
import androidx.annotation.PluralsRes
import androidx.annotation.StringRes
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalResources

interface Words {
    fun string(@StringRes id: Int, vararg args: Any): String

    fun plural(@PluralsRes id: Int, count: Int, vararg args: Any): String
}

class ResourceWords(private val resources: Resources) : Words {
    override fun string(id: Int, vararg args: Any): String =
        if (args.isEmpty()) resources.getString(id) else resources.getString(id, *args)

    override fun plural(id: Int, count: Int, vararg args: Any): String =
        if (args.isEmpty()) resources.getQuantityString(id, count)
        else resources.getQuantityString(id, count, *args)
}

@Composable
fun words(): Words {
    val resources = LocalResources.current
    return remember(resources) { ResourceWords(resources) }
}
