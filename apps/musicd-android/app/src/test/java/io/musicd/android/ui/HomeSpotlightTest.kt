package io.musicd.android.ui

import io.musicd.android.data.AlbumSummaryDto
import java.time.LocalDate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class HomeSpotlightTest {
    private val day = LocalDate.of(2026, 10, 1)
    private val library = (1..12).map { album("album-$it", trackCount = 10) }

    @Test
    fun spotlightIsStableForTheDayAndShowsThreeToFiveAlbums() {
        val first = homeSpotlightAlbums(library, emptySet(), day)
        val second = homeSpotlightAlbums(library.toList(), emptySet(), day)

        assertEquals(first, second)
        assertTrue(first.size in 3..5)
    }

    @Test
    fun spotlightSkipsShortAlbumsWhenLongerOnesExist() {
        val short = (1..5).map { album("short-$it", trackCount = 2) }

        val spotlight = homeSpotlightAlbums(library + short, emptySet(), day)

        assertTrue(spotlight.none { it.trackCount <= 3 })
    }

    @Test
    fun dismissedAlbumIsReplacedInTheSameSlot() {
        val initial = homeSpotlightAlbums(library, emptySet(), day)
        val dismissed = initial[1]

        val updated = homeSpotlightAlbums(library, setOf(dismissed.id), day)

        assertEquals(initial.size, updated.size)
        assertFalse(updated.any { it.id == dismissed.id })
        assertEquals(initial[0], updated[0])
        assertEquals(initial.drop(2), updated.drop(2))
    }

    @Test
    fun dismissedAlbumIsDroppedWhenThereIsNoReplacement() {
        val smallLibrary = library.take(3)
        val initial = homeSpotlightAlbums(smallLibrary, emptySet(), day)

        val updated = homeSpotlightAlbums(smallLibrary, setOf(initial.first().id), day)

        assertEquals(initial.drop(1), updated)
    }

    private fun album(id: String, trackCount: Int) = AlbumSummaryDto(
        id = id,
        title = "Title $id",
        artist = "Artist",
        trackCount = trackCount,
        firstTrackId = "$id-track-1",
    )
}
