package io.musicd.android.ui

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import io.musicd.android.data.AlbumDetailDto
import io.musicd.android.data.AlbumArtworkCandidateDto
import io.musicd.android.data.AlbumSummaryDto
import io.musicd.android.data.ArtistDetailDto
import io.musicd.android.data.ArtistSummaryDto
import io.musicd.android.data.MusicdApiException
import io.musicd.android.data.MusicdRepository
import io.musicd.android.data.MutationResponseDto
import io.musicd.android.data.NowPlayingDto
import io.musicd.android.data.PlaybackEventDto
import io.musicd.android.data.QueueDto
import io.musicd.android.data.RendererDto
import io.musicd.android.data.ServerInfoDto
import io.musicd.android.data.TrackSummaryDto
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

enum class MusicdTab {
    Home,
    Library,
    Queue,
}

enum class LibraryBrowseMode {
    Artists,
    Albums,
}

enum class LibrarySearchFacet {
    All,
    Artists,
    Albums,
    Tracks,
}

data class MusicdUiState(
    val serverInput: String = "",
    val baseUrl: String = "",
    val serverName: String? = null,
    val connected: Boolean = false,
    val showServerEditor: Boolean = false,
    val selectedTab: MusicdTab = MusicdTab.Home,
    val libraryBrowseMode: LibraryBrowseMode = LibraryBrowseMode.Artists,
    val librarySearchFacet: LibrarySearchFacet = LibrarySearchFacet.All,
    val searchQuery: String = "",
    val selectedRendererLocation: String = "",
    val renderers: List<RendererDto> = emptyList(),
    val nowPlaying: NowPlayingDto? = null,
    val artists: List<ArtistSummaryDto> = emptyList(),
    val albums: List<AlbumSummaryDto> = emptyList(),
    val tracks: List<TrackSummaryDto> = emptyList(),
    val selectedArtistDetail: ArtistDetailDto? = null,
    val selectedAlbumDetail: AlbumDetailDto? = null,
    val showAlbumArtworkPicker: Boolean = false,
    val albumArtworkCandidates: List<AlbumArtworkCandidateDto> = emptyList(),
    val isSearchingAlbumArtwork: Boolean = false,
    val isApplyingAlbumArtwork: Boolean = false,
    val albumArtworkErrorMessage: String? = null,
    val queue: QueueDto? = null,
    val showRendererPicker: Boolean = false,
    val isConnecting: Boolean = false,
    val isLoading: Boolean = false,
    val isDiscovering: Boolean = false,
    val errorMessage: String? = null,
    val warningMessage: String? = null,
    val infoMessage: String? = null,
)

class MusicdViewModel(application: Application) : AndroidViewModel(application) {
    private val repository = MusicdRepository(application)
    private var playbackEventsJob: Job? = null
    private var playbackEventsKey: String? = null
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
        _uiState.update {
            it.copy(
                searchQuery = value,
                librarySearchFacet = if (value.isBlank()) {
                    LibrarySearchFacet.All
                } else {
                    it.librarySearchFacet
                },
            )
        }
    }

    fun selectLibraryBrowseMode(mode: LibraryBrowseMode) {
        _uiState.update {
            it.copy(
                libraryBrowseMode = mode,
                selectedArtistDetail = null,
                selectedAlbumDetail = null,
            )
        }
    }

    fun selectLibrarySearchFacet(facet: LibrarySearchFacet) {
        _uiState.update { it.copy(librarySearchFacet = facet) }
    }

    fun toggleServerEditor(show: Boolean) {
        _uiState.update { it.copy(showServerEditor = show) }
    }

    fun connect(baseUrl: String = uiState.value.serverInput) {
        val normalized = baseUrl.trim().trimEnd('/')
        if (normalized.isBlank()) {
            _uiState.update { it.copy(errorMessage = "Enter the musicd server URL first.") }
            return
        }
        val wasConnected = uiState.value.connected
        _uiState.update {
            it.copy(
                serverInput = normalized,
                isConnecting = true,
                errorMessage = null,
                warningMessage = null,
                infoMessage = null,
            )
        }
        viewModelScope.launch {
            loadServerData(
                baseUrl = normalized,
                onSuccess = { serverInfo, renderers, artists, albums, tracks, nowPlaying, queue ->
                    repository.saveBaseUrl(normalized)
                    _uiState.update {
                        it.copy(
                            baseUrl = normalized,
                            serverName = serverInfo.name,
                            connected = true,
                            showServerEditor = false,
                            renderers = renderers,
                            artists = artists,
                            albums = albums,
                            tracks = tracks,
                            selectedRendererLocation = nowPlaying?.rendererLocation
                                ?: chooseRendererLocation(
                                    currentSelection = it.selectedRendererLocation,
                                    savedSelection = repository.loadRendererLocation(),
                                    renderers = renderers,
                                ).orEmpty(),
                            nowPlaying = nowPlaying,
                            queue = queue,
                            isConnecting = false,
                            isLoading = false,
                            errorMessage = null,
                            warningMessage = null,
                            infoMessage = "Connected to musicd.",
                        )
                    }
                },
                onFailure = { error ->
                    _uiState.update {
                        it.copy(
                            connected = wasConnected,
                            isConnecting = false,
                            isLoading = false,
                            errorMessage = connectionErrorMessage(error),
                        )
                    }
                },
            )
        }
    }

    fun refreshAll() {
        val baseUrl = uiState.value.baseUrl
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
            loadServerData(
                baseUrl = baseUrl,
                onSuccess = { serverInfo, renderers, artists, albums, tracks, nowPlaying, queue ->
                    _uiState.update {
                        val selectedRendererLocation = nowPlaying?.rendererLocation
                            ?: chooseRendererLocation(
                                currentSelection = it.selectedRendererLocation,
                                savedSelection = repository.loadRendererLocation(),
                                renderers = renderers,
                            ).orEmpty()
                        it.copy(
                            connected = true,
                            serverName = serverInfo.name,
                            renderers = renderers,
                            artists = artists,
                            albums = albums,
                            tracks = tracks,
                            selectedRendererLocation = selectedRendererLocation,
                            nowPlaying = nowPlaying,
                            queue = queue,
                            isLoading = false,
                            errorMessage = null,
                            warningMessage = null,
                            infoMessage = if (renderers.isEmpty()) {
                                "Connected. Discover a renderer to start playback."
                            } else {
                                it.infoMessage
                            },
                        )
                    }
                },
                onFailure = { error ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            errorMessage = connectionErrorMessage(error),
                        )
                    }
                },
            )
        }
    }

    fun retryConnection() {
        connect(uiState.value.serverInput.ifBlank { uiState.value.baseUrl })
    }

    private suspend fun loadServerData(
        baseUrl: String,
        onSuccess: (ServerInfoDto, List<RendererDto>, List<ArtistSummaryDto>, List<AlbumSummaryDto>, List<TrackSummaryDto>, NowPlayingDto?, QueueDto?) -> Unit,
        onFailure: (Throwable) -> Unit,
    ) {
        runCatching {
            val serverInfo = repository.getServerInfo(baseUrl)
            val renderers = repository.getRenderers(baseUrl)
            val artists = repository.getArtists(baseUrl)
            val albums = repository.getAlbums(baseUrl)
            val tracks = repository.getTracks(baseUrl)
            val rendererLocation = chooseRendererLocation(
                currentSelection = uiState.value.selectedRendererLocation,
                savedSelection = repository.loadRendererLocation(),
                renderers = renderers,
            )
            val nowPlaying = rendererLocation?.let { repository.getNowPlaying(baseUrl, it) }
            val queue = rendererLocation?.let { repository.getQueue(baseUrl, it) }
            Septuple(serverInfo, renderers, artists, albums, tracks, nowPlaying, queue)
        }.onSuccess { (serverInfo, renderers, artists, albums, tracks, nowPlaying, queue) ->
            onSuccess(serverInfo, renderers, artists, albums, tracks, nowPlaying, queue)
        }.onFailure(onFailure)
    }

    private fun connectionErrorMessage(error: Throwable): String {
        return when (error) {
            is MusicdApiException -> error.userMessage
            else -> error.message ?: "Could not connect to musicd."
        }
    }

    fun disconnectServer() {
        stopPlaybackEventSubscription()
        repository.clearBaseUrl()
        repository.clearRendererLocation()
        _uiState.update {
            it.copy(
                connected = false,
                baseUrl = "",
                serverName = null,
                serverInput = "",
                showServerEditor = false,
                selectedRendererLocation = "",
                renderers = emptyList(),
                nowPlaying = null,
                albums = emptyList(),
                tracks = emptyList(),
                artists = emptyList(),
                selectedArtistDetail = null,
                selectedAlbumDetail = null,
                queue = null,
                isConnecting = false,
                errorMessage = null,
                warningMessage = null,
                infoMessage = "Server disconnected.",
            )
        }
    }

    fun selectTab(tab: MusicdTab) {
        _uiState.update { it.copy(selectedTab = tab) }
    }

    fun updatePlaybackEventSubscription(enabled: Boolean) {
        val state = uiState.value
        val shouldRun = enabled &&
            state.connected &&
            !state.isConnecting &&
            state.baseUrl.isNotBlank() &&
            state.selectedRendererLocation.isNotBlank()
        if (!shouldRun) {
            stopPlaybackEventSubscription()
            return
        }

        val baseUrl = state.baseUrl
        val rendererLocation = state.selectedRendererLocation
        val desiredKey = "$baseUrl|$rendererLocation"
        if (playbackEventsKey == desiredKey && playbackEventsJob?.isActive == true) {
            return
        }

        playbackEventsJob?.cancel()
        playbackEventsKey = desiredKey
        playbackEventsJob = viewModelScope.launch {
            while (isActive) {
                try {
                    repository.observePlaybackEvents(baseUrl, rendererLocation) { event ->
                        applyPlaybackEvent(baseUrl, rendererLocation, event)
                    }
                    if (!isActive || playbackEventsKey != desiredKey) {
                        break
                    }
                    delay(1_000)
                } catch (error: Throwable) {
                    if (!isActive || playbackEventsKey != desiredKey) {
                        break
                    }
                    _uiState.update {
                        if (it.baseUrl == baseUrl && it.selectedRendererLocation == rendererLocation) {
                            it.copy(warningMessage = connectionErrorMessage(error))
                        } else {
                            it
                        }
                    }
                    delay(3_000)
                }
            }
        }
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

    fun openAlbum(albumId: String, preserveArtistContext: Boolean = false) {
        val baseUrl = uiState.value.baseUrl
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
            runCatching { repository.getAlbumDetail(baseUrl, albumId) }
                .onSuccess { album ->
                    _uiState.update {
                        val artistContext = if (preserveArtistContext) it.selectedArtistDetail else null
                        it.copy(
                            selectedTab = MusicdTab.Library,
                            selectedArtistDetail = artistContext,
                            selectedAlbumDetail = album,
                            isLoading = false,
                            warningMessage = null,
                        )
                    }
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            errorMessage = connectionErrorMessage(error),
                        )
                    }
                }
        }
    }

    fun closeAlbumDetail() {
        _uiState.update { it.copy(selectedAlbumDetail = null) }
    }

    fun openAlbumArtworkPicker() {
        val baseUrl = uiState.value.baseUrl
        val album = uiState.value.selectedAlbumDetail ?: return
        if (baseUrl.isBlank()) return

        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    showAlbumArtworkPicker = true,
                    albumArtworkCandidates = emptyList(),
                    isSearchingAlbumArtwork = true,
                    isApplyingAlbumArtwork = false,
                    albumArtworkErrorMessage = null,
                )
            }

            runCatching { repository.getAlbumArtworkCandidates(baseUrl, album.id) }
                .onSuccess { response ->
                    _uiState.update {
                        it.copy(
                            showAlbumArtworkPicker = true,
                            albumArtworkCandidates = response.candidates,
                            isSearchingAlbumArtwork = false,
                            albumArtworkErrorMessage = response.error,
                        )
                    }
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isSearchingAlbumArtwork = false,
                            albumArtworkErrorMessage = connectionErrorMessage(error),
                        )
                    }
                }
        }
    }

    fun dismissAlbumArtworkPicker() {
        _uiState.update {
            it.copy(
                showAlbumArtworkPicker = false,
                albumArtworkCandidates = emptyList(),
                isSearchingAlbumArtwork = false,
                isApplyingAlbumArtwork = false,
                albumArtworkErrorMessage = null,
            )
        }
    }

    fun applyAlbumArtwork(releaseId: String) {
        val baseUrl = uiState.value.baseUrl
        val album = uiState.value.selectedAlbumDetail ?: return
        if (baseUrl.isBlank()) return

        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    isApplyingAlbumArtwork = true,
                    albumArtworkErrorMessage = null,
                    errorMessage = null,
                    warningMessage = null,
                )
            }

            runCatching {
                repository.selectAlbumArtwork(baseUrl, album.id, releaseId)
                val albums = repository.getAlbums(baseUrl)
                val artists = repository.getArtists(baseUrl)
                val refreshedAlbum = repository.getAlbumDetail(baseUrl, album.id)
                val refreshedArtist = uiState.value.selectedArtistDetail?.let { selectedArtist ->
                    repository.getArtistDetail(baseUrl, selectedArtist.id)
                }
                Quadruple(albums, artists, refreshedAlbum, refreshedArtist)
            }.onSuccess { (albums, artists, refreshedAlbum, refreshedArtist) ->
                _uiState.update {
                    it.copy(
                        albums = albums,
                        artists = artists,
                        selectedAlbumDetail = refreshedAlbum,
                        selectedArtistDetail = refreshedArtist ?: it.selectedArtistDetail,
                        showAlbumArtworkPicker = false,
                        albumArtworkCandidates = emptyList(),
                        isSearchingAlbumArtwork = false,
                        isApplyingAlbumArtwork = false,
                        albumArtworkErrorMessage = null,
                        infoMessage = "Album artwork updated.",
                    )
                }
            }.onFailure { error ->
                _uiState.update {
                    it.copy(
                        isApplyingAlbumArtwork = false,
                        albumArtworkErrorMessage = connectionErrorMessage(error),
                    )
                }
            }
        }
    }

    fun openArtist(artistId: String) {
        val baseUrl = uiState.value.baseUrl
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
            runCatching { repository.getArtistDetail(baseUrl, artistId) }
                .onSuccess { artist ->
                    _uiState.update {
                        it.copy(
                            selectedTab = MusicdTab.Library,
                            selectedArtistDetail = artist,
                            selectedAlbumDetail = null,
                            isLoading = false,
                            warningMessage = null,
                        )
                    }
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            errorMessage = connectionErrorMessage(error),
                        )
                    }
                }
        }
    }

    fun closeArtistDetail() {
        _uiState.update { it.copy(selectedArtistDetail = null) }
    }

    fun openArtistByName(name: String) {
        val artistId = uiState.value.artists.firstOrNull {
            normalizeLibraryName(it.name) == normalizeLibraryName(name)
        }?.id

        if (artistId == null) {
            _uiState.update {
                it.copy(errorMessage = "Could not find an artist entry for \"$name\".")
            }
            return
        }

        openArtist(artistId)
    }

    fun discoverRenderers() {
        val baseUrl = uiState.value.baseUrl
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            _uiState.update { it.copy(isDiscovering = true, errorMessage = null, warningMessage = null) }
            runCatching { repository.discoverRenderers(baseUrl) }
                .onSuccess { discovered ->
                    val selected = chooseRendererLocation(
                        currentSelection = uiState.value.selectedRendererLocation,
                        savedSelection = repository.loadRendererLocation(),
                        renderers = discovered,
                    )
                    _uiState.update {
                        it.copy(
                            renderers = discovered,
                            selectedRendererLocation = selected.orEmpty(),
                            isDiscovering = false,
                            warningMessage = null,
                            infoMessage = if (discovered.isEmpty()) {
                                "No renderers found on the network."
                            } else {
                                "Renderer discovery refreshed."
                            },
                        )
                    }
                    selected?.let { refreshPlaybackSurfaces() }
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isDiscovering = false,
                            errorMessage = connectionErrorMessage(error),
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

    fun transportNext() {
        if (!canRequestPlaybackNavigation(uiState.value)) {
            return
        }
        transportAction { baseUrl, renderer ->
            repository.transportNext(baseUrl, renderer)
        }
    }

    fun playTrack(trackId: String) {
        val baseUrl = uiState.value.baseUrl
        val rendererLocation = uiState.value.selectedRendererLocation
        if (baseUrl.isBlank() || rendererLocation.isBlank()) {
            _uiState.update { it.copy(errorMessage = "Choose a renderer first.") }
            return
        }
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
            runCatching { repository.playTrack(baseUrl, rendererLocation, trackId) }
                .onSuccess { response ->
                    _uiState.update { it.copy(infoMessage = response.message) }
                    refreshPlaybackSurfaces()
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            errorMessage = connectionErrorMessage(error),
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
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
            runCatching { repository.playAlbum(baseUrl, rendererLocation, albumId) }
                .onSuccess { response ->
                    _uiState.update { it.copy(infoMessage = response.message) }
                    refreshPlaybackSurfaces()
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            errorMessage = connectionErrorMessage(error),
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

    fun refreshPlaybackState() {
        refreshPlaybackSurfaces()
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
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
            runCatching { action(baseUrl, rendererLocation) }
                .onSuccess {
                    refreshPlaybackSurfaces()
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLoading = false,
                            errorMessage = connectionErrorMessage(error),
                        )
                    }
                }
        }
    }

    fun transportPrevious() {
        if (!canRequestPlaybackNavigation(uiState.value)) {
            return
        }
        transportAction { baseUrl, renderer ->
            repository.transportPrevious(baseUrl, renderer)
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
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
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
                            errorMessage = connectionErrorMessage(error),
                        )
                    }
                }
        }
    }

    private fun refreshPlaybackSurfaces() {
        val baseUrl = uiState.value.baseUrl
        val rendererLocation = uiState.value.selectedRendererLocation
        if (baseUrl.isBlank() || rendererLocation.isBlank() || uiState.value.isConnecting) {
            _uiState.update { it.copy(isLoading = false, warningMessage = null) }
            return
        }
        viewModelScope.launch {
            runCatching {
                val nowPlaying = repository.getNowPlaying(baseUrl, rendererLocation)
                val queue = repository.getQueue(baseUrl, rendererLocation)
                nowPlaying to queue
            }.onSuccess { (nowPlaying, queue) ->
                _uiState.update {
                    it.copy(
                        nowPlaying = nowPlaying,
                        queue = queue,
                        isLoading = false,
                        errorMessage = null,
                        warningMessage = null,
                    )
                }
            }.onFailure { error ->
                _uiState.update {
                    it.copy(
                        isLoading = false,
                        warningMessage = connectionErrorMessage(error),
                    )
                }
            }
        }
    }

    private fun chooseRendererLocation(
        currentSelection: String,
        savedSelection: String,
        renderers: List<RendererDto>,
    ): String? {
        return when {
            currentSelection.isNotBlank() && renderers.any { it.location == currentSelection } -> currentSelection
            savedSelection.isNotBlank() && renderers.any { it.location == savedSelection } -> savedSelection
            renderers.any { it.selected } -> renderers.first { it.selected }.location
            else -> renderers.firstOrNull()?.location
        }?.also(repository::saveRendererLocation)
    }

    private fun extractMutationMessage(result: Any): String? =
        when (result) {
            is MutationResponseDto -> result.message
            else -> null
        }

    fun dismissError() {
        _uiState.update { it.copy(errorMessage = null) }
    }

    fun dismissWarning() {
        _uiState.update { it.copy(warningMessage = null) }
    }

    private fun stopPlaybackEventSubscription() {
        playbackEventsJob?.cancel()
        playbackEventsJob = null
        playbackEventsKey = null
    }

    private fun applyPlaybackEvent(
        baseUrl: String,
        rendererLocation: String,
        event: PlaybackEventDto,
    ) {
        _uiState.update {
            if (it.baseUrl != baseUrl || it.selectedRendererLocation != rendererLocation) {
                it
            } else {
                it.copy(
                    nowPlaying = event.nowPlaying,
                    queue = event.queue,
                    warningMessage = null,
                )
            }
        }
    }
}

private fun normalizeLibraryName(value: String): String =
    value.trim().lowercase()

private fun canRequestPlaybackNavigation(state: MusicdUiState): Boolean =
    state.queue?.entries?.isNotEmpty() == true ||
        state.nowPlaying?.currentTrack != null ||
        state.nowPlaying?.session?.queueEntryId != null

private data class Quintuple<A, B, C, D, E>(
    val first: A,
    val second: B,
    val third: C,
    val fourth: D,
    val fifth: E,
)

private data class Quadruple<A, B, C, D>(
    val first: A,
    val second: B,
    val third: C,
    val fourth: D,
)

private data class Sextuple<A, B, C, D, E, F>(
    val first: A,
    val second: B,
    val third: C,
    val fourth: D,
    val fifth: E,
    val sixth: F,
)

private data class Septuple<A, B, C, D, E, F, G>(
    val first: A,
    val second: B,
    val third: C,
    val fourth: D,
    val fifth: E,
    val sixth: F,
    val seventh: G,
)
