package org.resonate.android.ui.library

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.resonate.android.nativecore.SavedSearch
import org.resonate.android.nativecore.SearchOrder

class SavedSearchDraftTest {
    @Test
    fun aBlankCapAsksEveryRowAndADigitCapAsksThatMany() {
        assertEquals(Cap(null), capped(""))
        assertEquals(Cap(null), capped("   "))
        assertEquals(Cap(25u), capped(" 25 "))
    }

    @Test
    fun aCapThatIsNotAWholeNumberAboveZeroIsRefused() {
        assertNull(capped("0"))
        assertNull(capped("-3"))
        assertNull(capped("2.5"))
        assertNull(capped("many"))
        assertNull(capped("99999999999"))
    }

    @Test
    fun aDraftReadsBackAsTheSearchItWasMadeFrom() {
        val search = SavedSearch(text = "genre:jazz", order = SearchOrder.PLAYS, descending = true, rows = 40u)

        val draft = SavedSearchDraft.of("Jazz", search)

        assertEquals("Jazz", draft.name)
        assertEquals("40", draft.cap)
        assertEquals(search, draft.search())
    }

    @Test
    fun aDraftTrimsItsTextAndRefusesACapThatIsNotOne() {
        val draft =
            SavedSearchDraft(
                name = "Jazz",
                text = "  genre:jazz ",
                order = SearchOrder.TITLE,
                descending = false,
                cap = "",
            )

        assertEquals(SavedSearch("genre:jazz", SearchOrder.TITLE, false, null), draft.search())
        assertNull(draft.copy(cap = "0").search())
    }
}
