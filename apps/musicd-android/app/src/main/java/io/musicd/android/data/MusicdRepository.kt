package io.musicd.android.data

import android.content.Context
import androidx.core.content.edit
import io.musicd.android.data.AlbumDetailDto
import io.musicd.android.data.AlbumArtworkCandidatesResponseDto
import io.musicd.android.data.ArtistDetailDto
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

class MusicdRepository(
    context: Context,
    private val api: MusicdApi = MusicdApi(),
) {
    private val prefs = context.getSharedPreferences("musicd_android", Context.MODE_PRIVATE)

    fun loadBaseUrl(): String = prefs.getString(KEY_BASE_URL, "").orEmpty()

    fun loadRendererLocation(): String = prefs.getString(KEY_RENDERER_LOCATION, "").orEmpty()

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
        api.getAlbums(baseUrl.normalizeBaseUrl())
    }

    suspend fun getArtists(baseUrl: String): List<ArtistSummaryDto> = withContext(Dispatchers.IO) {
        api.getArtists(baseUrl.normalizeBaseUrl())
    }

    suspend fun getAlbumDetail(baseUrl: String, albumId: String): AlbumDetailDto =
        withContext(Dispatchers.IO) {
            api.getAlbumDetail(baseUrl.normalizeBaseUrl(), albumId)
        }

    suspend fun getAlbumArtworkCandidates(
        baseUrl: String,
        albumId: String,
    ): AlbumArtworkCandidatesResponseDto = withContext(Dispatchers.IO) {
        api.getAlbumArtworkCandidates(baseUrl.normalizeBaseUrl(), albumId)
    }

    suspend fun getArtistDetail(baseUrl: String, artistId: String): ArtistDetailDto =
        withContext(Dispatchers.IO) {
            api.getArtistDetail(baseUrl.normalizeBaseUrl(), artistId)
        }

    suspend fun getTracks(baseUrl: String): List<TrackSummaryDto> = withContext(Dispatchers.IO) {
        api.getTracks(baseUrl.normalizeBaseUrl())
    }

    suspend fun getRenderers(baseUrl: String): List<RendererDto> = withContext(Dispatchers.IO) {
        api.getRenderers(baseUrl.normalizeBaseUrl())
    }

    suspend fun discoverRenderers(baseUrl: String): List<RendererDto> =
        withContext(Dispatchers.IO) {
            api.discoverRenderers(baseUrl.normalizeBaseUrl())
        }

    suspend fun getQueue(baseUrl: String, rendererLocation: String): QueueDto =
        withContext(Dispatchers.IO) {
            api.getQueue(baseUrl.normalizeBaseUrl(), rendererLocation)
        }

    suspend fun getNowPlaying(baseUrl: String, rendererLocation: String): NowPlayingDto =
        withContext(Dispatchers.IO) {
            api.getNowPlaying(baseUrl.normalizeBaseUrl(), rendererLocation)
        }

    suspend fun transportPlay(baseUrl: String, rendererLocation: String): MutationResponseDto =
        transport(baseUrl, "/api/transport/play", rendererLocation)

    suspend fun playTrack(
        baseUrl: String,
        rendererLocation: String,
        trackId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playTrack(baseUrl.normalizeBaseUrl(), rendererLocation, trackId)
    }

    suspend fun playAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playAlbum(baseUrl.normalizeBaseUrl(), rendererLocation, albumId)
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
        api.appendTrack(baseUrl.normalizeBaseUrl(), rendererLocation, trackId)
    }

    suspend fun playNextTrack(
        baseUrl: String,
        rendererLocation: String,
        trackId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playNextTrack(baseUrl.normalizeBaseUrl(), rendererLocation, trackId)
    }

    suspend fun appendAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.appendAlbum(baseUrl.normalizeBaseUrl(), rendererLocation, albumId)
    }

    suspend fun playNextAlbum(
        baseUrl: String,
        rendererLocation: String,
        albumId: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.playNextAlbum(baseUrl.normalizeBaseUrl(), rendererLocation, albumId)
    }

    suspend fun moveQueueEntry(
        baseUrl: String,
        rendererLocation: String,
        entryId: Long,
        direction: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.moveQueueEntry(baseUrl.normalizeBaseUrl(), rendererLocation, entryId, direction)
    }

    suspend fun removeQueueEntry(
        baseUrl: String,
        rendererLocation: String,
        entryId: Long,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.removeQueueEntry(baseUrl.normalizeBaseUrl(), rendererLocation, entryId)
    }

    suspend fun clearQueue(
        baseUrl: String,
        rendererLocation: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.clearQueue(baseUrl.normalizeBaseUrl(), rendererLocation)
    }

    suspend fun transportPause(baseUrl: String, rendererLocation: String): MutationResponseDto =
        transport(baseUrl, "/api/transport/pause", rendererLocation)

    suspend fun transportStop(baseUrl: String, rendererLocation: String): MutationResponseDto =
        transport(baseUrl, "/api/transport/stop", rendererLocation)

    suspend fun transportNext(baseUrl: String, rendererLocation: String): MutationResponseDto =
        transport(baseUrl, "/api/transport/next", rendererLocation)

    suspend fun transportPrevious(baseUrl: String, rendererLocation: String): MutationResponseDto =
        transport(baseUrl, "/api/transport/previous", rendererLocation)

    private suspend fun transport(
        baseUrl: String,
        path: String,
        rendererLocation: String,
    ): MutationResponseDto = withContext(Dispatchers.IO) {
        api.transport(baseUrl.normalizeBaseUrl(), path, rendererLocation)
    }

    companion object {
        private const val KEY_BASE_URL = "base_url"
        private const val KEY_RENDERER_LOCATION = "renderer_location"
    }
}

private fun String.normalizeBaseUrl(): String = trim().trimEnd('/')
