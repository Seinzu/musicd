package io.musicd.android.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import okhttp3.FormBody
import okhttp3.OkHttpClient
import okhttp3.Request

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
    val error: String? = null,
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
)

@Serializable
data class AlbumSummaryDto(
    val id: String,
    val title: String,
    val artist: String,
    @SerialName("track_count") val trackCount: Int,
    @SerialName("first_track_id") val firstTrackId: String,
    @SerialName("artwork_url") val artworkUrl: String,
)

@Serializable
data class AlbumDetailDto(
    val id: String,
    val title: String,
    val artist: String,
    @SerialName("track_count") val trackCount: Int,
    @SerialName("first_track_id") val firstTrackId: String,
    @SerialName("artwork_url") val artworkUrl: String,
    val tracks: List<TrackSummaryDto> = emptyList(),
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
data class MutationResponseDto(
    val ok: Boolean,
    val message: String? = null,
    val error: String? = null,
    @SerialName("renderer_location") val rendererLocation: String? = null,
    val queue: QueueDto? = null,
    val session: SessionDto? = null,
)

class MusicdApi(
    private val client: OkHttpClient = OkHttpClient(),
    private val json: Json = Json { ignoreUnknownKeys = true },
) {
    suspend fun getAlbums(baseUrl: String): List<AlbumSummaryDto> =
        get("$baseUrl/api/albums")

    suspend fun getAlbumDetail(baseUrl: String, albumId: String): AlbumDetailDto =
        get("$baseUrl/api/albums/${albumId.encodeForUrl()}")

    suspend fun getTracks(baseUrl: String): List<TrackSummaryDto> =
        get("$baseUrl/api/tracks")

    suspend fun getRenderers(baseUrl: String): List<RendererDto> =
        get("$baseUrl/api/renderers")

    suspend fun discoverRenderers(baseUrl: String): List<RendererDto> =
        get("$baseUrl/api/renderers/discover")

    suspend fun getQueue(baseUrl: String, rendererLocation: String): QueueDto =
        get("$baseUrl/api/queue?renderer_location=${rendererLocation.encodeForUrl()}")

    suspend fun getNowPlaying(baseUrl: String, rendererLocation: String): NowPlayingDto =
        get("$baseUrl/api/now-playing?renderer_location=${rendererLocation.encodeForUrl()}")

    suspend fun transport(baseUrl: String, path: String, rendererLocation: String): MutationResponseDto =
        post(
            "$baseUrl$path",
            mapOf("renderer_location" to rendererLocation),
        )

    suspend fun playTrack(
        baseUrl: String,
        rendererLocation: String,
        trackId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/play",
        mapOf(
            "renderer_location" to rendererLocation,
            "track_id" to trackId,
        ),
    )

    suspend fun playAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/play-album",
        mapOf(
            "renderer_location" to rendererLocation,
            "album_id" to albumId,
        ),
    )

    suspend fun appendTrack(
        baseUrl: String,
        rendererLocation: String,
        trackId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/append-track",
        mapOf(
            "renderer_location" to rendererLocation,
            "track_id" to trackId,
        ),
    )

    suspend fun playNextTrack(
        baseUrl: String,
        rendererLocation: String,
        trackId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/play-next-track",
        mapOf(
            "renderer_location" to rendererLocation,
            "track_id" to trackId,
        ),
    )

    suspend fun appendAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/append-album",
        mapOf(
            "renderer_location" to rendererLocation,
            "album_id" to albumId,
        ),
    )

    suspend fun playNextAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/play-next-album",
        mapOf(
            "renderer_location" to rendererLocation,
            "album_id" to albumId,
        ),
    )

    suspend fun moveQueueEntry(
        baseUrl: String,
        rendererLocation: String,
        entryId: Long,
        direction: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/move",
        mapOf(
            "renderer_location" to rendererLocation,
            "entry_id" to entryId.toString(),
            "direction" to direction,
        ),
    )

    suspend fun removeQueueEntry(
        baseUrl: String,
        rendererLocation: String,
        entryId: Long,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/remove",
        mapOf(
            "renderer_location" to rendererLocation,
            "entry_id" to entryId.toString(),
        ),
    )

    suspend fun clearQueue(
        baseUrl: String,
        rendererLocation: String,
    ): MutationResponseDto = post(
        "$baseUrl/api/queue/clear",
        mapOf("renderer_location" to rendererLocation),
    )

    private suspend inline fun <reified T> get(url: String): T {
        val request = Request.Builder().url(url).get().build()
        val response = client.newCall(request).execute()
        if (!response.isSuccessful) {
            error("Request failed: ${response.code}")
        }
        val body = response.body?.string().orEmpty()
        return json.decodeFromString(body)
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
        val response = client.newCall(request).execute()
        val body = response.body?.string().orEmpty()
        if (!response.isSuccessful) {
            error("Request failed: ${response.code} $body")
        }
        return json.decodeFromString(body)
    }
}

private fun String.encodeForUrl(): String =
    java.net.URLEncoder.encode(this, Charsets.UTF_8.name())
