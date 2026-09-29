package io.musicd.android.ui

import io.musicd.android.data.AlbumRecommendationDto
import io.musicd.android.data.AlbumSummaryDto

internal const val HOME_RECOMMENDATION_LIMIT = 6

/**
 * Picks the next suggestion to show on the home screen from [candidates]: it must still be
 * `suggested` (not dismissed), not already on screen (by key or album identity), and not
 * already in the library. Candidates playable on TIDAL are preferred.
 */
internal fun chooseHomeRecommendationReplacement(
    candidates: List<AlbumRecommendationDto>,
    currentRecommendations: List<AlbumRecommendationDto>,
    libraryAlbums: List<AlbumSummaryDto>,
    extraExcludedKeys: Set<String>,
): AlbumRecommendationDto? {
    val excludedKeys = currentRecommendations
        .map { it.recommendationKey }
        .toSet() + extraExcludedKeys
    val excludedIdentities = currentRecommendations
        .map(::homeRecommendationIdentity)
        .toSet()
    val eligible = candidates
        .filter { it.status.equals("suggested", ignoreCase = true) }
        .filterNot { it.recommendationKey in excludedKeys }
        .filterNot { homeRecommendationIdentity(it) in excludedIdentities }
        .filter { findLibraryAlbumForHomeRecommendation(it, libraryAlbums) == null }
    return eligible.firstOrNull { tidalAlbumIdFromHomeRecommendation(it) != null }
        ?: eligible.firstOrNull()
}

internal fun prependHomeRecommendation(
    recommendations: List<AlbumRecommendationDto>,
    replacement: AlbumRecommendationDto,
): List<AlbumRecommendationDto> =
    (listOf(replacement) + recommendations.filterNot { it.recommendationKey == replacement.recommendationKey })
        .take(HOME_RECOMMENDATION_LIMIT)

internal fun replaceHomeRecommendation(
    recommendations: List<AlbumRecommendationDto>,
    removedRecommendationKey: String,
    replacement: AlbumRecommendationDto?,
): List<AlbumRecommendationDto> {
    val index = recommendations.indexOfFirst { it.recommendationKey == removedRecommendationKey }
    if (index < 0) {
        return recommendations
    }
    val updated = recommendations.toMutableList()
    if (replacement == null) {
        updated.removeAt(index)
    } else {
        updated[index] = replacement
    }
    return updated
        .distinctBy { it.recommendationKey }
        .take(HOME_RECOMMENDATION_LIMIT)
}

internal fun homeRecommendationIdentity(recommendation: AlbumRecommendationDto): String =
    recommendation.suggestedMusicbrainzReleaseGroupId
        ?.takeIf { it.isNotBlank() }
        ?.let { "release-group:$it" }
        ?: recommendation.suggestedMusicbrainzReleaseId
            ?.takeIf { it.isNotBlank() }
            ?.let { "release:$it" }
        ?: "artist-title:${normalizeRecommendationMatchText(recommendation.suggestedArtist)}:" +
            normalizeRecommendationMatchText(recommendation.suggestedTitle)

internal fun findLibraryAlbumForHomeRecommendation(
    recommendation: AlbumRecommendationDto,
    albums: List<AlbumSummaryDto>,
): AlbumSummaryDto? {
    recommendation.suggestedMusicbrainzReleaseId?.takeIf { it.isNotBlank() }?.let { releaseId ->
        albums.firstOrNull { it.metadata?.musicbrainzReleaseId == releaseId }?.let { return it }
    }
    recommendation.suggestedMusicbrainzReleaseGroupId?.takeIf { it.isNotBlank() }?.let { releaseGroupId ->
        albums.firstOrNull { it.metadata?.musicbrainzReleaseGroupId == releaseGroupId }?.let { return it }
    }
    recommendation.artworkUrl?.let(::albumIdFromRecommendationArtworkUrl)?.let { albumId ->
        albums.firstOrNull { it.id == albumId }?.let { return it }
    }

    val recommendedArtist = normalizeRecommendationMatchText(recommendation.suggestedArtist)
    val recommendedTitle = normalizeRecommendationMatchText(recommendation.suggestedTitle)
    return albums.firstOrNull {
        normalizeRecommendationMatchText(it.artist) == recommendedArtist &&
            normalizeRecommendationMatchText(it.title) == recommendedTitle
    }
}

private fun albumIdFromRecommendationArtworkUrl(url: String): String? =
    url
        .substringBefore('?')
        .trimEnd('/')
        .substringAfterLast('/')
        .takeIf { url.contains("/artwork/album/") && it.isNotBlank() }

private fun normalizeRecommendationMatchText(value: String): String =
    value.trim().lowercase().replace(Regex("""\s+"""), " ")

internal fun tidalAlbumIdFromHomeRecommendation(recommendation: AlbumRecommendationDto): String? {
    val url = recommendation.tidalUrl?.trim()?.takeIf { it.isNotBlank() } ?: return null
    val match = Regex("""^https?://(?:www\.)?(?:listen\.)?tidal\.com/(?:browse/)?album/([0-9]+)(?:[/?#].*)?$""")
        .matchEntire(url)
    return match?.groupValues?.getOrNull(1)
}
