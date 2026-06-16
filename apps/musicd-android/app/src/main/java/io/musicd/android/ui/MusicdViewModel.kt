package io.musicd.android.ui

import android.app.Application
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import io.musicd.android.data.AlbumDetailDto
import io.musicd.android.data.AlbumArtworkCandidateDto
import io.musicd.android.data.AlbumRecommendationDto
import io.musicd.android.data.AlbumSummaryDto
import io.musicd.android.data.ArtistDetailDto
import io.musicd.android.data.ArtistSummaryDto
import io.musicd.android.data.DiscoveredServer
import io.musicd.android.data.LastfmApiException
import io.musicd.android.data.LastfmRepository
import io.musicd.android.data.LastfmSettings
import io.musicd.android.data.LikeResponseDto
import io.musicd.android.data.LocalCompanionRepository
import io.musicd.android.data.MusicdApiException
import io.musicd.android.data.MusicdRepository
import io.musicd.android.data.MusicSourceKind
import io.musicd.android.data.MutationResponseDto
import io.musicd.android.data.NowPlayingDto
import io.musicd.android.data.PlaybackEventDto
import io.musicd.android.data.QueueDto
import io.musicd.android.data.RadioStationDto
import io.musicd.android.data.RendererDto
import io.musicd.android.data.ServerInfoDto
import io.musicd.android.data.TidalAlbumDto
import io.musicd.android.data.TidalTrackDto
import io.musicd.android.data.TrackSummaryDto
import io.musicd.android.playback.LastfmScrobbler
import io.musicd.android.playback.MusicdPlaybackNotificationService
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
    Radio,
    Tidal,
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
    val sourceKind: MusicSourceKind = MusicSourceKind.RemoteServer,
    val serverInput: String = "",
    val baseUrl: String = "",
    val serverName: String? = null,
    val connected: Boolean = false,
    val showServerEditor: Boolean = false,
    val selectedTab: MusicdTab = MusicdTab.Home,
    val libraryBrowseMode: LibraryBrowseMode = LibraryBrowseMode.Artists,
    val librarySearchFacet: LibrarySearchFacet = LibrarySearchFacet.All,
    val searchQuery: String = "",
    val radioQuery: String = "",
    val radioCountryCode: String = "",
    val radioStations: List<RadioStationDto> = emptyList(),
    val isSearchingRadio: Boolean = false,
    val hasSearchedRadio: Boolean = false,
    val tidalQuery: String = "",
    val tidalAlbums: List<TidalAlbumDto> = emptyList(),
    val tidalTracks: List<TidalTrackDto> = emptyList(),
    val isSearchingTidal: Boolean = false,
    val hasSearchedTidal: Boolean = false,
    val lastfmApiKey: String = "",
    val lastfmSharedSecret: String = "",
    val lastfmUsername: String = "",
    val lastfmPendingToken: String = "",
    val isLastfmBusy: Boolean = false,
    val selectedRendererLocation: String = "",
    val renderers: List<RendererDto> = emptyList(),
    val nowPlaying: NowPlayingDto? = null,
    val artists: List<ArtistSummaryDto> = emptyList(),
    val albums: List<AlbumSummaryDto> = emptyList(),
    val suppressedSpotlightAlbumIds: Set<String> = emptySet(),
    val homeRecommendations: List<AlbumRecommendationDto> = emptyList(),
    val tracks: List<TrackSummaryDto> = emptyList(),
    val selectedArtistDetail: ArtistDetailDto? = null,
    val selectedAlbumDetail: AlbumDetailDto? = null,
    val selectedAlbumRecommendations: List<AlbumRecommendationDto> = emptyList(),
    val showAlbumArtworkPicker: Boolean = false,
    val albumArtworkCandidates: List<AlbumArtworkCandidateDto> = emptyList(),
    val isSearchingAlbumArtwork: Boolean = false,
    val isApplyingAlbumArtwork: Boolean = false,
    val albumArtworkErrorMessage: String? = null,
    val queue: QueueDto? = null,
    val showRendererPicker: Boolean = false,
    val isCreatingRendererGroup: Boolean = false,
    val rendererGroupErrorMessage: String? = null,
    val isConnecting: Boolean = false,
    val isLoading: Boolean = false,
    val isDiscovering: Boolean = false,
    val isDiscoveringServers: Boolean = false,
    val discoveredServers: List<DiscoveredServer> = emptyList(),
    val hasRunServerDiscovery: Boolean = false,
    val errorMessage: String? = null,
    val warningMessage: String? = null,
    val infoMessage: String? = null,
)

class MusicdViewModel(application: Application) : AndroidViewModel(application) {
    private val repository = MusicdRepository(application)
    private val lastfmRepository = LastfmRepository(application)
    private val localCompanionLastfmScrobbler = LastfmScrobbler(lastfmRepository, viewModelScope)
    private val localCompanionRepository = LocalCompanionRepository(application)
    private var playbackEventsJob: Job? = null
    private var playbackEventsKey: String? = null
    private var tracksLoadJob: Job? = null
    private var playbackEventFailureCount: Int = 0
    private val _uiState = MutableStateFlow(
        MusicdUiState(serverInput = repository.loadBaseUrl()).withLastfmSettings(lastfmRepository.loadSettings()),
    )
    val uiState: StateFlow<MusicdUiState> = _uiState.asStateFlow()

    init {
        val savedBaseUrl = repository.loadBaseUrl()
        if (repository.loadSourceKind() == MusicSourceKind.LocalCompanion) {
            connectLocalCompanion(auto = true)
        } else if (savedBaseUrl.isNotBlank()) {
            connect(savedBaseUrl)
        } else {
            discoverServers(auto = true)
        }
    }

    fun connectLocalCompanion(auto: Boolean = false) {
        stopPlaybackEventSubscription()
        MusicdPlaybackNotificationService.stop(getApplication())
        repository.saveSourceKind(MusicSourceKind.LocalCompanion)
        repository.clearBaseUrl()
        repository.saveRendererLocation(LocalCompanionRepository.LOCAL_COMPANION_RENDERER)
        val companionInstalled = localCompanionRepository.isInstalled()
        val paired = companionInstalled && localCompanionRepository.pairController(repository.loadClientId())
        _uiState.update {
            it.copy(
                sourceKind = MusicSourceKind.LocalCompanion,
                serverInput = "",
                baseUrl = LocalCompanionRepository.LOCAL_COMPANION_BASE_URL,
                serverName = "musicd Companion",
                connected = true,
                showServerEditor = false,
                selectedTab = MusicdTab.Library,
                selectedRendererLocation = LocalCompanionRepository.LOCAL_COMPANION_RENDERER,
                renderers = listOf(localCompanionRenderer()),
                nowPlaying = null,
                artists = emptyList(),
                albums = emptyList(),
                homeRecommendations = emptyList(),
                tracks = emptyList(),
                selectedArtistDetail = null,
                selectedAlbumDetail = null,
                selectedAlbumRecommendations = emptyList(),
                queue = null,
                isConnecting = false,
                isLoading = false,
                isDiscovering = false,
                isDiscoveringServers = false,
                discoveredServers = emptyList(),
                errorMessage = null,
                warningMessage = if (!companionInstalled) {
                    "Install or run the musicd Companion app to add local music folders."
                } else if (!paired) {
                    "Open or reinstall the musicd Companion app to finish local pairing."
                } else {
                    null
                },
                infoMessage = if (auto) {
                    "Using local companion mode."
                } else {
                    "Local companion mode is ready."
                },
            )
        }
        refreshLocalCompanionLibrary(openCompanionOnFailure = !auto)
    }

    fun openLocalCompanion() {
        val launched = localCompanionRepository.launchCompanion()
        _uiState.update {
            it.copy(
                errorMessage = if (launched) null else "musicd Companion is not installed on this device.",
                infoMessage = if (launched) {
                    "Opened musicd Companion. Return here after scanning to refresh the local library."
                } else {
                    it.infoMessage
                },
            )
        }
    }

    fun onAppForegrounded() {
        val state = uiState.value
        if (state.sourceKind == MusicSourceKind.LocalCompanion && state.connected && !state.isLoading) {
            refreshLocalCompanionLibrary(openCompanionOnFailure = false)
        }
    }

    fun discoverServers(auto: Boolean = false) {
        if (auto && uiState.value.sourceKind == MusicSourceKind.LocalCompanion) return
        if (uiState.value.isDiscoveringServers) return
        if (auto && uiState.value.hasRunServerDiscovery) return
        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    isDiscoveringServers = true,
                    hasRunServerDiscovery = true,
                    errorMessage = if (auto) it.errorMessage else null,
                )
            }
            val results = runCatching { repository.discoverServers() }
                .getOrDefault(emptyList())
            _uiState.update { state ->
                state.copy(
                    isDiscoveringServers = false,
                    discoveredServers = results,
                    infoMessage = if (auto || state.connected) {
                        state.infoMessage
                    } else if (results.isEmpty()) {
                        "No musicd servers found on this network."
                    } else {
                        state.infoMessage
                    },
                )
            }
        }
    }

    fun selectDiscoveredServer(baseUrl: String) {
        if (baseUrl.isBlank()) return
        _uiState.update { it.copy(serverInput = baseUrl) }
        connect(baseUrl)
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
        if (value.isNotBlank()) {
            ensureTracksLoaded()
        }
    }

    fun selectLibraryBrowseMode(mode: LibraryBrowseMode) {
        _uiState.update {
            it.copy(
                libraryBrowseMode = mode,
                selectedArtistDetail = null,
                selectedAlbumDetail = null,
                selectedAlbumRecommendations = emptyList(),
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
                onSuccess = { serverInfo, renderers, artists, albums, homeRecommendations, nowPlaying, queue ->
                    repository.saveBaseUrl(normalized)
                    repository.saveSourceKind(MusicSourceKind.RemoteServer)
                    _uiState.update {
                        it.copy(
                            sourceKind = MusicSourceKind.RemoteServer,
                            baseUrl = normalized,
                            serverName = serverInfo.name,
                            connected = true,
                            showServerEditor = false,
                            renderers = renderers,
                            artists = artists,
                            albums = albums,
                            homeRecommendations = homeRecommendations,
                            tracks = emptyList(),
                            suppressedSpotlightAlbumIds = emptySet(),
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
                    if (uiState.value.selectedTab == MusicdTab.Library || uiState.value.searchQuery.isNotBlank()) {
                        ensureTracksLoaded()
                    }
                    syncPlaybackNotificationService()
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
        if (uiState.value.sourceKind == MusicSourceKind.LocalCompanion) {
            refreshLocalCompanionLibrary(openCompanionOnFailure = false)
            return
        }
        val baseUrl = uiState.value.baseUrl
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
            loadServerData(
                baseUrl = baseUrl,
                onSuccess = { serverInfo, renderers, artists, albums, homeRecommendations, nowPlaying, queue ->
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
                            homeRecommendations = homeRecommendations,
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
                    if (uiState.value.selectedTab == MusicdTab.Library || uiState.value.searchQuery.isNotBlank()) {
                        ensureTracksLoaded(force = true)
                    }
                    syncPlaybackNotificationService()
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
        onSuccess: (
            ServerInfoDto,
            List<RendererDto>,
            List<ArtistSummaryDto>,
            List<AlbumSummaryDto>,
            List<AlbumRecommendationDto>,
            NowPlayingDto?,
            QueueDto?,
        ) -> Unit,
        onFailure: (Throwable) -> Unit,
    ) {
        runCatching {
            val serverInfo = repository.getServerInfo(baseUrl)
            runCatching {
                repository.registerAndroidLocalRenderer(
                    baseUrl = baseUrl,
                    rendererLocation = androidLocalRendererLocation(),
                    name = "This phone",
                    manufacturer = Build.MANUFACTURER,
                    modelName = Build.MODEL,
                )
            }
            val renderers = repository.getRenderers(baseUrl)
            val artists = repository.getArtists(baseUrl)
            val albums = repository.getAlbums(baseUrl)
            val homeRecommendations = runCatching {
                repository.getCollectionRecommendations(baseUrl, HOME_RECOMMENDATION_LIMIT).recommendations
            }.getOrDefault(emptyList())
            val rendererLocation = chooseRendererLocation(
                currentSelection = uiState.value.selectedRendererLocation,
                savedSelection = repository.loadRendererLocation(),
                renderers = renderers,
            )
            val nowPlaying = rendererLocation?.let { repository.getNowPlaying(baseUrl, it) }
            val queue = rendererLocation?.let { repository.getQueue(baseUrl, it) }
            Septuple(serverInfo, renderers, artists, albums, homeRecommendations, nowPlaying, queue)
        }.onSuccess { (serverInfo, renderers, artists, albums, homeRecommendations, nowPlaying, queue) ->
            onSuccess(serverInfo, renderers, artists, albums, homeRecommendations, nowPlaying, queue)
        }.onFailure(onFailure)
    }

    private fun refreshLocalCompanionLibrary(openCompanionOnFailure: Boolean) {
        val baseUrl = LocalCompanionRepository.LOCAL_COMPANION_BASE_URL
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
            runCatching {
                loadLocalCompanionData(baseUrl)
            }.recoverCatching {
                prepareLocalCompanionControlSurface(waitForStartup = true)
                loadLocalCompanionData(baseUrl)
            }.onSuccess { (serverInfo, renderers, artists, albums, tracks, nowPlaying, queue) ->
                _uiState.update {
                    it.copy(
                        sourceKind = MusicSourceKind.LocalCompanion,
                        baseUrl = baseUrl,
                        serverName = serverInfo.name,
                        connected = true,
                        selectedRendererLocation = LocalCompanionRepository.LOCAL_COMPANION_RENDERER,
                        renderers = renderers.ifEmpty { listOf(localCompanionRenderer()) },
                        artists = artists,
                        albums = albums,
                        tracks = tracks,
                        nowPlaying = nowPlaying,
                        queue = queue,
                        isLoading = false,
                        warningMessage = if (albums.isEmpty() && tracks.isEmpty()) {
                            "No local tracks indexed yet. Open musicd Companion to add folders and scan."
                        } else {
                            null
                        },
                        infoMessage = if (tracks.isEmpty()) it.infoMessage else "Loaded ${tracks.size} local tracks.",
                    )
                }
            }.onFailure { error ->
                if (openCompanionOnFailure) {
                    localCompanionRepository.launchCompanion()
                }
                _uiState.update {
                    it.copy(
                        renderers = listOf(localCompanionRenderer()),
                        isLoading = false,
                        warningMessage = "Open musicd Companion, add folders, then scan. ${connectionErrorMessage(error)}",
                    )
                }
            }
        }
    }

    private suspend fun loadLocalCompanionData(
        baseUrl: String,
    ): Septuple<
        ServerInfoDto,
        List<RendererDto>,
        List<ArtistSummaryDto>,
        List<AlbumSummaryDto>,
        List<TrackSummaryDto>,
        NowPlayingDto,
        QueueDto,
    > {
        prepareLocalCompanionControlSurface(waitForStartup = false)
        val serverInfo = repository.getServerInfo(baseUrl)
        val renderers = repository.getRenderers(baseUrl)
        val artists = repository.getArtists(baseUrl)
        val albums = repository.getAlbums(baseUrl)
        val tracks = repository.getTracks(baseUrl)
        val nowPlaying = repository.getNowPlaying(baseUrl, LocalCompanionRepository.LOCAL_COMPANION_RENDERER)
        val queue = repository.getQueue(baseUrl, LocalCompanionRepository.LOCAL_COMPANION_RENDERER)
        return Septuple(serverInfo, renderers, artists, albums, tracks, nowPlaying, queue)
    }

    private suspend fun prepareLocalCompanionControlSurface(waitForStartup: Boolean): Boolean {
        val prepared = localCompanionRepository.pairController(repository.loadClientId())
        if (prepared && waitForStartup) {
            delay(350)
        }
        return prepared
    }

    private fun connectionErrorMessage(error: Throwable): String {
        return when (error) {
            is MusicdApiException -> error.userMessage
            else -> error.message ?: "Could not connect to musicd."
        }
    }

    private fun lastfmErrorMessage(error: Throwable): String =
        when (error) {
            is LastfmApiException -> error.userMessage
            else -> error.message ?: "Could not connect to Last.fm."
        }

    private fun openExternalUrl(url: String) {
        val intent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        runCatching { getApplication<Application>().startActivity(intent) }
            .onFailure {
                _uiState.update { state ->
                    state.copy(warningMessage = "Open this Last.fm sign-in URL in a browser: $url")
                }
            }
    }

    private fun androidLocalRendererLocation(): String {
        val androidId = Settings.Secure.getString(
            getApplication<Application>().contentResolver,
            Settings.Secure.ANDROID_ID,
        ).orEmpty().ifBlank { "this-device" }
        return "android-local://$androidId"
    }

    fun disconnectServer() {
        stopPlaybackEventSubscription()
        MusicdPlaybackNotificationService.stop(getApplication())
        repository.saveSourceKind(MusicSourceKind.RemoteServer)
        repository.clearBaseUrl()
        repository.clearRendererLocation()
        _uiState.update {
            it.copy(
                connected = false,
                sourceKind = MusicSourceKind.RemoteServer,
                baseUrl = "",
                serverName = null,
                serverInput = "",
                showServerEditor = false,
                selectedRendererLocation = "",
                renderers = emptyList(),
                nowPlaying = null,
                albums = emptyList(),
                suppressedSpotlightAlbumIds = emptySet(),
                homeRecommendations = emptyList(),
                tracks = emptyList(),
                radioStations = emptyList(),
                radioQuery = "",
                radioCountryCode = "",
                isSearchingRadio = false,
                hasSearchedRadio = false,
                tidalQuery = "",
                tidalAlbums = emptyList(),
                tidalTracks = emptyList(),
                isSearchingTidal = false,
                hasSearchedTidal = false,
                artists = emptyList(),
                selectedArtistDetail = null,
                selectedAlbumDetail = null,
                selectedAlbumRecommendations = emptyList(),
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
        if (tab == MusicdTab.Library) {
            ensureTracksLoaded()
        }
    }

    fun updateRadioQuery(value: String) {
        _uiState.update { it.copy(radioQuery = value) }
    }

    fun updateRadioCountryCode(value: String) {
        _uiState.update { it.copy(radioCountryCode = value.take(2).uppercase()) }
    }

    fun updateTidalQuery(value: String) {
        _uiState.update { it.copy(tidalQuery = value) }
    }

    fun updateLastfmApiKey(value: String) {
        _uiState.update { it.copy(lastfmApiKey = value) }
        lastfmRepository.saveAppCredentials(value, uiState.value.lastfmSharedSecret)
    }

    fun updateLastfmSharedSecret(value: String) {
        _uiState.update { it.copy(lastfmSharedSecret = value) }
        lastfmRepository.saveAppCredentials(uiState.value.lastfmApiKey, value)
    }

    fun beginLastfmAuthentication() {
        if (uiState.value.isLastfmBusy) return
        lastfmRepository.saveAppCredentials(uiState.value.lastfmApiKey, uiState.value.lastfmSharedSecret)
        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    isLastfmBusy = true,
                    errorMessage = null,
                    warningMessage = null,
                    infoMessage = null,
                )
            }
            runCatching { lastfmRepository.beginAuthentication() }
                .onSuccess { token ->
                    _uiState.update {
                        it.withLastfmSettings(lastfmRepository.loadSettings()).copy(
                            isLastfmBusy = false,
                            infoMessage = "Authorize musicd in Last.fm, then return here to complete sign-in.",
                        )
                    }
                    openExternalUrl(LastfmRepository.authUrl(uiState.value.lastfmApiKey, token))
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLastfmBusy = false,
                            errorMessage = lastfmErrorMessage(error),
                        )
                    }
                }
        }
    }

    fun completeLastfmAuthentication() {
        if (uiState.value.isLastfmBusy) return
        lastfmRepository.saveAppCredentials(uiState.value.lastfmApiKey, uiState.value.lastfmSharedSecret)
        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    isLastfmBusy = true,
                    errorMessage = null,
                    warningMessage = null,
                    infoMessage = null,
                )
            }
            runCatching { lastfmRepository.completeAuthentication() }
                .onSuccess { session ->
                    _uiState.update {
                        it.withLastfmSettings(lastfmRepository.loadSettings()).copy(
                            isLastfmBusy = false,
                            infoMessage = "Last.fm scrobbling enabled for ${session.name}.",
                        )
                    }
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(
                            isLastfmBusy = false,
                            errorMessage = lastfmErrorMessage(error),
                        )
                    }
                }
        }
    }

    fun disconnectLastfm() {
        lastfmRepository.disconnect()
        _uiState.update {
            it.withLastfmSettings(lastfmRepository.loadSettings()).copy(
                infoMessage = "Last.fm scrobbling disconnected.",
            )
        }
    }

    fun searchRadioStations() {
        val state = uiState.value
        if (state.sourceKind == MusicSourceKind.LocalCompanion) {
            _uiState.update { it.copy(infoMessage = "Internet radio is not part of local companion mode yet.") }
            return
        }
        val baseUrl = state.baseUrl
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    isSearchingRadio = true,
                    hasSearchedRadio = true,
                    errorMessage = null,
                    warningMessage = null,
                    infoMessage = null,
                )
            }
            runCatching {
                repository.searchRadioStations(
                    baseUrl = baseUrl,
                    query = uiState.value.radioQuery.trim(),
                    countryCode = uiState.value.radioCountryCode.trim(),
                )
            }.onSuccess { stations ->
                _uiState.update {
                    it.copy(
                        radioStations = stations,
                        isSearchingRadio = false,
                        warningMessage = null,
                        infoMessage = if (stations.isEmpty()) {
                            "No radio stations matched that search."
                        } else {
                            null
                        },
                    )
                }
            }.onFailure { error ->
                _uiState.update {
                    it.copy(
                        isSearchingRadio = false,
                        errorMessage = connectionErrorMessage(error),
                    )
                }
            }
        }
    }

    fun searchTidalTracks() {
        val state = uiState.value
        if (state.sourceKind == MusicSourceKind.LocalCompanion) {
            _uiState.update { it.copy(infoMessage = "TIDAL is not part of local companion mode yet.") }
            return
        }
        val baseUrl = state.baseUrl
        val query = state.tidalQuery.trim()
        if (baseUrl.isBlank()) return
        if (query.isBlank()) {
            _uiState.update { it.copy(errorMessage = "Enter a TIDAL search first.") }
            return
        }
        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    isSearchingTidal = true,
                    hasSearchedTidal = true,
                    errorMessage = null,
                    warningMessage = null,
                    infoMessage = null,
                )
            }
            runCatching {
                val albums = repository.searchTidalAlbums(
                    baseUrl = baseUrl,
                    query = query,
                )
                val tracks = repository.searchTidalTracks(
                    baseUrl = baseUrl,
                    query = query,
                )
                albums to tracks
            }.onSuccess { (albums, tracks) ->
                _uiState.update {
                    it.copy(
                        tidalAlbums = albums,
                        tidalTracks = tracks,
                        isSearchingTidal = false,
                        warningMessage = null,
                        infoMessage = if (albums.isEmpty() && tracks.isEmpty()) {
                            "No TIDAL albums or tracks matched that search."
                        } else {
                            null
                        },
                    )
                }
            }.onFailure { error ->
                _uiState.update {
                    it.copy(
                        isSearchingTidal = false,
                        errorMessage = connectionErrorMessage(error),
                    )
                }
            }
        }
    }

    private fun ensureTracksLoaded(force: Boolean = false) {
        val state = uiState.value
        if (state.sourceKind == MusicSourceKind.LocalCompanion) return
        val baseUrl = state.baseUrl
        if (!state.connected || baseUrl.isBlank()) return
        if (!force && state.tracks.isNotEmpty()) return
        if (tracksLoadJob?.isActive == true) return

        tracksLoadJob = viewModelScope.launch {
            runCatching { repository.getTracks(baseUrl) }
                .onSuccess { tracks ->
                    _uiState.update {
                        if (it.connected && it.baseUrl == baseUrl) {
                            it.copy(
                                tracks = tracks,
                                warningMessage = if (it.warningMessage == TRACKS_WARNING_MESSAGE) {
                                    null
                                } else {
                                    it.warningMessage
                                },
                            )
                        } else {
                            it
                        }
                    }
                }
                .onFailure {
                    _uiState.update {
                        if (it.connected && it.baseUrl == baseUrl) {
                            it.copy(warningMessage = TRACKS_WARNING_MESSAGE)
                        } else {
                            it
                        }
                    }
                }
        }
    }

    fun updatePlaybackEventSubscription(enabled: Boolean) {
        val state = uiState.value
        if (state.sourceKind == MusicSourceKind.LocalCompanion) {
            if (enabled) {
                startLocalCompanionPlaybackPolling()
            } else {
                stopPlaybackEventSubscription()
            }
            return
        }
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
        playbackEventFailureCount = 0
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
                    playbackEventFailureCount += 1
                    val shouldSurfaceWarning = playbackEventFailureCount >= PLAYBACK_EVENT_WARNING_THRESHOLD
                    _uiState.update {
                        if (it.baseUrl == baseUrl && it.selectedRendererLocation == rendererLocation) {
                            it.copy(
                                warningMessage = when {
                                    !shouldSurfaceWarning -> it.warningMessage
                                    it.nowPlaying != null || it.queue != null -> PLAYBACK_EVENT_RECONNECTING_MESSAGE
                                    else -> connectionErrorMessage(error)
                                },
                            )
                        } else {
                            it
                        }
                    }
                    delay(3_000)
                }
            }
        }
    }

    private fun startLocalCompanionPlaybackPolling() {
        val desiredKey = "local-companion"
        if (playbackEventsKey == desiredKey && playbackEventsJob?.isActive == true) {
            return
        }
        playbackEventsJob?.cancel()
        playbackEventsKey = desiredKey
        playbackEventsJob = viewModelScope.launch {
            while (isActive && uiState.value.sourceKind == MusicSourceKind.LocalCompanion) {
                refreshLocalPlaybackSurfacesOnce()
                delay(1_000)
            }
        }
    }

    private suspend fun refreshLocalPlaybackSurfacesOnce() {
        val baseUrl = LocalCompanionRepository.LOCAL_COMPANION_BASE_URL
        val rendererLocation = LocalCompanionRepository.LOCAL_COMPANION_RENDERER
        runCatching {
            loadLocalPlaybackSurfaces(baseUrl, rendererLocation)
        }.recoverCatching {
            prepareLocalCompanionControlSurface(waitForStartup = true)
            loadLocalPlaybackSurfaces(baseUrl, rendererLocation)
        }.onSuccess { (nowPlaying, queue) ->
            localCompanionLastfmScrobbler.handlePlaybackEvent(
                PlaybackEventDto(
                    rendererLocation = rendererLocation,
                    nowPlaying = nowPlaying,
                    queue = queue,
                ),
            )
            _uiState.update {
                if (it.sourceKind == MusicSourceKind.LocalCompanion) {
                    it.copy(
                        nowPlaying = nowPlaying,
                        queue = queue,
                        isLoading = false,
                        warningMessage = null,
                    )
                } else {
                    it
                }
            }
        }.onFailure { error ->
            _uiState.update {
                if (it.sourceKind == MusicSourceKind.LocalCompanion) {
                    it.copy(
                        isLoading = false,
                        warningMessage = connectionErrorMessage(error),
                    )
                } else {
                    it
                }
            }
        }
    }

    fun toggleRendererPicker(show: Boolean) {
        _uiState.update {
            if (show) {
                it.copy(showRendererPicker = true)
            } else {
                it.copy(
                    showRendererPicker = false,
                    rendererGroupErrorMessage = null,
                )
            }
        }
    }

    fun deleteRendererGroup(location: String, inheritRendererLocation: String? = null) {
        val state = uiState.value
        val baseUrl = state.baseUrl
        if (baseUrl.isBlank() || location.isBlank()) return
        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    isCreatingRendererGroup = true,
                    rendererGroupErrorMessage = null,
                    errorMessage = null,
                    warningMessage = null,
                    infoMessage = null,
                )
            }
            runCatching {
                val response = repository.deleteRendererGroup(baseUrl, location, inheritRendererLocation)
                val renderers = repository.getRenderers(baseUrl)
                response to renderers
            }.onSuccess { (response, renderers) ->
                val preferred = inheritRendererLocation?.takeIf { it.isNotBlank() }
                val nextSelection = chooseRendererLocation(
                    currentSelection = preferred
                        ?: uiState.value.selectedRendererLocation.takeUnless { it == location }.orEmpty(),
                    savedSelection = repository.loadRendererLocation().takeUnless { it == location }.orEmpty(),
                    renderers = renderers,
                ).orEmpty()
                if (nextSelection.isBlank()) {
                    repository.clearRendererLocation()
                }
                _uiState.update {
                    it.copy(
                        renderers = renderers,
                        selectedRendererLocation = nextSelection,
                        isCreatingRendererGroup = false,
                        infoMessage = response.message ?: "Renderer group deleted.",
                    )
                }
                syncPlaybackNotificationService()
                refreshPlaybackSurfaces()
            }.onFailure { error ->
                _uiState.update {
                    it.copy(
                        isCreatingRendererGroup = false,
                        rendererGroupErrorMessage = connectionErrorMessage(error),
                    )
                }
            }
        }
    }

    fun removeRendererGroupMember(groupLocation: String, memberLocation: String) {
        val state = uiState.value
        val baseUrl = state.baseUrl
        if (baseUrl.isBlank() || groupLocation.isBlank() || memberLocation.isBlank()) return
        val group = state.renderers
            .firstOrNull { it.location == groupLocation && it.kind == "group" }
            ?.group ?: return
        val remainingMemberLocations = group.members
            .map { it.rendererLocation }
            .filterNot { it == memberLocation }
        if (remainingMemberLocations.size == group.members.size) return
        if (remainingMemberLocations.size < 2) {
            val inheritor = remainingMemberLocations.firstOrNull()
            deleteRendererGroup(groupLocation, inheritor)
            return
        }

        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    isCreatingRendererGroup = true,
                    rendererGroupErrorMessage = null,
                    errorMessage = null,
                    warningMessage = null,
                    infoMessage = null,
                )
            }
            runCatching {
                val response = repository.updateRendererGroup(
                    baseUrl = baseUrl,
                    rendererLocation = groupLocation,
                    name = group.name,
                    memberLocations = remainingMemberLocations,
                )
                val renderers = repository.getRenderers(baseUrl)
                response to renderers
            }.onSuccess { (response, renderers) ->
                _uiState.update {
                    it.copy(
                        renderers = renderers,
                        isCreatingRendererGroup = false,
                        rendererGroupErrorMessage = null,
                        infoMessage = response.message ?: "Renderer removed from group.",
                    )
                }
                syncPlaybackNotificationService()
                refreshPlaybackSurfaces()
            }.onFailure { error ->
                _uiState.update {
                    it.copy(
                        isCreatingRendererGroup = false,
                        rendererGroupErrorMessage = connectionErrorMessage(error),
                    )
                }
            }
        }
    }

    fun quickAddRendererToTarget(targetLocation: String, memberLocation: String) {
        val state = uiState.value
        val baseUrl = state.baseUrl
        if (baseUrl.isBlank() || targetLocation.isBlank() || memberLocation.isBlank()) return
        val physicalRendererLocations = state.renderers
            .filter { it.kind != "group" && it.directAccess }
            .map { it.location }
            .toSet()
        if (memberLocation !in physicalRendererLocations) return
        val target = state.renderers.firstOrNull { it.location == targetLocation } ?: return
        val mutation = if (target.kind == "group") {
            val group = target.group ?: return
            val memberLocations = (group.members.map { it.rendererLocation } + memberLocation)
                .filter { it in physicalRendererLocations }
                .distinct()
            if (memberLocations.size == group.members.size) return
            RendererGroupQuickAddMutation.UpdateGroup(
                groupLocation = target.location,
                groupName = group.name,
                memberLocations = memberLocations,
            )
        } else {
            if (target.location !in physicalRendererLocations || target.location == memberLocation) return
            RendererGroupQuickAddMutation.CreateAdHocGroup(
                sourceRendererLocation = target.location,
                memberLocations = listOf(target.location, memberLocation),
            )
        }

        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    isCreatingRendererGroup = true,
                    rendererGroupErrorMessage = null,
                    errorMessage = null,
                    warningMessage = null,
                    infoMessage = null,
                )
            }
            runCatching {
                val response = when (mutation) {
                    is RendererGroupQuickAddMutation.CreateAdHocGroup -> {
                        repository.createRendererGroup(
                            baseUrl = baseUrl,
                            name = "",
                            memberLocations = mutation.memberLocations,
                            sourceRendererLocation = mutation.sourceRendererLocation,
                        )
                    }
                    is RendererGroupQuickAddMutation.UpdateGroup -> {
                        repository.updateRendererGroup(
                            baseUrl = baseUrl,
                            rendererLocation = mutation.groupLocation,
                            name = mutation.groupName,
                            memberLocations = mutation.memberLocations,
                        )
                    }
                }
                val renderers = repository.getRenderers(baseUrl)
                response to renderers
            }.onSuccess { (response, renderers) ->
                val selectedRendererLocation = when (mutation) {
                    is RendererGroupQuickAddMutation.CreateAdHocGroup -> {
                        response.rendererLocation
                            ?.takeIf { location -> renderers.any { it.location == location } }
                            ?: uiState.value.selectedRendererLocation
                    }
                    is RendererGroupQuickAddMutation.UpdateGroup -> uiState.value.selectedRendererLocation
                }
                if (
                    mutation is RendererGroupQuickAddMutation.CreateAdHocGroup &&
                    selectedRendererLocation != uiState.value.selectedRendererLocation
                ) {
                    repository.saveRendererLocation(selectedRendererLocation)
                }
                _uiState.update {
                    it.copy(
                        renderers = renderers,
                        selectedRendererLocation = selectedRendererLocation,
                        isCreatingRendererGroup = false,
                        rendererGroupErrorMessage = null,
                        showRendererPicker = mutation is RendererGroupQuickAddMutation.UpdateGroup &&
                            it.showRendererPicker,
                        infoMessage = response.message ?: "Renderer added.",
                    )
                }
                syncPlaybackNotificationService()
                refreshPlaybackSurfaces()
            }.onFailure { error ->
                _uiState.update {
                    it.copy(
                        isCreatingRendererGroup = false,
                        rendererGroupErrorMessage = connectionErrorMessage(error),
                    )
                }
            }
        }
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
        syncPlaybackNotificationService()
        refreshPlaybackSurfaces()
    }

    fun openAlbum(albumId: String, preserveArtistContext: Boolean = false) {
        val baseUrl = uiState.value.baseUrl
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            _uiState.update {
                it.copy(
                    isLoading = true,
                    errorMessage = null,
                    warningMessage = null,
                    selectedAlbumRecommendations = emptyList(),
                )
            }
            runCatching {
                val album = repository.getAlbumDetail(baseUrl, albumId)
                val recommendations = runCatching {
                    repository.getAlbumRecommendations(baseUrl, albumId).recommendations
                }.getOrDefault(emptyList())
                album to recommendations
            }
                .onSuccess { (album, recommendations) ->
                    _uiState.update {
                        val artistContext = if (preserveArtistContext) it.selectedArtistDetail else null
                        it.copy(
                            selectedTab = MusicdTab.Library,
                            selectedArtistDetail = artistContext,
                            selectedAlbumDetail = album,
                            selectedAlbumRecommendations = recommendations,
                            isLoading = false,
                            warningMessage = null,
                        )
                    }
                }
                .onFailure { error ->
                    if (isUnavailableAlbumError(error, includeInvalidResponse = true)) {
                        refreshLibraryAfterAlbumUnavailable(baseUrl, albumId)
                    } else {
                        _uiState.update {
                            it.copy(
                                isLoading = false,
                                errorMessage = connectionErrorMessage(error),
                            )
                        }
                    }
                }
        }
    }

    fun closeAlbumDetail() {
        _uiState.update {
            it.copy(
                selectedAlbumDetail = null,
                selectedAlbumRecommendations = emptyList(),
            )
        }
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
                            selectedAlbumRecommendations = emptyList(),
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
        if (uiState.value.sourceKind == MusicSourceKind.LocalCompanion) {
            _uiState.update { it.copy(infoMessage = "The local companion only plays on this phone for now.") }
            return
        }
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            _uiState.update { it.copy(isDiscovering = true, errorMessage = null, warningMessage = null) }
            runCatching {
                repository.discoverRenderers(baseUrl)
                repository.getRenderers(baseUrl)
            }.onSuccess { discovered ->
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

    fun transportPlay() {
        if (!canRequestPlaybackResume(uiState.value)) {
            _uiState.update { it.copy(infoMessage = "Nothing queued to play.") }
            return
        }
        transportAction { baseUrl, renderer ->
            repository.transportPlay(baseUrl, renderer)
        }
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
                    syncPlaybackNotificationService()
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
                    syncPlaybackNotificationService()
                    refreshPlaybackSurfaces()
                }
                .onFailure { error ->
                    if (isUnavailableAlbumError(error)) {
                        refreshLibraryAfterAlbumUnavailable(baseUrl, albumId)
                    } else {
                        _uiState.update {
                            it.copy(
                                isLoading = false,
                                errorMessage = connectionErrorMessage(error),
                            )
                        }
                    }
                }
        }
    }

    fun playRadioStation(station: RadioStationDto) {
        val baseUrl = uiState.value.baseUrl
        val rendererLocation = uiState.value.selectedRendererLocation
        if (baseUrl.isBlank() || rendererLocation.isBlank()) {
            _uiState.update { it.copy(errorMessage = "Choose a renderer first.") }
            return
        }
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
            runCatching { repository.playRadioStation(baseUrl, rendererLocation, station) }
                .onSuccess { response ->
                    _uiState.update { it.copy(infoMessage = response.message ?: "Radio started.") }
                    syncPlaybackNotificationService()
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

    fun playTidalTrack(track: TidalTrackDto) {
        val baseUrl = uiState.value.baseUrl
        val rendererLocation = uiState.value.selectedRendererLocation
        if (baseUrl.isBlank() || rendererLocation.isBlank()) {
            _uiState.update { it.copy(errorMessage = "Choose a renderer first.") }
            return
        }
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
            runCatching { repository.playTidalTrack(baseUrl, rendererLocation, track) }
                .onSuccess { response ->
                    _uiState.update { it.copy(infoMessage = response.message ?: "TIDAL track started.") }
                    syncPlaybackNotificationService()
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

    fun playTidalAlbum(album: TidalAlbumDto) {
        val baseUrl = uiState.value.baseUrl
        val rendererLocation = uiState.value.selectedRendererLocation
        if (baseUrl.isBlank() || rendererLocation.isBlank()) {
            _uiState.update { it.copy(errorMessage = "Choose a renderer first.") }
            return
        }
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true, errorMessage = null, warningMessage = null) }
            runCatching { repository.playTidalAlbum(baseUrl, rendererLocation, album) }
                .onSuccess { response ->
                    _uiState.update { it.copy(infoMessage = response.message ?: "TIDAL album started.") }
                    syncPlaybackNotificationService()
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

    fun appendTidalTrack(track: TidalTrackDto) = queueMutationAction("TIDAL track queued.") { baseUrl, renderer ->
        repository.appendTidalTrack(baseUrl, renderer, track)
    }

    fun playNextTidalTrack(track: TidalTrackDto) = queueMutationAction(
        "TIDAL track queued to play next.",
    ) { baseUrl, renderer ->
        repository.playNextTidalTrack(baseUrl, renderer, track)
    }

    fun appendTidalAlbum(album: TidalAlbumDto) = queueMutationAction("TIDAL album queued.") { baseUrl, renderer ->
        repository.appendTidalAlbum(baseUrl, renderer, album)
    }

    fun playNextTidalAlbum(album: TidalAlbumDto) = queueMutationAction(
        "TIDAL album queued to play next.",
    ) { baseUrl, renderer ->
        repository.playNextTidalAlbum(baseUrl, renderer, album)
    }

    fun appendAlbum(albumId: String) = queueMutationAction(
        fallbackMessage = "Album queued.",
        unavailableAlbumId = albumId,
    ) { baseUrl, renderer ->
        repository.appendAlbum(baseUrl, renderer, albumId)
    }

    fun playNextAlbum(albumId: String) = queueMutationAction(
        fallbackMessage = "Album queued to play next.",
        unavailableAlbumId = albumId,
    ) { baseUrl, renderer ->
        repository.playNextAlbum(baseUrl, renderer, albumId)
    }

    fun likeAlbum(albumId: String) {
        likeItem { baseUrl -> repository.likeAlbum(baseUrl, albumId) }
    }

    fun likeTrack(trackId: String) {
        likeItem { baseUrl -> repository.likeTrack(baseUrl, trackId) }
    }

    private fun likeItem(request: suspend (String) -> LikeResponseDto) {
        if (uiState.value.sourceKind == MusicSourceKind.LocalCompanion) {
            _uiState.update { it.copy(infoMessage = "Likes are not wired for local companion mode yet.") }
            return
        }
        val baseUrl = uiState.value.baseUrl
        if (baseUrl.isBlank()) return
        viewModelScope.launch {
            runCatching { request(baseUrl) }
                .onSuccess { response ->
                    _uiState.update { state -> state.applyLikeResponse(response) }
                }
                .onFailure { error ->
                    _uiState.update {
                        it.copy(errorMessage = connectionErrorMessage(error))
                    }
                }
        }
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
                    syncPlaybackNotificationService()
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
        unavailableAlbumId: String? = null,
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
                    syncPlaybackNotificationService()
                    refreshPlaybackSurfaces()
                }
                .onFailure { error ->
                    if (unavailableAlbumId != null && isUnavailableAlbumError(error)) {
                        refreshLibraryAfterAlbumUnavailable(baseUrl, unavailableAlbumId)
                    } else {
                        _uiState.update {
                            it.copy(
                                isLoading = false,
                                errorMessage = connectionErrorMessage(error),
                            )
                        }
                    }
                }
        }
    }

    private fun refreshLibraryAfterAlbumUnavailable(baseUrl: String, albumId: String) {
        viewModelScope.launch {
            val shouldRefreshTracks = uiState.value.tracks.isNotEmpty()
            val refreshedAlbums = runCatching {
                repository.getAlbums(baseUrl).filterNot { it.id == albumId }
            }.getOrNull()
            val refreshedArtists = runCatching {
                repository.getArtists(baseUrl)
            }.getOrNull()
            val refreshedTracks = if (shouldRefreshTracks) {
                runCatching {
                    repository.getTracks(baseUrl).filterNot { it.albumId == albumId }
                }.getOrNull()
            } else {
                null
            }

            _uiState.update { state ->
                if (state.baseUrl != baseUrl) {
                    return@update state
                }
                val selectedArtistDetail = state.selectedArtistDetail?.let { artist ->
                    artist.copy(
                        albums = artist.albums.filterNot { it.id == albumId },
                    )
                }
                state.copy(
                    albums = refreshedAlbums ?: state.albums.filterNot { it.id == albumId },
                    artists = refreshedArtists ?: state.artists,
                    tracks = refreshedTracks ?: if (shouldRefreshTracks) {
                        state.tracks.filterNot { it.albumId == albumId }
                    } else {
                        state.tracks
                    },
                    selectedAlbumDetail = state.selectedAlbumDetail?.takeUnless { it.id == albumId },
                    selectedAlbumRecommendations = if (state.selectedAlbumDetail?.id == albumId) {
                        emptyList()
                    } else {
                        state.selectedAlbumRecommendations
                    },
                    selectedArtistDetail = selectedArtistDetail,
                    suppressedSpotlightAlbumIds = state.suppressedSpotlightAlbumIds + albumId,
                    isLoading = false,
                    errorMessage = null,
                    warningMessage = "That album is no longer available, so it was removed from the spotlight.",
                )
            }
        }
    }

    private fun refreshPlaybackSurfaces() {
        val baseUrl = uiState.value.baseUrl
        val rendererLocation = uiState.value.selectedRendererLocation
        if (uiState.value.sourceKind == MusicSourceKind.LocalCompanion) {
            refreshLocalPlaybackSurfaces()
            return
        }
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

    private fun refreshLocalPlaybackSurfaces() {
        viewModelScope.launch {
            refreshLocalPlaybackSurfacesOnce()
        }
    }

    private suspend fun loadLocalPlaybackSurfaces(
        baseUrl: String,
        rendererLocation: String,
    ): Pair<NowPlayingDto, QueueDto> {
        prepareLocalCompanionControlSurface(waitForStartup = false)
        val nowPlaying = repository.getNowPlaying(baseUrl, rendererLocation)
        val queue = repository.getQueue(baseUrl, rendererLocation)
        return nowPlaying to queue
    }

    private fun chooseRendererLocation(
        currentSelection: String,
        savedSelection: String,
        renderers: List<RendererDto>,
    ): String? {
        val selectableRenderers = renderers.filter { it.kind == "group" || it.directAccess }
        return when {
            currentSelection.isNotBlank() && selectableRenderers.any { it.location == currentSelection } -> currentSelection
            savedSelection.isNotBlank() && selectableRenderers.any { it.location == savedSelection } -> savedSelection
            selectableRenderers.any { it.selected } -> selectableRenderers.first { it.selected }.location
            else -> selectableRenderers.firstOrNull()?.location
        }?.also(repository::saveRendererLocation)
    }

    private fun localCompanionRenderer(): RendererDto =
        RendererDto(
            location = LocalCompanionRepository.LOCAL_COMPANION_RENDERER,
            name = "This phone",
            manufacturer = Build.MANUFACTURER,
            modelName = Build.MODEL,
            selected = true,
            kind = "android_local",
            directAccess = true,
        )

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
        playbackEventFailureCount = 0
    }

    private fun syncPlaybackNotificationService() {
        val state = uiState.value
        val baseUrl = state.baseUrl
        val rendererLocation = state.selectedRendererLocation
        if (!state.connected || baseUrl.isBlank() || rendererLocation.isBlank()) {
            MusicdPlaybackNotificationService.stop(getApplication())
            return
        }
        val notificationStarted = MusicdPlaybackNotificationService.start(
            context = getApplication(),
            baseUrl = baseUrl,
            rendererLocation = rendererLocation,
            localRendererLocation = androidLocalRendererLocation(),
            serverName = state.serverName,
        )
        if (!notificationStarted) {
            _uiState.update {
                if (it.connected && it.baseUrl == baseUrl && it.selectedRendererLocation == rendererLocation) {
                    it.copy(warningMessage = PLAYBACK_NOTIFICATION_UNAVAILABLE_MESSAGE)
                } else {
                    it
                }
            }
        }
    }

    private fun applyPlaybackEvent(
        baseUrl: String,
        rendererLocation: String,
        event: PlaybackEventDto,
    ) {
        playbackEventFailureCount = 0
        _uiState.update {
            if (it.baseUrl != baseUrl || it.selectedRendererLocation != rendererLocation) {
                it
            } else {
                it.copy(
                    nowPlaying = event.nowPlaying,
                    queue = event.queue,
                    warningMessage = event.nowPlaying.session?.lastError,
                )
            }
        }
    }
}

private fun normalizeLibraryName(value: String): String =
    value.trim().lowercase()

private fun MusicdUiState.withLastfmSettings(settings: LastfmSettings): MusicdUiState =
    copy(
        lastfmApiKey = settings.apiKey,
        lastfmSharedSecret = settings.sharedSecret,
        lastfmUsername = settings.username,
        lastfmPendingToken = settings.pendingToken,
    )

private fun canRequestPlaybackNavigation(state: MusicdUiState): Boolean =
    state.queue?.entries?.isNotEmpty() == true ||
        state.nowPlaying?.currentTrack != null ||
        state.nowPlaying?.session?.queueEntryId != null

private fun canRequestPlaybackResume(state: MusicdUiState): Boolean =
    state.queue?.currentEntryId != null ||
        state.queue?.entries?.any { entry ->
            entry.entryStatus.equals("playing", ignoreCase = true) ||
                entry.entryStatus.equals("pending", ignoreCase = true) ||
                entry.entryStatus.equals("queued", ignoreCase = true)
        } == true ||
        state.nowPlaying?.currentTrack != null

private fun MusicdUiState.applyLikeResponse(response: LikeResponseDto): MusicdUiState =
    when (response.itemKind) {
        "album" -> copy(
            albums = albums.map { album ->
                if (album.id == response.itemId) {
                    album.copy(likeCount = response.likeCount, likedByClient = response.likedByClient)
                } else {
                    album
                }
            },
            selectedAlbumDetail = selectedAlbumDetail?.let { album ->
                if (album.id == response.itemId) {
                    album.copy(likeCount = response.likeCount, likedByClient = response.likedByClient)
                } else {
                    album
                }
            },
            selectedArtistDetail = selectedArtistDetail?.let { artist ->
                artist.copy(
                    albums = artist.albums.map { album ->
                        if (album.id == response.itemId) {
                            album.copy(
                                likeCount = response.likeCount,
                                likedByClient = response.likedByClient,
                            )
                        } else {
                            album
                        }
                    },
                )
            },
        )
        "track" -> copy(
            tracks = tracks.map { track ->
                if (track.id == response.itemId) {
                    track.copy(likeCount = response.likeCount, likedByClient = response.likedByClient)
                } else {
                    track
                }
            },
            selectedAlbumDetail = selectedAlbumDetail?.let { album ->
                album.copy(
                    tracks = album.tracks.map { track ->
                        if (track.id == response.itemId) {
                            track.copy(
                                likeCount = response.likeCount,
                                likedByClient = response.likedByClient,
                            )
                        } else {
                            track
                        }
                    },
                )
            },
            nowPlaying = nowPlaying?.let { nowPlaying ->
                nowPlaying.copy(
                    currentTrack = nowPlaying.currentTrack?.let { track ->
                        if (track.id == response.itemId) {
                            track.copy(
                                likeCount = response.likeCount,
                                likedByClient = response.likedByClient,
                            )
                        } else {
                            track
                        }
                    },
                )
            },
        )
        else -> this
    }

private sealed interface RendererGroupQuickAddMutation {
    data class CreateAdHocGroup(
        val sourceRendererLocation: String,
        val memberLocations: List<String>,
    ) : RendererGroupQuickAddMutation

    data class UpdateGroup(
        val groupLocation: String,
        val groupName: String,
        val memberLocations: List<String>,
    ) : RendererGroupQuickAddMutation
}

private fun isUnavailableAlbumError(
    error: Throwable,
    includeInvalidResponse: Boolean = false,
): Boolean =
    when (error) {
        is MusicdApiException.Http -> {
            val serverMessage = error.serverMessage.orEmpty()
            (error.statusCode == 404 && serverMessage.contains("album", ignoreCase = true)) ||
                serverMessage.contains("album not found", ignoreCase = true) ||
                serverMessage.contains("queued track not found", ignoreCase = true) ||
                serverMessage.contains("no such file", ignoreCase = true)
        }
        is MusicdApiException.InvalidResponse -> includeInvalidResponse
        else -> false
    }

private const val TRACKS_WARNING_MESSAGE = "Track library unavailable right now."
private const val PLAYBACK_EVENT_RECONNECTING_MESSAGE = "Reconnecting live playback updates…"
private const val PLAYBACK_NOTIFICATION_UNAVAILABLE_MESSAGE =
    "Connected, but Android blocked the playback notification service."
private const val PLAYBACK_EVENT_WARNING_THRESHOLD = 3
private const val HOME_RECOMMENDATION_LIMIT = 6

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
