package io.musicd.android.ui

import io.musicd.android.data.AlbumSummaryDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LibraryNewestTest {
    @Test
    fun newestAlbumsAreOrderedByAdditionNewestFirst() {
        val albums = listOf(
            album("old", addedUnixMillis = 1_000L),
            album("newest", addedUnixMillis = 3_000L),
            album("middle", addedUnixMillis = 2_000L),
        )

        assertEquals(listOf("newest", "middle", "old"), newestAlbums(albums).map { it.id })
    }

    @Test
    fun newestAlbumsAreLimitedToTen() {
        val albums = (1..15).map { album("album-$it", addedUnixMillis = it * 1_000L) }

        val newest = newestAlbums(albums)

        assertEquals(10, newest.size)
        assertEquals("album-15", newest.first().id)
        assertEquals("album-6", newest.last().id)
    }

    @Test
    fun albumsWithoutAnAdditionTimeAreLeftOut() {
        val albums = listOf(
            album("unknown", addedUnixMillis = null),
            album("zero", addedUnixMillis = 0L),
            album("known", addedUnixMillis = 1_000L),
        )

        assertEquals(listOf("known"), newestAlbums(albums).map { it.id })
        assertTrue(newestAlbums(albums.take(2)).isEmpty())
    }

    @Test
    fun albumsAddedTogetherAreOrderedByTitle() {
        val albums = listOf(
            album("b", addedUnixMillis = 1_000L, title = "beta"),
            album("a", addedUnixMillis = 1_000L, title = "Alpha"),
        )

        assertEquals(listOf("a", "b"), newestAlbums(albums).map { it.id })
    }

    @Test
    fun addedAgoLabelUsesTheLargestWholeUnit() {
        val now = 1_000L * DAY

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
