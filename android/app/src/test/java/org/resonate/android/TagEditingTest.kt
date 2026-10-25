package org.resonate.android

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.SongTags

class TagEditingTest {
    @Test
    fun aMixedFieldLeftBlankIsUnchangedAndAnEmptiedOneIsWrittenEmpty() {
        val mixed = setOf(TagName.Genre)

        val left = TagDiff.changed(emptyMap(), emptyMap(), mixed)
        val emptied = TagDiff.changed(emptyMap(), emptyMap(), mixed, setOf(TagName.Genre))

        assertTrue(left.isEmpty())
        assertEquals(listOf(TagName.Genre), emptied)
        assertEquals("", TagDiff.change(emptyMap(), emptied).genre)
    }

    @Test
    fun aMixedFieldTypedInIsWrittenWhetherOrNotItWasEmptied() {
        val shown = mapOf(TagName.Genre to "Jazz")

        val changed =
            TagDiff.changed(shown, emptyMap(), setOf(TagName.Genre), setOf(TagName.Genre))

        assertEquals(listOf(TagName.Genre), changed)
        assertEquals("Jazz", TagDiff.change(shown, changed).genre)
    }

    @Test
    fun onlyTheFieldsThatMovedAreSent() {
        val read = mapOf(TagName.Title to "Echoes", TagName.Album to "Meddle")
        val shown = read + (TagName.Album to "Obscured by Clouds")

        val changed = TagDiff.changed(shown, read, emptySet())
        val sent = TagDiff.change(shown, changed)

        assertEquals(listOf(TagName.Album), changed)
        assertEquals("Obscured by Clouds", sent.album)
        assertEquals(null, sent.title)
    }

    @Test
    fun songsShareAFieldTheyAgreeOnAndDifferOnTheRest() {
        val shared =
            TagDiff.common(
                listOf(
                    SongTags(title = "One", album = "Meddle", genre = "Rock"),
                    SongTags(title = "Two", album = "Meddle", genre = null),
                )
            )

        assertEquals("Meddle", shared.common[TagName.Album])
        assertEquals(setOf(TagName.Title, TagName.Genre), shared.differing)
        assertEquals("", shared.common[TagName.Composer])
    }

    @Test
    fun severalSongsHideWhatIsEachSongsOwn() {
        val own = TagName.entries.filter { it.eachSongsOwn }

        assertEquals(listOf(TagName.Title, TagName.TrackNumber), own)
        assertEquals(
            listOf(TagName.TrackNumber, TagName.TrackTotal, TagName.DiscNumber, TagName.DiscTotal),
            TagName.entries.filter { it.numbered },
        )
    }

    @Test
    fun theNoticeCountsWhatWasSavedAndWhatWasLeft() {
        assertEquals(
            TagNotice("Saved the tags of 1 song", false),
            TagSentences.told(1, 0, null, undoing = false),
        )
        assertEquals(
            TagNotice("Put back the tags of 3 songs", false),
            TagSentences.told(3, 0, null, undoing = true),
        )
        assertEquals(
            TagNotice("Nothing to change", false),
            TagSentences.told(0, 0, null, undoing = false),
        )
        assertEquals(
            TagNotice("The song's file could not be written (2 songs left as they were)", true),
            TagSentences.told(1, 2, "The song's file could not be written", undoing = false),
        )
        assertEquals(
            TagNotice("The song's file could not be written", true),
            TagSentences.told(0, 1, "The song's file could not be written", undoing = false),
        )
    }

    @Test
    fun theSheetCountsTheSongsItEdits() {
        assertEquals("Edit tags", TagSentences.editTitle(1))
        assertEquals("Edit 12 songs", TagSentences.editTitle(12))
        assertEquals(
            "1 song cannot be written and will be left alone.",
            TagSentences.cannotBeWritten(1),
        )
    }

    @Test
    fun anOpenedCopyIsWrittenAtItsPathAndAGrantedDocumentThroughTheBoundary() {
        val folder = File("/data/files/opened")

        val copy = TagFiles.songFile(OpenedNames.localUri("My Song.flac"), folder)
        val granted = TagFiles.songFile("content://tree/a/document/b.flac", folder)

        assertEquals(File(folder, "My Song.flac").absolutePath, copy.path)
        assertEquals("", granted.path)
        assertEquals("content://tree/a/document/b.flac", granted.uri)
    }

    @Test
    fun aMediaStoreFileIsWrittenThroughItsAudioItem() {
        assertEquals(
            "content://media/external_primary/audio/media/42",
            TagFiles.writeUri("content://media/external_primary/file/42"),
        )
        assertEquals(
            "content://com.android.externalstorage.documents/tree/a/document/b",
            TagFiles.writeUri("content://com.android.externalstorage.documents/tree/a/document/b"),
        )
    }

    @Test
    fun aKindIsTheLowercaseExtensionOfTheName() {
        assertEquals("flac", TagFiles.extensionOf("Echoes.FLAC"))
        assertEquals("", TagFiles.extensionOf("Echoes"))
    }
}
