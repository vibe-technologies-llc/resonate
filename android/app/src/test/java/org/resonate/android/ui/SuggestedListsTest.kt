package org.resonate.android.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.MixAccent
import org.resonate.android.nativecore.MixAccents
import org.resonate.android.nativecore.SavedSearch
import org.resonate.android.nativecore.SearchOrder
import org.resonate.android.nativecore.SuggestedList
import org.resonate.android.nativecore.SuggestionReason
import org.resonate.android.nativecore.SuggestionShelf

class SuggestedListsTest {
    private fun list(name: String, shelf: SuggestionShelf) =
        SuggestedList(
            id = name,
            name = name,
            detail = "",
            reason = SuggestionReason.ON_REPEAT,
            shelf = shelf,
            shelfName = "",
            search = SavedSearch("", SearchOrder.RELEVANCE, false, null),
            songs = 10u,
            durationMs = 0uL,
            covers = emptyList(),
            accents = MixAccents(MixAccent.TEAL, MixAccent.BLUE),
        )

    @Test
    fun aSearchIsKeptAsPlainValuesWithItsOrderDirectionAndCap() {
        val search = SavedSearch("plays:>=3@1mo", SearchOrder.PLAYS_THIS_MONTH, true, 40u)

        val kept = SuggestedSearch.of(search)

        assertEquals(SuggestedSearch("plays:>=3@1mo", "PLAYS_THIS_MONTH", true, 40), kept)
        assertEquals(search, kept.native())
    }

    @Test
    fun aSearchWithNoCapComesBackWithNone() {
        val search = SavedSearch("artist:=\"Air\"", SearchOrder.ALBUM, false, null)

        assertNull(SuggestedSearch.of(search).native().rows)
        assertEquals(search, SuggestedSearch.of(search).native())
    }

    @Test
    fun homeShowsTheListsFromListeningAndLeavesTheCatalogsShelvesToThePage() {
        val lists =
            listOf(
                list("On repeat", SuggestionShelf.FOR_YOU),
                list("The eighties", SuggestionShelf.ERAS),
                list("Old flames", SuggestionShelf.REDISCOVER),
                list("Rock", SuggestionShelf.GENRES),
                list("Never heard", SuggestionShelf.LISTENING),
            )

        assertEquals(listOf("On repeat", "Old flames", "Never heard"), lists.onHome().map { it.name })
    }

    @Test
    fun homeShowsAtMostTwelveLists() {
        val lists = (0 until 30).map { list("List $it", SuggestionShelf.FOR_YOU) }

        assertEquals(12, lists.onHome().size)
        assertEquals("List 0", lists.onHome().first().name)
    }
}
