package org.resonate.android

object FindingNotices {
    fun shown(tellsFindings: Boolean, message: String): String? = message.takeIf { tellsFindings }

    fun hidden(count: UInt, fetching: Boolean): String {
        val gone =
            if (count == 1u) "1 song whose file is gone was hidden"
            else "$count songs whose files are gone were hidden"
        return gone + if (fetching) " · fetching from your devices" else ""
    }

    fun kept(landed: Int): String =
        "Kept $landed ${if (landed == 1) "song" else "songs"} from your devices"
}
