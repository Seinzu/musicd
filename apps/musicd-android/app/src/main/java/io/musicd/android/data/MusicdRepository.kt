package io.musicd.android.data

import android.content.Context
import androidx.core.content.edit
import io.musicd.android.data.AlbumDetailDto
import io.musicd.android.data.AlbumArtworkCandidatesResponseDto
import io.musicd.android.data.AlbumRecommendationsResponseDto
import io.musicd.android.data.ArtistDetailDto
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.util.UUID

class MusicdRepository(
    context: Context,
    private val api: MusicdApi = MusicdApi(),
    private val discovery: MusicdDiscovery = MusicdDiscovery(context),
) : MusicSourceRepository {
    override val sourceKind: MusicSourceKind = MusicSourceKind.RemoteServer

    private val prefs = context.getSharedPreferences("musicd_android", Context.MODE_PRIVATE)

    suspend fun discoverServers(timeoutMillis: Long = MusicdDiscovery.DEFAULT_TIMEOUT_MS): List<DiscoveredServer> =
        discovery.discoverServers(timeoutMillis)

    fun loadBaseUrl(): String = prefs.getString(KEY_BASE_URL, "").orEmpty()

    fun loadRendererLocation(): String = prefs.getString(KEY_RENDERER_LOCATION, "").orEmpty()

    fun loadSourceKind(): MusicSourceKind =
        when (prefs.getString(KEY_SOURCE_KIND, MusicSourceKind.RemoteServer.name)) {
            MusicSourceKind.LocalCompanion.name -> MusicSourceKind.LocalCompanion
            else -> MusicSourceKind.RemoteServer
        }

    fun saveSourceKind(sourceKind: MusicSourceKind) {
        prefs.edit { putString(KEY_SOURCE_KIND, sourceKind.name) }
    }

    fun loadClientId(): String {
        val existing = prefs.getString(KEY_CLIENT_ID, "").orEmpty()
        if (existing.isNotBlank()) return existing
        val generated = UUID.randomUUID().toString()
        prefs.edit { putString(KEY_CLIENT_ID, generated) }
        return generated
    }

    fun saveBaseUrl(baseUrl: String) {
        prefs.edit { putString(KEY_BASE_URL, baseUrl) }
    }

    fun saveRendererLocation(rendererLocation: String) {
        prefs.edit { putString(KEY_RENDERER_LOCATION, rendererLocation) }
    }

    fun clearBaseUrl() {
        prefs.edit { remove(KEY_BASE_URL) }
    }

    fun clearRendererLocation() {
        prefs.edit { remove(KEY_RENDERER_LOCATION) }
    }

    suspend fun getServerInfo(baseUrl: String): ServerInfoDto = withContext(Dispatchers.IO) {
        api.getServerInfo(baseUrl.normalizeBaseUrl())
    }

    suspend fun getAlbums(baseUrl: String): List<AlbumSummaryDto> = withContext(Dispatchers.IO) {
        api.getAlbums(baseUrl.normalizeBaseUrl(), loadClientId())
    }

    suspend fun getArtists(baseUrl: String): List<ArtistSummaryDto> = withContext(Dispatchers.IO) {
        api.getArtists(baseUrl.normalizeBaseUrl())
    }

    suspend fun getAlbumDetail(baseUrl: String, albumId: String): AlbumDetailDto =
        withContext(Dispatchers.IO) {
            api.getAlbumDetail(baseUrl.normalizeBaseUrl(), albumId, loadClientId())
        }

    suspend fun getAlbumArtworkCandidates(
        baseUrl: String,
        albumId: String,
    ): AlbumArtworkCandidatesResponseDto = withContext(Dispatchers.IO) {
        api.getAlbumArtworkCandidates(baseUrl.normalizeBaseUrl(), albumId)
    }

    suspend fun getAlbumRecommendations(
        baseUrl: String,
        albumId: String,
    ): AlbumRecommendationsResponseDto = withContext(Dispatchers.IO) {
        api.getAlbumRecommendations(baseUrl.normalizeBaseUrl(), albumId)
    }

    suspend fun getCollectionRecommendations(
        baseUrl: String,
        limit: Int = 5,
    ): AlbumRecommendationsResponseDto = withContext(Dispatchers.IO) {
        api.getCollectionRecommendations(baseUrl.normalizeBaseUrl(), limit)
    }

    suspend fun searchRadioStations(
        baseUrl: String,
        query: String,
        countryCode: String,
        limit: Int = 24,
    ): List<RadioStationDto> = withContext(Dispatchers.IO) {
        api.searchRadioStations(baseUrl.normalizeBaseUrl(), query, countryCode, limit)
    }

    suspend fun searchTidalTracks(
        baseUrl: String,
        query: String,
        limit: Int = 24,
    ): List<TidalTrackDto> = withContext(Dispatchers.IO) {
        api.searchTidalTracks(baseUrl.normalizeBaseUrl(), query, limit)
    }

    suspend fun searchTidalAlbums(
        baseUrl: String,
        query: String,
        limit: Int = 12,
    ): List<TidalAlbumDto> = withContext(Dispatchers.IO) {
        api.searchTidalAlbums(baseUrl.normalizeBaseUrl(), query, limit)
    }

    suspend fun getArtistDetail(baseUrl: String, artistId: String): ArtistDetailDto =
        withContext(Dispatchers.IO) {
            api.getArtistDetail(baseUrl.normalizeBaseUrl(), artistId, loadClientId())
        }

    suspend fun getTracks(baseUrl: String): List<TrackSummaryDto> = withContext(Dispatchers.IO) {
        api.getTracks(baseUrl.normalizeBaseUrl(), loadClientId())
    }

    suspend fun getRenderers(baseUrl: String): List<RendererDto> = withContext(Dispatchers.IO) {
        api.getRenderers(baseUrl.normalizeBaseUrl(), loadClientId())
    }

    suspend fun discoverRenderers(baseUrl: String): List<RendererDto> =
        withContext(Dispatchers.IO) {
            api.discoverRenderers(baseUrl.normalizeBaseUrl())
        }

    suspend fun getRendererVolume(baseUrl: String, rendererLocation: String): RendererVolumeDto =
        withContext(Dispatchers.IO) {
            api.getRendererVolume(baseUrl.normalizeBaseUrl(), rendererLocation, loadClientId())
        }

    suspend fun setRendererVolume(
        baseUrl: String,
        rendererLocation: String,
        volume: Int,
    ): RendererVolumeDto = withContext(Dispatchers.IO) {
        api.setRendererVolume(
            baseUrl.normalizeBaseUrl(),
            rendererLocation,
            volume,
            loadClientId(),
        )
    }

    suspend fun createRendererGroup(
        baseUrl: String,
        name: String,
        memberLocations: List<String>,
        sourceRendererLocation: String?,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.createRendererGroup(
            baseUrl.normalizeBaseUrl(),
            name,
            memberLocations,
            sourceRendererLocation,
            loadClientId(),
        )
    }

    suspend fun deleteRendererGroup(
        baseUrl: String,
        rendererLocation: String,
        inheritRendererLocation: String? = null,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.deleteRendererGroup(
            baseUrl.normalizeBaseUrl(),
            rendererLocation,
            loadClientId(),
            inheritRendererLocation,
        )
    }

    suspend fun updateRendererGroup(
        baseUrl: String,
        rendererLocation: String,
        name: String,
        memberLocations: List<String>,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.updateRendererGroup(
            baseUrl.normalizeBaseUrl(),
            rendererLocation,
            name,
            memberLocations,
            loadClientId(),
        )
    }

    suspend fun registerAndroidLocalRenderer(
        baseUrl: String,
        rendererLocation: String,
        name: String,
        manufacturer: String?,
        modelName: String?,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.registerAndroidLocalRenderer(
            baseUrl.normalizeBaseUrl(),
            rendererLocation,
            name,
            manufacturer,
            modelName,
            loadClientId(),
        )
    }

    suspend fun reportAndroidLocalSession(
        baseUrl: String,
        rendererLocation: String,
        transportState: String,
        currentTrackUri: String?,
        positionSeconds: Long?,
        durationSeconds: Long?,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.reportAndroidLocalSession(
            baseUrl.normalizeBaseUrl(),
            rendererLocation,
            transportState,
            currentTrackUri,
            positionSeconds,
            durationSeconds,
            loadClientId(),
        )
    }

    suspend fun reportAndroidLocalCompleted(
        baseUrl: String,
        rendererLocation: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.reportAndroidLocalCompleted(baseUrl.normalizeBaseUrl(), rendererLocation, loadClientId())
    }

    suspend fun getQueue(baseUrl: String, rendererLocation: String): QueueDto =
        withContext(Dispatchers.IO) {
            api.getQueue(baseUrl.normalizeBaseUrl(), rendererLocation, loadClientId())
        }

    suspend fun getNowPlaying(baseUrl: String, rendererLocation: String): NowPlayingDto =
        withContext(Dispatchers.IO) {
            api.getNowPlaying(baseUrl.normalizeBaseUrl(), rendererLocation, loadClientId())
        }

    suspend fun transportPlay(baseUrl: String, rendererLocation: String): MutationResponseDto =
        transport(baseUrl, "/api/transport/play", rendererLocation)

    suspend fun playTrack(
        baseUrl: String,
        rendererLocation: String,
        trackId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playTrack(baseUrl.normalizeBaseUrl(), rendererLocation, trackId, loadClientId())
    }

    suspend fun playAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playAlbum(baseUrl.normalizeBaseUrl(), rendererLocation, albumId, loadClientId())
    }

    suspend fun playRadioStation(
        baseUrl: String,
        rendererLocation: String,
        station: RadioStationDto,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playRadioStation(baseUrl.normalizeBaseUrl(), rendererLocation, station, loadClientId())
    }

    suspend fun playTidalTrack(
        baseUrl: String,
        rendererLocation: String,
        track: TidalTrackDto,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playTidalTrack(baseUrl.normalizeBaseUrl(), rendererLocation, track, loadClientId())
    }

    suspend fun playTidalAlbum(
        baseUrl: String,
        rendererLocation: String,
        album: TidalAlbumDto,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playTidalAlbum(baseUrl.normalizeBaseUrl(), rendererLocation, album, loadClientId())
    }

    suspend fun selectAlbumArtwork(
        baseUrl: String,
        albumId: String,
        releaseId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.selectAlbumArtwork(baseUrl.normalizeBaseUrl(), albumId, releaseId)
    }

    suspend fun appendTrack(
        baseUrl: String,
        rendererLocation: String,
        trackId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.appendTrack(baseUrl.normalizeBaseUrl(), rendererLocation, trackId, loadClientId())
    }

    suspend fun playNextTrack(
        baseUrl: String,
        rendererLocation: String,
        trackId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playNextTrack(baseUrl.normalizeBaseUrl(), rendererLocation, trackId, loadClientId())
    }

    suspend fun appendTidalTrack(
        baseUrl: String,
        rendererLocation: String,
        track: TidalTrackDto,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.appendTidalTrack(baseUrl.normalizeBaseUrl(), rendererLocation, track, loadClientId())
    }

    suspend fun appendTidalAlbum(
        baseUrl: String,
        rendererLocation: String,
        album: TidalAlbumDto,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.appendTidalAlbum(baseUrl.normalizeBaseUrl(), rendererLocation, album, loadClientId())
    }

    suspend fun playNextTidalTrack(
        baseUrl: String,
        rendererLocation: String,
        track: TidalTrackDto,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playNextTidalTrack(baseUrl.normalizeBaseUrl(), rendererLocation, track, loadClientId())
    }

    suspend fun playNextTidalAlbum(
        baseUrl: String,
        rendererLocation: String,
        album: TidalAlbumDto,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playNextTidalAlbum(baseUrl.normalizeBaseUrl(), rendererLocation, album, loadClientId())
    }

    suspend fun appendAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.appendAlbum(baseUrl.normalizeBaseUrl(), rendererLocation, albumId, loadClientId())
    }

    suspend fun playNextAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playNextAlbum(baseUrl.normalizeBaseUrl(), rendererLocation, albumId, loadClientId())
    }

    suspend fun likeAlbum(baseUrl: String, albumId: String): LikeResponseDto =
        likeItem(baseUrl, "album", albumId)

    suspend fun likeTrack(baseUrl: String, trackId: String): LikeResponseDto =
        likeItem(baseUrl, "track", trackId)

    private suspend fun likeItem(
        baseUrl: String,
        itemKind: String,
        itemId: String,
    ): LikeResponseDto = withContext(Dispatchers.IO) {
        api.likeItem(baseUrl.normalizeBaseUrl(), itemKind, itemId, loadClientId())
    }

    suspend fun moveQueueEntry(
        baseUrl: String,
        rendererLocation: String,
        entryId: Long,
        direction: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.moveQueueEntry(baseUrl.normalizeBaseUrl(), rendererLocation, entryId, direction, loadClientId())
    }

    suspend fun removeQueueEntry(
        baseUrl: String,
        rendererLocation: String,
        entryId: Long,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.removeQueueEntry(baseUrl.normalizeBaseUrl(), rendererLocation, entryId, loadClientId())
    }

    suspend fun clearQueue(
        baseUrl: String,
        rendererLocation: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.clearQueue(baseUrl.normalizeBaseUrl(), rendererLocation, loadClientId())
    }

    suspend fun observePlaybackEvents(
        baseUrl: String,
        rendererLocation: String,
        onEvent: (PlaybackEventDto) -> Unit,
    ) = withContext(Dispatchers.IO) {
        api.observePlaybackEvents(baseUrl.normalizeBaseUrl(), rendererLocation, loadClientId(), onEvent)
    }

    suspend fun transportPause(baseUrl: String, rendererLocation: String): MutationResponseDto =
        transport(baseUrl, "/api/transport/pause", rendererLocation)

    suspend fun transportStop(baseUrl: String, rendererLocation: String): MutationResponseDto =
        transport(baseUrl, "/api/transport/stop", rendererLocation)

    suspend fun transportNext(baseUrl: String, rendererLocation: String): MutationResponseDto =
        transport(baseUrl, "/api/transport/next", rendererLocation)

    suspend fun transportPrevious(baseUrl: String, rendererLocation: String): MutationResponseDto =
        transport(baseUrl, "/api/transport/previous", rendererLocation)

    suspend fun transportSeek(
        baseUrl: String,
        rendererLocation: String,
        positionSeconds: Long,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.transportSeek(
            baseUrl.normalizeBaseUrl(),
            rendererLocation,
            positionSeconds.coerceAtLeast(0L),
            loadClientId(),
        )
    }

    private suspend fun transport(
        baseUrl: String,
        path: String,
        rendererLocation: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.transport(baseUrl.normalizeBaseUrl(), path, rendererLocation, loadClientId())
    }

    companion object {
        private const val KEY_BASE_URL = "base_url"
        private const val KEY_RENDERER_LOCATION = "renderer_location"
        private const val KEY_CLIENT_ID = "client_id"
        private const val KEY_SOURCE_KIND = "source_kind"
    }
}

private fun String.normalizeBaseUrl(): String = trim().trimEnd('/')
