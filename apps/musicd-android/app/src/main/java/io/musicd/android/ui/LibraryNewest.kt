package io.musicd.android.ui

import io.musicd.android.data.AlbumSummaryDto

private const val MILLIS_PER_DAY = 24L * 60 * 60 * 1000

/** How far back the Library "New" view looks: roughly three months. */
internal const val NEW_ALBUM_WINDOW_DAYS = 90L

/**
 * Albums added to the library in the [NEW_ALBUM_WINDOW_DAYS] days before [nowUnixMillis], newest
 * first. Albums the server doesn't have an addition time for (older servers, the local companion)
 * are left out.
 */
internal fun recentlyAddedAlbums(
    albums: List<AlbumSummaryDto>,
    nowUnixMillis: Long,
): List<AlbumSummaryDto> {
    val cutoff = nowUnixMillis - NEW_ALBUM_WINDOW_DAYS * MILLIS_PER_DAY
    return albums
        .filter { album -> album.addedUnixMillis.let { it != null && it > 0L && it >= cutoff } }
        .sortedWith(
            compareByDescending<AlbumSummaryDto> { it.addedUnixMillis }
                .thenBy { it.title.lowercase() }
                .thenBy { it.id },
        )
}

/** A short "Added 3 days ago" label for an album added at [addedUnixMillis]. */
internal fun addedAgoLabel(addedUnixMillis: Long, nowUnixMillis: Long): String {
    val days = ((nowUnixMillis - addedUnixMillis) / MILLIS_PER_DAY).coerceAtLeast(0L)
    return when {
        days == 0L -> "Added today"
        days == 1L -> "Added yesterday"
        days < 7L -> "Added $days days ago"
        days < 30L -> plural(days / 7L, "week")
        days < 365L -> plural(days / 30L, "month")
        else -> plural(days / 365L, "year")
    }
}

private fun plural(count: Long, unit: String): String =
    if (count == 1L) "Added 1 $unit ago" else "Added $count ${unit}s ago"
