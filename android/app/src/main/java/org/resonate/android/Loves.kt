package org.resonate.android

import org.resonate.android.nativecore.FavouritesTold

fun lovesSaid(words: Words, told: FavouritesTold): String? {
    val parts =
        listOfNotNull(
            told.loved
                .takeIf { it > 0u }
                ?.let { words.plural(R.plurals.loves_loved, it.toInt(), it.toInt()) },
            told.takenBack
                .takeIf { it > 0u }
                ?.let { words.plural(R.plurals.loves_taken_back, it.toInt(), it.toInt()) },
            told.refused
                .takeIf { it > 0u }
                ?.let { words.plural(R.plurals.loves_refused, it.toInt(), it.toInt()) },
        )
    if (parts.isEmpty()) return null
    return parts.joinToString(", ").replaceFirstChar { it.uppercase() }
}
