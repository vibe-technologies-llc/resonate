package org.resonate.android

object FindingNotices {
    fun shown(tellsFindings: Boolean, message: String): String? = message.takeIf { tellsFindings }

    fun hidden(words: Words, count: UInt, fetching: Boolean): String =
        words.plural(
            if (fetching) R.plurals.finding_hidden_fetching else R.plurals.finding_hidden,
            count.toInt(),
            count.toInt(),
        )

    fun kept(words: Words, landed: Int): String =
        words.plural(R.plurals.finding_kept, landed, landed)
}
