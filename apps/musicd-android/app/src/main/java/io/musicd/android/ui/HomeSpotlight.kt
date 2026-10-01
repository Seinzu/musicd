package io.musicd.android.ui

import io.musicd.android.data.AlbumSummaryDto
import java.time.LocalDate
import kotlin.random.Random

/**
 * Picks the albums for the home screen's library spotlight. The choice is stable for a given
 * [day] and library: 3 to 5 albums with more than three tracks (or any albums if none qualify).
 * Albums in [suppressedAlbumIds] (played or dismissed this session) are swapped for the next
 * album in the day's order, keeping their slot.
 */
internal fun homeSpotlightAlbums(
    albums: List<AlbumSummaryDto>,
    suppressedAlbumIds: Set<String>,
    day: LocalDate,
): List<AlbumSummaryDto> {
    val eligibleAlbums = albums.filter { it.trackCount > 3 }
    val dailySeed = eligibleAlbums
        .map { it.id }
        .sorted()
        .joinToString("|")
        .plus("|")
        .plus(day.toString())
        .hashCode()
    val orderedAlbums = if (eligibleAlbums.isEmpty()) {
        albums
    } else {
        eligibleAlbums.shuffled(Random(dailySeed))
    }
    val maxCount = minOf(5, orderedAlbums.size)
    val minCount = minOf(3, maxCount)
    val targetCount = if (eligibleAlbums.isEmpty() || minCount == maxCount) {
        maxCount
    } else {
        Random(dailySeed).nextInt(minCount, maxCount + 1)
    }
    val initialAlbums = orderedAlbums.take(targetCount)
    val replacementAlbums = orderedAlbums
        .drop(targetCount)
        .filterNot { it.id in suppressedAlbumIds }
        .iterator()
    return initialAlbums.mapNotNull { album ->
        if (album.id in suppressedAlbumIds) {
            if (replacementAlbums.hasNext()) replacementAlbums.next() else null
        } else {
            album
        }
    }
}
