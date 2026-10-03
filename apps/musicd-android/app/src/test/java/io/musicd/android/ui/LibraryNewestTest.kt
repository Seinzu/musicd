package io.musicd.android.ui

import io.musicd.android.data.AlbumSummaryDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LibraryNewestTest {
    private val now = 1_000L * DAY

    @Test
    fun recentAlbumsAreOrderedByAdditionNewestFirst() {
        val albums = listOf(
            album("old", addedUnixMillis = now - 30 * DAY),
            album("newest", addedUnixMillis = now - DAY),
            album("middle", addedUnixMillis = now - 10 * DAY),
        )

        assertEquals(listOf("newest", "middle", "old"), recentlyAddedAlbums(albums, now).map { it.id })
    }

    @Test
    fun everyAlbumFromTheLastThreeMonthsIsIncluded() {
        val recent = (1..40).map { album("recent-$it", addedUnixMillis = now - it * DAY) }
        val older = listOf(
            album("just-outside", addedUnixMillis = now - 91 * DAY),
            album("last-year", addedUnixMillis = now - 400 * DAY),
        )
        val edge = album("edge", addedUnixMillis = now - 90 * DAY)

        val result = recentlyAddedAlbums(recent + older + edge, now)

        assertEquals(41, result.size)
        assertEquals("recent-1", result.first().id)
        assertEquals("edge", result.last().id)
    }

    @Test
    fun albumsWithoutAnAdditionTimeAreLeftOut() {
        val albums = listOf(
            album("unknown", addedUnixMillis = null),
            album("zero", addedUnixMillis = 0L),
            album("known", addedUnixMillis = now - DAY),
        )

        assertEquals(listOf("known"), recentlyAddedAlbums(albums, now).map { it.id })
        assertTrue(recentlyAddedAlbums(albums.take(2), now).isEmpty())
    }

    @Test
    fun albumsAddedTogetherAreOrderedByTitle() {
        val albums = listOf(
            album("b", addedUnixMillis = now - DAY, title = "beta"),
            album("a", addedUnixMillis = now - DAY, title = "Alpha"),
        )

        assertEquals(listOf("a", "b"), recentlyAddedAlbums(albums, now).map { it.id })
    }

    @Test
    fun addedAgoLabelUsesTheLargestWholeUnit() {
        assertEquals("Added today", addedAgoLabel(now - DAY / 2, now))
        assertEquals("Added yesterday", addedAgoLabel(now - DAY, now))
        assertEquals("Added 6 days ago", addedAgoLabel(now - 6 * DAY, now))
        assertEquals("Added 1 week ago", addedAgoLabel(now - 7 * DAY, now))
        assertEquals("Added 4 weeks ago", addedAgoLabel(now - 29 * DAY, now))
        assertEquals("Added 2 months ago", addedAgoLabel(now - 60 * DAY, now))
        assertEquals("Added 1 year ago", addedAgoLabel(now - 400 * DAY, now))
        assertEquals("Added today", addedAgoLabel(now + DAY, now))
    }

    private fun album(id: String, addedUnixMillis: Long?, title: String = "Title $id") = AlbumSummaryDto(
        id = id,
        title = title,
        artist = "Artist",
        trackCount = 10,
        firstTrackId = "$id-track-1",
        addedUnixMillis = addedUnixMillis,
    )

    private companion object {
        const val DAY = 24L * 60 * 60 * 1000
    }
}
