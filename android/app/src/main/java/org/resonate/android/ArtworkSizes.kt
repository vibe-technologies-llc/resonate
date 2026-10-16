package org.resonate.android

object ArtworkSizes {
    const val MEDIA_EDGE = 512
    private const val DEFAULT_EDGE = 768
    private val edges = listOf(96, 192, 384, 512, 768, 1024)

    fun edgeFor(pixels: Int?): Int =
        if (pixels == null || pixels <= 0) DEFAULT_EDGE
        else edges.firstOrNull { it >= pixels } ?: edges.last()

    fun sampleFor(width: Int, height: Int, edge: Int): Int {
        var sample = 1
        while (width / (sample * 2) >= edge && height / (sample * 2) >= edge) sample *= 2
        return sample
    }
}
