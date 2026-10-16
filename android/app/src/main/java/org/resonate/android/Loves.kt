package org.resonate.android

import org.resonate.android.nativecore.FavouritesTold

fun lovesSaid(told: FavouritesTold): String? {
    val parts =
        listOfNotNull(
            told.loved.takeIf { it > 0u }?.let { "loved ${counted(it, "favourite")}" },
            told.takenBack.takeIf { it > 0u }?.let { "took back ${counted(it, "love")}" },
            told.refused.takeIf { it > 0u }?.let { "${counted(it, "love")} refused" },
        )
    if (parts.isEmpty()) return null
    return parts.joinToString(", ").replaceFirstChar { it.uppercase() }
}

private fun counted(count: UInt, noun: String) = "$count $noun${if (count == 1u) "" else "s"}"
