package org.resonate.android

import android.content.res.Configuration
import org.resonate.android.nativecore.speakLanguages

object SpokenLanguages {
    fun tags(configuration: Configuration): List<String> {
        val locales = configuration.locales
        return (0 until locales.size()).map { locales[it].toLanguageTag() }
    }

    fun tell(configuration: Configuration): String = speakLanguages(tags(configuration))
}
