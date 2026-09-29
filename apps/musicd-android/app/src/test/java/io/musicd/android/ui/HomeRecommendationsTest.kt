package io.musicd.android.ui

import io.musicd.android.data.AlbumMetadataDto
import io.musicd.android.data.AlbumRecommendationDto
import io.musicd.android.data.AlbumSummaryDto
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

class HomeRecommendationsTest {
    @Test
    fun replacementSkipsRecommendationsThatAreNoLongerSuggested() {
        val accepted = recommendation("accepted", title = "Accepted", status = "accepted")
        val suggested = recommendation("suggested", title = "Suggested")

        val replacement = chooseHomeRecommendationReplacement(
            candidates = listOf(accepted, suggested),
            currentRecommendations = emptyList(),
            libraryAlbums = emptyList(),
            extraExcludedKeys = emptySet(),
        )

        assertEquals("suggested", replacement?.recommendationKey)
    }

    @Test
    fun replacementSkipsAlbumsDismissedThisSessionEvenFromAnotherSeed() {
        val dismissed = recommendation("seed-a-eden", title = "Spirit of Eden", releaseGroupId = "eden")
        val sameAlbumOtherSeed = recommendation("seed-b-eden", title = "Spirit of Eden", releaseGroupId = "eden")
        val fresh = recommendation("fresh", title = "Laughing Stock")

        val replacement = chooseHomeRecommendationReplacement(
            candidates = listOf(sameAlbumOtherSeed, fresh),
            currentRecommendations = emptyList(),
            libraryAlbums = emptyList(),
            extraExcludedKeys = setOf(dismissed.recommendationKey),
            dismissedIdentities = setOf(homeRecommendationIdentity(dismissed)),
        )

        assertEquals("fresh", replacement?.recommendationKey)
    }

    @Test
    fun visibleRecommendationsHideAlbumsDismissedThisSession() {
        val dismissed = recommendation("seed-a-eden", title = "Spirit of Eden", releaseGroupId = "eden")
        val sameAlbumOtherSeed = recommendation("seed-b-eden", title = "Spirit of Eden", releaseGroupId = "eden")
        val other = recommendation("other", title = "Laughing Stock")
        val recommendations = listOf(dismissed, other, sameAlbumOtherSeed)

        val visible = visibleHomeRecommendations(recommendations, setOf(homeRecommendationIdentity(dismissed)))

        assertEquals(listOf("other"), visible.map { it.recommendationKey })
        assertSame(recommendations, visibleHomeRecommendations(recommendations, emptySet()))
    }

    @Test
    fun replacementSkipsTheRemovedRecommendation() {
        val removed = recommendation("removed", title = "Removed")
        val other = recommendation("other", title = "Other")

        val replacement = chooseHomeRecommendationReplacement(
            candidates = listOf(removed, other),
            currentRecommendations = emptyList(),
            libraryAlbums = emptyList(),
            extraExcludedKeys = setOf("removed"),
        )

        assertEquals("other", replacement?.recommendationKey)
    }

    @Test
    fun replacementSkipsAlbumsAlreadyOnScreenEvenFromAnotherSeed() {
        val onScreen = recommendation("seed-a-eden", title = "Spirit of Eden", releaseGroupId = "eden")
        val sameAlbumOtherSeed = recommendation("seed-b-eden", title = "Spirit of Eden", releaseGroupId = "eden")
        val sameArtistTitle = recommendation("seed-c-eden", title = "  spirit   of eden ")
        val fresh = recommendation("fresh", title = "Laughing Stock")

        val replacement = chooseHomeRecommendationReplacement(
            candidates = listOf(sameAlbumOtherSeed, sameArtistTitle, fresh),
            currentRecommendations = listOf(onScreen, recommendation("x", title = "Spirit of Eden")),
            libraryAlbums = emptyList(),
            extraExcludedKeys = emptySet(),
        )

        assertEquals("fresh", replacement?.recommendationKey)
    }

    @Test
    fun replacementSkipsAlbumsAlreadyInTheLibrary() {
        val byReleaseId = recommendation("by-release", title = "A", releaseId = "release-1")
        val byArtwork = recommendation("by-artwork", title = "B", artworkUrl = "/artwork/album/lib-2?size=250")
        val byArtistTitle = recommendation("by-name", title = "Colour Green", artist = "Sibylle Baier")
        val notOwned = recommendation("not-owned", title = "D")
        val library = listOf(
            album("lib-1", metadata = AlbumMetadataDto(musicbrainzReleaseId = "release-1")),
            album("lib-2"),
            album("lib-3", title = "colour green", artist = "sibylle baier"),
        )

        val replacement = chooseHomeRecommendationReplacement(
            candidates = listOf(byReleaseId, byArtwork, byArtistTitle, notOwned),
            currentRecommendations = emptyList(),
            libraryAlbums = library,
            extraExcludedKeys = emptySet(),
        )

        assertEquals("not-owned", replacement?.recommendationKey)
    }

    @Test
    fun replacementPrefersRecommendationsPlayableOnTidal() {
        val noTidal = recommendation("no-tidal", title = "A")
        val badTidalUrl = recommendation("bad-tidal", title = "B", tidalUrl = "https://tidal.com/browse/artist/1")
        val tidal = recommendation("tidal", title = "C", tidalUrl = "https://listen.tidal.com/album/12345")

        val replacement = chooseHomeRecommendationReplacement(
            candidates = listOf(noTidal, badTidalUrl, tidal),
            currentRecommendations = emptyList(),
            libraryAlbums = emptyList(),
            extraExcludedKeys = emptySet(),
        )

        assertEquals("tidal", replacement?.recommendationKey)
    }

    @Test
    fun replacementIsNullWhenNothingIsEligible() {
        val replacement = chooseHomeRecommendationReplacement(
            candidates = listOf(recommendation("gone", title = "Gone", status = "accepted")),
            currentRecommendations = emptyList(),
            libraryAlbums = emptyList(),
            extraExcludedKeys = emptySet(),
        )

        assertNull(replacement)
    }

    @Test
    fun replaceKeepsTheSlotOfTheRemovedRecommendation() {
        val first = recommendation("first", title = "First")
        val removed = recommendation("removed", title = "Removed")
        val last = recommendation("last", title = "Last")
        val replacement = recommendation("new", title = "New")

        val updated = replaceHomeRecommendation(
            recommendations = listOf(first, removed, last),
            removedRecommendationKey = "removed",
            replacement = replacement,
        )

        assertEquals(listOf("first", "new", "last"), updated.map { it.recommendationKey })
    }

    @Test
    fun replaceDropsTheSlotWhenThereIsNoReplacement() {
        val updated = replaceHomeRecommendation(
            recommendations = listOf(recommendation("a", title = "A"), recommendation("b", title = "B")),
            removedRecommendationKey = "a",
            replacement = null,
        )

        assertEquals(listOf("b"), updated.map { it.recommendationKey })
    }

    @Test
    fun replaceLeavesListUntouchedWhenKeyIsMissing() {
        val recommendations = listOf(recommendation("a", title = "A"))

        val updated = replaceHomeRecommendation(
            recommendations = recommendations,
            removedRecommendationKey = "missing",
            replacement = recommendation("new", title = "New"),
        )

        assertSame(recommendations, updated)
    }

    @Test
    fun prependCapsTheListAtTheHomeLimit() {
        val existing = (1..HOME_RECOMMENDATION_LIMIT).map { recommendation("r$it", title = "R$it") }

        val updated = prependHomeRecommendation(existing, recommendation("new", title = "New"))

        assertEquals(HOME_RECOMMENDATION_LIMIT, updated.size)
        assertEquals("new", updated.first().recommendationKey)
        assertEquals("r${HOME_RECOMMENDATION_LIMIT - 1}", updated.last().recommendationKey)
    }

    private fun recommendation(
        key: String,
        title: String,
        artist: String = "Talk Talk",
        status: String = "suggested",
        releaseId: String? = null,
        releaseGroupId: String? = null,
        tidalUrl: String? = null,
        artworkUrl: String? = null,
    ) = AlbumRecommendationDto(
        recommendationKey = key,
        source = "test",
        seedAlbumId = "seed",
        suggestedArtist = artist,
        suggestedTitle = title,
        suggestedMusicbrainzReleaseId = releaseId,
        suggestedMusicbrainzReleaseGroupId = releaseGroupId,
        tidalUrl = tidalUrl,
        artworkUrl = artworkUrl,
        status = status,
    )

    private fun album(
        id: String,
        title: String = "Library $id",
        artist: String = "Someone",
        metadata: AlbumMetadataDto? = null,
    ) = AlbumSummaryDto(
        id = id,
        title = title,
        artist = artist,
        trackCount = 10,
        firstTrackId = "$id-track-1",
        metadata = metadata,
    )
}
