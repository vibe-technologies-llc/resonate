package org.resonate.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class StoredChoicesTest {
    private fun stored(vararg held: Pair<String, Boolean>): (String) -> Boolean? = mapOf(*held)::get

    @Test
    fun aCopyOfMusicbrainzIsTakenUnlessChosenOffWhateverTheFirstSwitchSaved() {
        assertTrue(StoredChoices.musicbrainzCopy(stored()))
        assertTrue(StoredChoices.musicbrainzCopy(stored("sync-musicbrainz-copy" to false)))
        assertTrue(StoredChoices.musicbrainzCopy(stored("sync-musicbrainz-copy-chosen" to true)))
        assertFalse(StoredChoices.musicbrainzCopy(stored("sync-musicbrainz-copy-chosen" to false)))
        assertFalse(
            StoredChoices.musicbrainzCopy(
                stored("sync-musicbrainz-copy" to true, "sync-musicbrainz-copy-chosen" to false),
            ),
        )
    }
}
