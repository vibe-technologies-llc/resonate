package org.resonate.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
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
        assertEquals(1, ArtworkSizes.sampleFor(1200, 1200, 1024))
        assertEquals(2, ArtworkSizes.sampleFor(1200, 800, 384))
    }

    @Test
    fun theSampleIsTheLargestPowerOfTwoKeepingTheShorterSideAtLeastTheEdge() {
        for (width in listOf(250, 500, 999, 1000, 1200, 1600, 3000))
            for (height in listOf(250, 500, 800, 1200, 3000))
                for (edge in listOf(96, 192, 256, 384, 512, 768, 1024)) {
                    val sample = ArtworkSizes.sampleFor(width, height, edge)
                    val shorter = minOf(width, height)
                    assertEquals(0, sample and (sample - 1))
                    assertTrue(sample == 1 || shorter / sample >= edge)
                    assertTrue(shorter / (sample * 2) < edge)
                }
    }
}
