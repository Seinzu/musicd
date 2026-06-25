package io.musicd.android.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import okhttp3.FormBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import java.io.IOException
import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.UnknownHostException
import java.net.UnknownServiceException

@Serializable
data class RendererDto(
    val location: String,
    val name: String,
    val manufacturer: String? = null,
    @SerialName("model_name") val modelName: String? = null,
    @SerialName("av_transport_control_url") val avTransportControlUrl: String? = null,
    @SerialName("last_seen_unix") val lastSeenUnix: Long = 0L,
    val selected: Boolean = false,
    val kind: String = "upnp",
    @SerialName("direct_access") val directAccess: Boolean = true,
    val visibility: String = "public",
    val health: RendererHealthDto? = null,
    val error: String? = null,
    val group: RendererGroupDto? = null,
)

@Serializable
data class RendererHealthDto(
    @SerialName("last_checked_unix") val lastCheckedUnix: Long = 0L,
    @SerialName("last_reachable_unix") val lastReachableUnix: Long? = null,
    @SerialName("last_error") val lastError: String? = null,
    val reachable: Boolean = false,
)

@Serializable
data class RendererGroupDto(
    val id: String,
    val location: String,
    val name: String,
    @SerialName("member_count") val memberCount: Int = 0,
    val members: List<RendererGroupMemberDto> = emptyList(),
)

@Serializable
data class RendererGroupMemberDto(
    @SerialName("renderer_location") val rendererLocation: String,
    val position: Long = 0L,
    @SerialName("joined_unix") val joinedUnix: Long = 0L,
)

@Serializable
data class RendererVolumeDto(
    val ok: Boolean = true,
    @SerialName("renderer_location") val rendererLocation: String,
    val volume: Int,
)

@Serializable
data class ServerInfoDto(
    val name: String,
    @SerialName("base_url") val baseUrl: String,
    @SerialName("bind_address") val bindAddress: String,
)

@Serializable
data class TrackSummaryDto(
    val id: String,
    @SerialName("album_id") val albumId: String,
    val title: String,
    val artist: String,
    val album: String,
    @SerialName("disc_number") val discNumber: Int? = null,
    @SerialName("track_number") val trackNumber: Int? = null,
    @SerialName("duration_seconds") val durationSeconds: Long? = null,
    @SerialName("artwork_url") val artworkUrl: String? = null,
    @SerialName("mime_type") val mimeType: String? = null,
    @SerialName("like_count") val likeCount: Long = 0L,
    @SerialName("liked_by_client") val likedByClient: Boolean = false,
)

@Serializable
data class AlbumMetadataDto(
    @SerialName("release_date") val releaseDate: String? = null,
    @SerialName("musicbrainz_release_id") val musicbrainzReleaseId: String? = null,
    @SerialName("musicbrainz_release_group_id") val musicbrainzReleaseGroupId: String? = null,
    @SerialName("original_release_date") val originalReleaseDate: String? = null,
    @SerialName("release_country") val releaseCountry: String? = null,
    @SerialName("release_type") val releaseType: String? = null,
    val genres: List<String> = emptyList(),
    @SerialName("source_track_id") val sourceTrackId: String? = null,
)

@Serializable
data class AlbumSummaryDto(
    val id: String,
    val title: String,
    val artist: String,
    @SerialName("track_count") val trackCount: Int,
    @SerialName("first_track_id") val firstTrackId: String,
    @SerialName("artwork_url") val artworkUrl: String = "",
    val metadata: AlbumMetadataDto? = null,
    @SerialName("like_count") val likeCount: Long = 0L,
    @SerialName("liked_by_client") val likedByClient: Boolean = false,
)



@Serializable
data class ArtistSummaryDto(
    val id: String,
    val name: String,
    @SerialName("album_count") val albumCount: Int,
    @SerialName("track_count") val trackCount: Int,
    @SerialName("artwork_url") val artworkUrl: String? = null,
    @SerialName("first_album_id") val firstAlbumId: String,
)

@Serializable
data class AlbumDetailDto(
    val id: String,
    val title: String,
    val artist: String,
    val metadata: AlbumMetadataDto,
    @SerialName("track_count") val trackCount: Int,
    @SerialName("first_track_id") val firstTrackId: String,
    @SerialName("artwork_url") val artworkUrl: String = "",
    @SerialName("like_count") val likeCount: Long = 0L,
    @SerialName("liked_by_client") val likedByClient: Boolean = false,
    val tracks: List<TrackSummaryDto> = emptyList(),
)

@Serializable
data class AlbumArtworkCandidateDto(
    @SerialName("release_id") val releaseId: String,
    @SerialName("release_group_id") val releaseGroupId: String? = null,
    val title: String,
    val artist: String,
    val date: String? = null,
    val country: String? = null,
    val score: Int = 0,
    @SerialName("thumbnail_url") val thumbnailUrl: String,
    @SerialName("image_url") val imageUrl: String,
    val source: String,
)

@Serializable
data class AlbumArtworkCandidatesResponseDto(
    val album: AlbumSummaryDto? = null,
    val candidates: List<AlbumArtworkCandidateDto> = emptyList(),
    val error: String? = null,
)

@Serializable
data class AlbumRecommendationDto(
    @SerialName("recommendation_key") val recommendationKey: String,
    val source: String,
    @SerialName("batch_id") val batchId: String? = null,
    @SerialName("seed_album_id") val seedAlbumId: String,
    @SerialName("seed_musicbrainz_release_id") val seedMusicbrainzReleaseId: String? = null,
    @SerialName("suggested_artist") val suggestedArtist: String,
    @SerialName("suggested_title") val suggestedTitle: String,
    @SerialName("suggested_musicbrainz_release_id") val suggestedMusicbrainzReleaseId: String? = null,
    @SerialName("suggested_musicbrainz_release_group_id") val suggestedMusicbrainzReleaseGroupId: String? = null,
    val confidence: Double? = null,
    val rationale: String? = null,
    @SerialName("external_url") val externalUrl: String? = null,
    @SerialName("tidal_url") val tidalUrl: String? = null,
    @SerialName("artwork_url") val artworkUrl: String? = null,
    val status: String = "suggested",
    @SerialName("created_unix") val createdUnix: Long = 0L,
    @SerialName("updated_unix") val updatedUnix: Long = 0L,
)

@Serializable
data class AlbumRecommendationsResponseDto(
    val recommendations: List<AlbumRecommendationDto> = emptyList(),
)

@Serializable
data class RadioStationDto(
    val id: String,
    val name: String,
    @SerialName("stream_url") val streamUrl: String,
    @SerialName("homepage_url") val homepageUrl: String? = null,
    @SerialName("artwork_url") val artworkUrl: String? = null,
    val tags: List<String> = emptyList(),
    @SerialName("country_code") val countryCode: String? = null,
    val language: String? = null,
    val codec: String? = null,
    val bitrate: Int? = null,
    val votes: Int? = null,
    @SerialName("click_count") val clickCount: Int? = null,
)

@Serializable
data class TidalTrackDto(
    @SerialName("track_id") val trackId: String,
    val title: String,
    val artist: String? = null,
    val album: String? = null,
    @SerialName("duration_seconds") val durationSeconds: Long? = null,
    @SerialName("artwork_url") val artworkUrl: String? = null,
)

@Serializable
data class TidalAlbumDto(
    @SerialName("album_id") val albumId: String,
    val title: String,
    val artist: String? = null,
    @SerialName("track_count") val trackCount: Long? = null,
    @SerialName("duration_seconds") val durationSeconds: Long? = null,
    @SerialName("artwork_url") val artworkUrl: String? = null,
    @SerialName("release_date") val releaseDate: String? = null,
)

@Serializable
data class ArtistDetailDto(
    val id: String,
    val name: String,
    @SerialName("album_count") val albumCount: Int,
    @SerialName("track_count") val trackCount: Int,
    @SerialName("artwork_url") val artworkUrl: String? = null,
    @SerialName("first_album_id") val firstAlbumId: String,
    val albums: List<AlbumSummaryDto> = emptyList(),
)

@Serializable
data class SessionDto(
    @SerialName("transport_state") val transportState: String,
    @SerialName("queue_entry_id") val queueEntryId: Long? = null,
    @SerialName("next_queue_entry_id") val nextQueueEntryId: Long? = null,
    @SerialName("current_track_uri") val currentTrackUri: String? = null,
    @SerialName("position_seconds") val positionSeconds: Long? = null,
    @SerialName("duration_seconds") val durationSeconds: Long? = null,
    @SerialName("last_observed_unix") val lastObservedUnix: Long = 0L,
    @SerialName("server_unix") val serverUnix: Long = 0L,
    @SerialName("last_error") val lastError: String? = null,
    val title: String? = null,
    val artist: String? = null,
    val album: String? = null,
)

@Serializable
data class QueueEntryDto(
    val id: Long,
    val position: Long,
    @SerialName("track_id") val trackId: String,
    val title: String? = null,
    val artist: String? = null,
    val album: String? = null,
    @SerialName("entry_status") val entryStatus: String,
    @SerialName("duration_seconds") val durationSeconds: Long? = null,
)

@Serializable
data class QueueDto(
    @SerialName("renderer_location") val rendererLocation: String,
    val name: String = "",
    val status: String,
    val version: Long = 0L,
    @SerialName("updated_unix") val updatedUnix: Long = 0L,
    @SerialName("current_entry_id") val currentEntryId: Long? = null,
    val entries: List<QueueEntryDto> = emptyList(),
    val session: SessionDto? = null,
)

@Serializable
data class QueueSummaryDto(
    val status: String,
    val name: String,
    @SerialName("entry_count") val entryCount: Int,
    @SerialName("current_entry_id") val currentEntryId: Long? = null,
    @SerialName("updated_unix") val updatedUnix: Long = 0L,
    val version: Long = 0L,
)

@Serializable
data class NowPlayingDto(
    @SerialName("renderer_location") val rendererLocation: String,
    val renderer: RendererDto? = null,
    @SerialName("current_track") val currentTrack: TrackSummaryDto? = null,
    val session: SessionDto? = null,
    @SerialName("queue_summary") val queueSummary: QueueSummaryDto,
)

@Serializable
data class PlaybackEventDto(
    @SerialName("renderer_location") val rendererLocation: String,
    @SerialName("now_playing") val nowPlaying: NowPlayingDto,
    val queue: QueueDto,
)

@Serializable
data class MutationResponseDto(
    val ok: Boolean,
    val message: String? = null,
    val error: String? = null,
    @SerialName("renderer_location") val rendererLocation: String? = null,
    val queue: QueueDto? = null,
    val session: SessionDto? = null,
)

@Serializable
data class LikeResponseDto(
    val ok: Boolean,
    @SerialName("item_kind") val itemKind: String,
    @SerialName("item_id") val itemId: String,
    @SerialName("like_count") val likeCount: Long = 0L,
    @SerialName("liked_by_client") val likedByClient: Boolean = false,
    val created: Boolean = false,
    val error: String? = null,
)

@Serializable
private data class ErrorEnvelopeDto(
    val error: String? = null,
    val message: String? = null,
)

sealed class MusicdApiException(
    open val userMessage: String,
    cause: Throwable? = null,
) : IOException(userMessage, cause) {
    class Network(
        override val userMessage: String,
        cause: Throwable? = null,
    ) : MusicdApiException(userMessage, cause)

    class Http(
        val statusCode: Int,
        val serverMessage: String?,
        override val userMessage: String,
    ) : MusicdApiException(userMessage)

    class InvalidResponse(
        override val userMessage: String,
        cause: Throwable? = null,
    ) : MusicdApiException(userMessage, cause)
}

class MusicdApi(
    private val client: OkHttpClient = OkHttpClient(),
    private val json: Json = Json {
        ignoreUnknownKeys = true
        coerceInputValues = true
                                  },
) {
    suspend fun getServerInfo(baseUrl: String): ServerInfoDto =
        get("$baseUrl/api/server")

    suspend fun getAlbums(baseUrl: String, clientId: String): List<AlbumSummaryDto> =
        get("$baseUrl/api/albums?client_id=${clientId.encodeForUrl()}")

    suspend fun getArtists(baseUrl: String): List<ArtistSummaryDto> =
        get("$baseUrl/api/artists")

    suspend fun getAlbumDetail(baseUrl: String, albumId: String, clientId: String): AlbumDetailDto =
        get("$baseUrl/api/albums/${albumId.encodeForUrl()}?client_id=${clientId.encodeForUrl()}")

    suspend fun getAlbumArtworkCandidates(
        baseUrl: String,
        albumId: String,
    ): AlbumArtworkCandidatesResponseDto =
        get("$baseUrl/api/albums/${albumId.encodeForUrl()}/artwork/candidates")

    suspend fun getAlbumRecommendations(
        baseUrl: String,
        albumId: String,
    ): AlbumRecommendationsResponseDto =
        get("$baseUrl/api/recommendations?album_id=${albumId.encodeForUrl()}")

    suspend fun getCollectionRecommendations(
        baseUrl: String,
        limit: Int,
    ): AlbumRecommendationsResponseDto =
        get("$baseUrl/api/recommendations?exclude_library=true&status=suggested&random=true&limit=$limit")

    suspend fun searchRadioStations(
        baseUrl: String,
        query: String,
        countryCode: String,
        limit: Int,
    ): List<RadioStationDto> =
        get(
            buildString {
                append("$baseUrl/api/radio/stations?limit=$limit")
                query.takeIf { it.isNotBlank() }?.let {
                    append("&query=${it.encodeForUrl()}")
                }
                countryCode.takeIf { it.isNotBlank() }?.let {
                    append("&countrycode=${it.encodeForUrl()}")
                }
            },
        )

    suspend fun searchTidalTracks(
        baseUrl: String,
        query: String,
        limit: Int,
    ): List<TidalTrackDto> =
        get("$baseUrl/api/tidal/search-tracks?query=${query.encodeForUrl()}&limit=$limit")

    suspend fun searchTidalAlbums(
        baseUrl: String,
        query: String,
        limit: Int,
    ): List<TidalAlbumDto> =
        get("$baseUrl/api/tidal/search-albums?query=${query.encodeForUrl()}&limit=$limit")

    suspend fun getArtistDetail(baseUrl: String, artistId: String, clientId: String): ArtistDetailDto =
        get("$baseUrl/api/artists/${artistId.encodeForUrl()}?client_id=${clientId.encodeForUrl()}")

    suspend fun getTracks(baseUrl: String, clientId: String): List<TrackSummaryDto> =
        get("$baseUrl/api/tracks?client_id=${clientId.encodeForUrl()}")

    suspend fun getRenderers(baseUrl: String, clientId: String): List<RendererDto> =
        get("$baseUrl/api/renderers?client_id=${clientId.encodeForUrl()}")

    suspend fun discoverRenderers(baseUrl: String): List<RendererDto> =
        get("$baseUrl/api/renderers/discover")

    suspend fun getRendererVolume(
        baseUrl: String,
        rendererLocation: String,
        clientId: String,
    ): RendererVolumeDto =
        get("$baseUrl/api/renderers/volume?renderer_location=${rendererLocation.encodeForUrl()}&client_id=${clientId.encodeForUrl()}")

    suspend fun setRendererVolume(
        baseUrl: String,
        rendererLocation: String,
        volume: Int,
        clientId: String,
    ): RendererVolumeDto = post(
        "$baseUrl/api/renderers/volume",
        mapOf(
            "renderer_location" to rendererLocation,
            "volume" to volume.coerceIn(0, 100).toString(),
            "client_id" to clientId,
        ),
    )

    suspend fun createRendererGroup(
        baseUrl: String,
        name: String,
        memberLocations: List<String>,
        sourceRendererLocation: String?,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/renderer-groups",
        buildMap {
            put("client_id", clientId)
            put("name", name)
            put("members", memberLocations.joinToString(","))
            sourceRendererLocation
                ?.takeIf { it.isNotBlank() }
                ?.let { put("source_renderer_location", it) }
        },
    )

    suspend fun deleteRendererGroup(
        baseUrl: String,
        rendererLocation: String,
        clientId: String,
        inheritRendererLocation: String? = null,
    ): MutationResponseDto = post(
        "$baseUrl/api/renderer-groups/delete",
        buildMap {
            put("renderer_location", rendererLocation)
            put("client_id", clientId)
            inheritRendererLocation
                ?.takeIf { it.isNotBlank() }
                ?.let { put("inherit_renderer_location", it) }
        },
    )

    suspend fun updateRendererGroup(
        baseUrl: String,
        rendererLocation: String,
        name: String,
        memberLocations: List<String>,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/renderer-groups/update",
        mapOf(
            "renderer_location" to rendererLocation,
            "client_id" to clientId,
            "name" to name,
            "members" to memberLocations.joinToString(","),
        ),
    )

    suspend fun registerAndroidLocalRenderer(
        baseUrl: String,
        rendererLocation: String,
        name: String,
        manufacturer: String?,
        modelName: String?,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/renderers/register-android-local",
        buildMap {
            put("client_id", clientId)
            put("renderer_location", rendererLocation)
            put("name", name)
            put("visibility", "private")
            manufacturer?.takeIf { it.isNotBlank() }?.let { put("manufacturer", it) }
            modelName?.takeIf { it.isNotBlank() }?.let { put("model_name", it) }
        },
    )

    suspend fun reportAndroidLocalSession(
        baseUrl: String,
        rendererLocation: String,
        transportState: String,
        currentTrackUri: String?,
        positionSeconds: Long?,
        durationSeconds: Long?,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/renderers/android-local/session",
        buildMap {
            put("client_id", clientId)
            put("renderer_location", rendererLocation)
            put("transport_state", transportState)
            currentTrackUri?.takeIf { it.isNotBlank() }?.let { put("current_track_uri", it) }
            positionSeconds?.let { put("position_seconds", it.toString()) }
            durationSeconds?.let { put("duration_seconds", it.toString()) }
        },
    )

    suspend fun reportAndroidLocalCompleted(
        baseUrl: String,
        rendererLocation: String,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/renderers/android-local/completed",
        mapOf("renderer_location" to rendererLocation, "client_id" to clientId),
    )

    suspend fun getQueue(baseUrl: String, rendererLocation: String, clientId: String): QueueDto =
        get("$baseUrl/api/queue?renderer_location=${rendererLocation.encodeForUrl()}&client_id=${clientId.encodeForUrl()}")

    suspend fun getNowPlaying(baseUrl: String, rendererLocation: String, clientId: String): NowPlayingDto =
        get("$baseUrl/api/now-playing?renderer_location=${rendererLocation.encodeForUrl()}&client_id=${clientId.encodeForUrl()}")

    suspend fun transport(baseUrl: String, path: String, rendererLocation: String, clientId: String): MutationResponseDto =
        post(
            "$baseUrl$path",
            mapOf("renderer_location" to rendererLocation, "client_id" to clientId),
        )

    suspend fun playTrack(
        baseUrl: String,
        rendererLocation: String,
        trackId: String,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/play",
        mapOf(
            "renderer_location" to rendererLocation,
            "client_id" to clientId,
            "track_id" to trackId,
        ),
    )

    suspend fun playAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/play-album",
        mapOf(
            "renderer_location" to rendererLocation,
            "client_id" to clientId,
            "album_id" to albumId,
        ),
    )

    suspend fun playRadioStation(
        baseUrl: String,
        rendererLocation: String,
        station: RadioStationDto,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/radio/play",
        buildMap {
            put("renderer_location", rendererLocation)
            put("client_id", clientId)
            put("stream_url", station.streamUrl)
            put("station_name", station.name)
            station.id.takeIf { it.isNotBlank() }?.let { put("station_id", it) }
            station.codec?.takeIf { it.isNotBlank() }?.let { put("codec", it) }
            station.artworkUrl?.takeIf { it.isNotBlank() }?.let { put("artwork_url", it) }
        },
    )

    suspend fun playTidalTrack(
        baseUrl: String,
        rendererLocation: String,
        track: TidalTrackDto,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/tidal/play-track",
        tidalTrackFormFields(rendererLocation, track, clientId),
    )

    suspend fun playTidalAlbum(
        baseUrl: String,
        rendererLocation: String,
        album: TidalAlbumDto,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/tidal/play-album",
        tidalAlbumFormFields(rendererLocation, album, clientId),
    )

    suspend fun selectAlbumArtwork(
        baseUrl: String,
        albumId: String,
        releaseId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/albums/artwork/select",
        mapOf(
            "album_id" to albumId,
            "release_id" to releaseId,
        ),
    )

    suspend fun appendTrack(
        baseUrl: String,
        rendererLocation: String,
        trackId: String,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/append-track",
        mapOf(
            "renderer_location" to rendererLocation,
            "client_id" to clientId,
            "track_id" to trackId,
        ),
    )

    suspend fun playNextTrack(
        baseUrl: String,
        rendererLocation: String,
        trackId: String,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/play-next-track",
        mapOf(
            "renderer_location" to rendererLocation,
            "client_id" to clientId,
            "track_id" to trackId,
        ),
    )

    suspend fun appendTidalTrack(
        baseUrl: String,
        rendererLocation: String,
        track: TidalTrackDto,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/tidal/append-track",
        tidalTrackFormFields(rendererLocation, track, clientId),
    )

    suspend fun appendTidalAlbum(
        baseUrl: String,
        rendererLocation: String,
        album: TidalAlbumDto,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/tidal/append-album",
        tidalAlbumFormFields(rendererLocation, album, clientId),
    )

    suspend fun playNextTidalTrack(
        baseUrl: String,
        rendererLocation: String,
        track: TidalTrackDto,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/tidal/play-next-track",
        tidalTrackFormFields(rendererLocation, track, clientId),
    )

    suspend fun playNextTidalAlbum(
        baseUrl: String,
        rendererLocation: String,
        album: TidalAlbumDto,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/tidal/play-next-album",
        tidalAlbumFormFields(rendererLocation, album, clientId),
    )

    suspend fun appendAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/append-album",
        mapOf(
            "renderer_location" to rendererLocation,
            "client_id" to clientId,
            "album_id" to albumId,
        ),
    )

    suspend fun playNextAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/play-next-album",
        mapOf(
            "renderer_location" to rendererLocation,
            "client_id" to clientId,
            "album_id" to albumId,
        ),
    )

    suspend fun likeItem(
        baseUrl: String,
        itemKind: String,
        itemId: String,
        clientId: String,
    ): LikeResponseDto = post(
        "$baseUrl/api/like",
        mapOf(
            "item_kind" to itemKind,
            "item_id" to itemId,
            "client_id" to clientId,
        ),
    )

    suspend fun moveQueueEntry(
        baseUrl: String,
        rendererLocation: String,
        entryId: Long,
        direction: String,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/move",
        mapOf(
            "renderer_location" to rendererLocation,
            "client_id" to clientId,
            "entry_id" to entryId.toString(),
            "direction" to direction,
        ),
    )

    suspend fun removeQueueEntry(
        baseUrl: String,
        rendererLocation: String,
        entryId: Long,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/remove",
        mapOf(
            "renderer_location" to rendererLocation,
            "client_id" to clientId,
            "entry_id" to entryId.toString(),
        ),
    )

    suspend fun clearQueue(
        baseUrl: String,
        rendererLocation: String,
        clientId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/clear",
        mapOf("renderer_location" to rendererLocation, "client_id" to clientId),
    )

    fun observePlaybackEvents(
        baseUrl: String,
        rendererLocation: String,
        clientId: String,
        onEvent: (PlaybackEventDto) -> Unit,
    ) {
        val request = Request.Builder()
            .url("$baseUrl/api/events?renderer_location=${rendererLocation.encodeForUrl()}&client_id=${clientId.encodeForUrl()}")
            .addHeader("Accept", "text/event-stream")
            .get()
            .build()

        try {
            client.newCall(request).execute().use { response ->
                if (!response.isSuccessful) {
                    response.requireSuccessfulBody(json)
                    return
                }
                val reader = response.body?.charStream()?.buffered()
                    ?: throw MusicdApiException.InvalidResponse("musicd returned an empty event stream.")
                var eventName: String? = null
                val dataLines = mutableListOf<String>()
                reader.useLines { lines ->
                    lines.forEach { line ->
                        when {
                            line.isEmpty() -> {
                                if (eventName == "playback" && dataLines.isNotEmpty()) {
                                    val payload = dataLines.joinToString("\n")
                                    onEvent(
                                        decodeBody<PlaybackEventDto>(payload, "$baseUrl/api/events")
                                    )
                                }
                                eventName = null
                                dataLines.clear()
                            }
                            line.startsWith(":") -> Unit
                            line.startsWith("event:") -> {
                                eventName = line.substringAfter(':').trim()
                            }
                            line.startsWith("data:") -> {
                                dataLines += line.substringAfter(':').trimStart()
                            }
                        }
                    }
                }
            }
        } catch (error: MusicdApiException) {
            throw error
        } catch (error: UnknownHostException) {
            throw MusicdApiException.Network(
                "Couldn't find that server. Check the address and try again.",
                error,
            )
        } catch (error: ConnectException) {
            throw MusicdApiException.Network(
                "Couldn't connect to musicd at that address.",
                error,
            )
        } catch (error: SocketTimeoutException) {
            throw MusicdApiException.Network(
                "musicd took too long to respond.",
                error,
            )
        } catch (error: UnknownServiceException) {
            val message = if (error.message.orEmpty().contains("CLEARTEXT", ignoreCase = true)) {
                "This server must use a normal http:// LAN address."
            } else {
                "The server connection type is not supported."
            }
            throw MusicdApiException.Network(message, error)
        } catch (error: IOException) {
            throw MusicdApiException.Network(
                "Network error while listening to musicd events.",
                error,
            )
        }
    }

    private suspend inline fun <reified T> get(url: String): T {
        val request = Request.Builder().url(url).get().build()
        val body = executeRequest(request)
        return decodeBody(body, url)
    }

    private suspend inline fun <reified T> post(
        url: String,
        formFields: Map<String, String>,
    ): T {
        val bodyBuilder = FormBody.Builder()
        for ((key, value) in formFields) {
            bodyBuilder.add(key, value)
        }
        val request = Request.Builder()
            .url(url)
            .post(bodyBuilder.build())
            .build()
        val body = executeRequest(request)
        return decodeBody(body, url)
    }

    private fun tidalTrackFormFields(
        rendererLocation: String,
        track: TidalTrackDto,
        clientId: String,
    ): Map<String, String> = buildMap {
        put("renderer_location", rendererLocation)
        put("client_id", clientId)
        put("tidal_track_id", track.trackId)
        track.title.takeIf { it.isNotBlank() }?.let { put("title", it) }
        track.artist?.takeIf { it.isNotBlank() }?.let { put("artist", it) }
        track.album?.takeIf { it.isNotBlank() }?.let { put("album", it) }
        track.durationSeconds?.let { put("duration_seconds", it.toString()) }
        track.artworkUrl?.takeIf { it.isNotBlank() }?.let { put("artwork_url", it) }
    }

    private fun tidalAlbumFormFields(
        rendererLocation: String,
        album: TidalAlbumDto,
        clientId: String,
    ): Map<String, String> = buildMap {
        put("renderer_location", rendererLocation)
        put("client_id", clientId)
        put("tidal_album_id", album.albumId)
        album.title.takeIf { it.isNotBlank() }?.let { put("title", it) }
        album.artist?.takeIf { it.isNotBlank() }?.let { put("artist", it) }
        album.trackCount?.let { put("track_count", it.toString()) }
        album.durationSeconds?.let { put("duration_seconds", it.toString()) }
        album.artworkUrl?.takeIf { it.isNotBlank() }?.let { put("artwork_url", it) }
    }

    private fun executeRequest(request: Request): String {
        try {
            client.newCall(request).execute().use { response ->
                return response.requireSuccessfulBody(json)
            }
        } catch (error: MusicdApiException) {
            throw error
        } catch (error: UnknownHostException) {
            throw MusicdApiException.Network(
                "Couldn't find that server. Check the address and try again.",
                error,
            )
        } catch (error: ConnectException) {
            throw MusicdApiException.Network(
                "Couldn't connect to musicd at that address.",
                error,
            )
        } catch (error: SocketTimeoutException) {
            throw MusicdApiException.Network(
                "musicd took too long to respond.",
                error,
            )
        } catch (error: UnknownServiceException) {
            val message = if (error.message.orEmpty().contains("CLEARTEXT", ignoreCase = true)) {
                "This server must use a normal http:// LAN address."
            } else {
                "The server connection type is not supported."
            }
            throw MusicdApiException.Network(message, error)
        } catch (error: IOException) {
            throw MusicdApiException.Network(
                "Network error while talking to musicd.",
                error,
            )
        }
    }

    private inline fun <reified T> decodeBody(body: String, sourceUrl: String): T =
        try {
            json.decodeFromString(body)
        } catch (error: SerializationException) {
            throw MusicdApiException.InvalidResponse(
                "musicd returned unexpected JSON from ${sourceUrl.musicdEndpointLabel()}.",
                error,
            )
        }
}

private fun Response.requireSuccessfulBody(json: Json): String {
    val bodyText = body?.string().orEmpty()
    if (!isSuccessful) {
        val errorMessage = parseApiError(json, bodyText)
        throw MusicdApiException.Http(
            statusCode = code,
            serverMessage = errorMessage,
            userMessage = friendlyHttpMessage(code, errorMessage),
        )
    }
    if (bodyText.isBlank()) {
        throw MusicdApiException.InvalidResponse("musicd returned an empty response.")
    }
    return bodyText
}

private fun String.encodeForUrl(): String =
    java.net.URLEncoder.encode(this, Charsets.UTF_8.name())

private fun parseApiError(json: Json, body: String): String? =
    try {
        val envelope = json.decodeFromString<ErrorEnvelopeDto>(body)
        envelope.error ?: envelope.message
    } catch (_: SerializationException) {
        null
    }

private fun friendlyHttpMessage(statusCode: Int, serverMessage: String?): String =
    when (statusCode) {
        400 -> serverMessage ?: "musicd rejected that request."
        403 -> if (serverMessage?.contains("pairing", ignoreCase = true) == true) {
            "The local companion pairing is stale. Re-select local companion mode or reset pairing in the companion app."
        } else {
            serverMessage ?: "musicd rejected this controller."
        }
        404 -> serverMessage ?: "That server responded, but it does not look like a musicd instance."
        in 500..599 -> serverMessage ?: "musicd responded with a server error."
        else -> serverMessage ?: "musicd request failed ($statusCode)."
    }

private fun String.musicdEndpointLabel(): String =
    substringAfter("://", this)
        .substringAfter('/', missingDelimiterValue = "")
        .substringBefore('?')
        .takeIf { it.isNotBlank() }
        ?.let { "/$it" }
        ?: this
