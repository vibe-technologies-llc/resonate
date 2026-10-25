package org.resonate.android.ui.settings

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.resonate.android.nativecore.OrganisePlan

class OrganiseWordsTest {
    private val plan = OrganisePlan(emptyList(), 2u, 0u, 0u)

    @Test
    fun aPlanStandsOnlyForTheLayoutItWasMadeFor() {
        val planned = Planned("{title}.{ext}", plan)

        assertEquals(plan, OrganiseWords.current(planned, "{title}.{ext}"))
        assertNull(OrganiseWords.current(planned, "{track} {title}.{ext}"))
        assertNull(OrganiseWords.current(null, "{title}.{ext}"))
    }

    @Test
    fun theSpinnerWaitsOnlyForALayoutThatReadsAndHasNeitherAPlanNorAFailure() {
        assertTrue(OrganiseWords.waiting(null, null, null, "a"))
        assertFalse(OrganiseWords.waiting("unknown field", null, null, "a"))
        assertFalse(OrganiseWords.waiting(null, plan, null, "a"))
        assertFalse(OrganiseWords.waiting(null, null, "a", "a"))
        assertTrue(OrganiseWords.waiting(null, null, "b", "a"))
    }

    @Test
    fun theConfirmationNamesTheFolder() {
        assertTrue(OrganiseWords.explained("Music").startsWith("The files move inside Music,"))
    }
}
