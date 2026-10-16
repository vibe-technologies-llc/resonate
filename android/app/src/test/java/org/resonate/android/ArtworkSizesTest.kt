package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Test

class ArtworkSizesTest {
    @Test
    fun aRequestIsServedAtTheSmallestBucketThatHoldsIt() {
        assertEquals(96, ArtworkSizes.edgeFor(50))
        assertEquals(96, ArtworkSizes.edgeFor(96))
        assertEquals(192, ArtworkSizes.edgeFor(97))
        assertEquals(384, ArtworkSizes.edgeFor(384))
        assertEquals(1024, ArtworkSizes.edgeFor(900))
        assertEquals(1024, ArtworkSizes.edgeFor(4000))
    }

    @Test
    fun aRequestWithoutASizeGetsTheDefaultBucket() {
        assertEquals(768, ArtworkSizes.edgeFor(null))
        assertEquals(768, ArtworkSizes.edgeFor(0))
    }

    @Test
    fun aPictureIsHalvedOnlyWhileItStaysAtLeastTheEdgeAsked() {
        assertEquals(1, ArtworkSizes.sampleFor(1024, 1024, 1024))
        assertEquals(4, ArtworkSizes.sampleFor(1024, 1024, 192))
        assertEquals(2, ArtworkSizes.sampleFor(1000, 600, 192))
        assertEquals(1, ArtworkSizes.sampleFor(100, 100, 192))
    }
}
