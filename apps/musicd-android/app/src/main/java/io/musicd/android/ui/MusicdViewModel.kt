package io.musicd.android.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import io.musicd.android.data.AlbumDetailDto
import io.musicd.android.data.AlbumSummaryDto
import io.musicd.android.data.MusicdRepository
import io.musicd.android.data.NowPlayingDto
import io.musicd.android.data.QueueDto
import io.musicd.android.data.RendererDto
import io.musicd.android.data.TrackSummaryDto
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

enum class MusicdTab {
    Home,
    Library,
    Queue,
}

data class MusicdUiState(
    val serverInput: String = "",
    val baseUrl: String = "",
    val connected: Boolean = false,
    val selectedTab: MusicdTab = MusicdTab.Home,
    val searchQuery: String = "",
    val selectedRendererLocation: String = "",
    val renderers: List<RendererDto> = emptyList(),
    val nowPlaying: NowPlayingDto? = null,
    val albums: List<AlbumSummaryDto> = emptyList(),
    val tracks: List<TrackSummaryDto> = emptyList(),
    val selectedAlbumDetail: AlbumDetailDto? = null,
    val queue: QueueDto? = null,
    val showRendererPicker: Boolean = false,
    val isLoading: Boolean = false,
    val isDiscovering: Boolean = false,
    val errorMessage: String? = null,
    val infoMessage: String? = null,
)

class MusicdViewModel(application: Application) : AndroidViewModel(application) {
    private val repository = MusicdRepository(application)
    private val _uiState = MutableStateFlow(
        MusicdUiState(serverInput = repository.loadBaseUrl()),
    )
    val uiState: StateFlow<MusicdUiState> = _uiState.asStateFlow()

    init {
        val savedBaseUrl = repository.loadBaseUrl()
        if (savedBaseUrl.isNotBlank()) {
            connect(savedBaseUrl)
        }
    }

    fun updateServerInput(value: String) {
        _uiState.update { it.copy(serverInput = value) }
    }

    fun updateSearchQuery(value: String) {
        _uiState.update { it.copy(searchQuery = value) }
    }

    fun connect(baseUrl: String = uiState.value.serverInput) {
        val normalized = baseUrl.trim().trimEnd('/')
        if (normalized.isBlank()) {
            _uiState.update { it.copy(errorMessage = "Enter the musicd server URL first.") }
            return
        }
        repository.saveBaseUrl(normalized)
        _uiState.update {
            it.copy(
                baseUrl = normalized,
                connected = true,
                serverInput = normalized,
                isLoading = true,
                errorMessage = null,
            )
        }
        refreshAll()
    }

    fun selectTab(tab: MusicdTab) {
        _uiState.update { it.copy(selectedTab = tab) }
    }

    fun toggleRendererPicker(show: Boolean) {
        _uiState.update { it.copy(showRendererPicker = show) }
    }

    fun selectRenderer(location: String) {
        repository.saveRendererLocation(location)
        _uiState.update {
            it.copy(
                selectedRendererLocation = location,
                showRendererPicker = false,
                infoMessage = "Renderer updated.",
            )
        }
        refreshPlaybackSurfaces()
    }

    fun refreshAll() {
        val baseUrl = uiState.value.baseUrl
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null) }
            runCatching {
                val renderersDeferred = async { repository.getRenderers(baseUrl) }
                val albumsDeferred = async { repository.getAlbums(baseUrl) }
                val tracksDeferred = async { repository.getTracks(baseUrl) }
                val renderers = renderersDeferred.await()
                val albums = albumsDeferred.await()
                val tracks = tracksDeferred.await()
                val rendererLocation = chooseRenderer(renderers)
                val nowPlaying = rendererLocation?.let { repository.getNowPlaying(baseUrl, it) }
                val queue = rendererLocation?.let { repository.getQueue(baseUrl, it) }
                Quintuple(renderers, albums, tracks, nowPlaying, queue)
            }.onSuccess { (renderers, albums, tracks, nowPlaying, queue) ->
                _uiState.update {
                    it.copy(
                        renderers = renderers,
                        albums = albums,
                        tracks = tracks,
                        selectedRendererLocation = nowPlaying?.rendererLocation
                            ?: it.selectedRendererLocation.ifBlank {
                                renderers.firstOrNull()?.location.orEmpty()
                            },
                        nowPlaying = nowPlaying,
                        queue = queue,
                        isLoading = false,
                    )
                }
            }.onFailure { error ->
                _uiState.update {
                    it.copy(
                        isLoading = false,
                        errorMessage = error.message ?: "Failed to load server data.",
                    )
                }
            }
        }
    }

    fun openAlbum(albumId: String) {
        val baseUrl = uiState.value.baseUrl
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null) }
            runCatching { repository.getAlbumDetail(baseUrl, albumId) }
                .onSuccess { album ->
                    _uiState.update {
                        it.copy(
                            selectedAlbumDetail = album,
                            isLoading = false,
                        )
                    }
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            errorMessage = error.message ?: "Failed to load album detail.",
                        )
                    }
                }
        }
    }

    fun closeAlbumDetail() {
        _uiState.update { it.copy(selectedAlbumDetail = null) }
    }

    fun discoverRenderers() {
        val baseUrl = uiState.value.baseUrl
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            _uiState.update { it.copy(isDiscovering = true, errorMessage = null) }
            runCatching { repository.discoverRenderers(baseUrl) }
                .onSuccess { discovered ->
                    val selected = chooseRenderer(discovered)
                    _uiState.update {
                        it.copy(
                            renderers = discovered,
                            selectedRendererLocation = selected ?: it.selectedRendererLocation,
                            isDiscovering = false,
                            infoMessage = "Renderer discovery refreshed.",
                        )
                    }
                    selected?.let { refreshPlaybackSurfaces() }
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isDiscovering = false,
                            errorMessage = error.message ?: "Renderer discovery failed.",
                        )
                    }
                }
        }
    }

    fun transportPlay() = transportAction { baseUrl, renderer ->
        repository.transportPlay(baseUrl, renderer)
    }

    fun transportPause() = transportAction { baseUrl, renderer ->
        repository.transportPause(baseUrl, renderer)
    }

    fun transportStop() = transportAction { baseUrl, renderer ->
        repository.transportStop(baseUrl, renderer)
    }

    fun transportNext() = transportAction { baseUrl, renderer ->
        repository.transportNext(baseUrl, renderer)
    }

    fun transportPrevious() = transportAction { baseUrl, renderer ->
        repository.transportPrevious(baseUrl, renderer)
    }

    fun playTrack(trackId: String) {
        val baseUrl = uiState.value.baseUrl
        val rendererLocation = uiState.value.selectedRendererLocation
        if (baseUrl.isBlank() || rendererLocation.isBlank()) {
            _uiState.update { it.copy(errorMessage = "Choose a renderer first.") }
            return
        }
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null) }
            runCatching { repository.playTrack(baseUrl, rendererLocation, trackId) }
                .onSuccess { response ->
                    _uiState.update { it.copy(infoMessage = response.message) }
                    refreshPlaybackSurfaces()
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            errorMessage = error.message ?: "Track playback failed.",
                        )
                    }
                }
        }
    }

    fun playAlbum(albumId: String) {
        val baseUrl = uiState.value.baseUrl
        val rendererLocation = uiState.value.selectedRendererLocation
        if (baseUrl.isBlank() || rendererLocation.isBlank()) {
            _uiState.update { it.copy(errorMessage = "Choose a renderer first.") }
            return
        }
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null) }
            runCatching { repository.playAlbum(baseUrl, rendererLocation, albumId) }
                .onSuccess { response ->
                    _uiState.update { it.copy(infoMessage = response.message) }
                    refreshPlaybackSurfaces()
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            errorMessage = error.message ?: "Album playback failed.",
                        )
                    }
                }
        }
    }

    fun appendTrack(trackId: String) = queueMutationAction("Track queued.") { baseUrl, renderer ->
        repository.appendTrack(baseUrl, renderer, trackId)
    }

    fun playNextTrack(trackId: String) = queueMutationAction("Track queued to play next.") { baseUrl, renderer ->
        repository.playNextTrack(baseUrl, renderer, trackId)
    }

    fun appendAlbum(albumId: String) = queueMutationAction("Album queued.") { baseUrl, renderer ->
        repository.appendAlbum(baseUrl, renderer, albumId)
    }

    fun playNextAlbum(albumId: String) = queueMutationAction("Album queued to play next.") { baseUrl, renderer ->
        repository.playNextAlbum(baseUrl, renderer, albumId)
    }

    fun moveQueueEntryUp(entryId: Long) = queueMutationAction("Queue order updated.") { baseUrl, renderer ->
        repository.moveQueueEntry(baseUrl, renderer, entryId, "up")
    }

    fun moveQueueEntryDown(entryId: Long) = queueMutationAction("Queue order updated.") { baseUrl, renderer ->
        repository.moveQueueEntry(baseUrl, renderer, entryId, "down")
    }

    fun removeQueueEntry(entryId: Long) = queueMutationAction("Queue entry removed.") { baseUrl, renderer ->
        repository.removeQueueEntry(baseUrl, renderer, entryId)
    }

    fun clearQueue() = queueMutationAction("Queue cleared.") { baseUrl, renderer ->
        repository.clearQueue(baseUrl, renderer)
    }

    private fun transportAction(
        action: suspend (String, String) -> Any,
    ) {
        val baseUrl = uiState.value.baseUrl
        val rendererLocation = uiState.value.selectedRendererLocation
        if (baseUrl.isBlank() || rendererLocation.isBlank()) {
            _uiState.update { it.copy(errorMessage = "Choose a renderer first.") }
            return
        }
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null) }
            runCatching { action(baseUrl, rendererLocation) }
                .onSuccess {
                    refreshPlaybackSurfaces()
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            errorMessage = error.message ?: "Transport action failed.",
                        )
                    }
                }
        }
    }

    private fun queueMutationAction(
        fallbackMessage: String,
        action: suspend (String, String) -> Any,
    ) {
        val baseUrl = uiState.value.baseUrl
        val rendererLocation = uiState.value.selectedRendererLocation
        if (baseUrl.isBlank() || rendererLocation.isBlank()) {
            _uiState.update { it.copy(errorMessage = "Choose a renderer first.") }
            return
        }
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null) }
            runCatching { action(baseUrl, rendererLocation) }
                .onSuccess { result ->
                    val message = extractMutationMessage(result) ?: fallbackMessage
                    _uiState.update { it.copy(infoMessage = message) }
                    refreshPlaybackSurfaces()
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            errorMessage = error.message ?: "Queue update failed.",
                        )
                    }
                }
        }
    }

    private fun refreshPlaybackSurfaces() {
        val baseUrl = uiState.value.baseUrl
        val rendererLocation = uiState.value.selectedRendererLocation
        if (baseUrl.isBlank() || rendererLocation.isBlank()) {
            _uiState.update { it.copy(isLoading = false) }
            return
        }
        viewModelScope.launch {
            runCatching {
                val nowPlayingDeferred = async { repository.getNowPlaying(baseUrl, rendererLocation) }
                val queueDeferred = async { repository.getQueue(baseUrl, rendererLocation) }
                nowPlayingDeferred.await() to queueDeferred.await()
            }.onSuccess { (nowPlaying, queue) ->
                _uiState.update {
                    it.copy(
                        nowPlaying = nowPlaying,
                        queue = queue,
                        isLoading = false,
                    )
                }
            }.onFailure { error ->
                _uiState.update {
                    it.copy(
                        isLoading = false,
                        errorMessage = error.message ?: "Failed to refresh playback state.",
                    )
                }
            }
        }
    }

    private fun chooseRenderer(renderers: List<RendererDto>): String? {
        val saved = repository.loadRendererLocation()
        return when {
            saved.isNotBlank() && renderers.any { it.location == saved } -> saved
            renderers.any { it.selected } -> renderers.first { it.selected }.location
            else -> renderers.firstOrNull()?.location
        }?.also(repository::saveRendererLocation)
    }

    private fun extractMutationMessage(result: Any): String? =
        when (result) {
            is io.musicd.android.data.MutationResponseDto -> result.message
            else -> null
        }
}

private data class Quintuple<A, B, C, D, E>(
    val first: A,
    val second: B,
    val third: C,
    val fourth: D,
    val fifth: E,
)
