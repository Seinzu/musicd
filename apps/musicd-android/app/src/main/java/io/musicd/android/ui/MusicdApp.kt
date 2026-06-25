package io.musicd.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.AddToQueue
import androidx.compose.material.icons.rounded.Album
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Home
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PhoneAndroid
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.QueuePlayNext
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.Wifi
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.foundation.BorderStroke
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedIconButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.TextButton
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.compose.currentStateAsState
import coil.compose.AsyncImage
import io.musicd.android.data.AlbumDetailDto
import io.musicd.android.data.AlbumArtworkCandidateDto
import io.musicd.android.data.AlbumRecommendationDto
import io.musicd.android.data.AlbumSummaryDto
import io.musicd.android.data.ArtistDetailDto
import io.musicd.android.data.ArtistSummaryDto
import io.musicd.android.data.DiscoveredServer
import io.musicd.android.data.MusicSourceKind
import io.musicd.android.data.QueueEntryDto
import io.musicd.android.data.RadioStationDto
import io.musicd.android.data.RendererDto
import io.musicd.android.data.TidalAlbumDto
import io.musicd.android.data.TidalTrackDto
import io.musicd.android.data.TrackSummaryDto
import java.time.LocalDate
import java.time.LocalTime
import kotlinx.coroutines.launch
import kotlin.math.roundToInt
import kotlin.random.Random

private data class LibrarySearchResults(
    val artists: List<ArtistSummaryDto>,
    val albums: List<AlbumSummaryDto>,
    val tracks: List<TrackSummaryDto>,
) {
    fun isEmpty(): Boolean = artists.isEmpty() && albums.isEmpty() && tracks.isEmpty()
}

private data class AlphabetJumpTarget(
    val label: String,
    val itemIndex: Int,
)

@Composable
fun MusicdApp(viewModel: MusicdViewModel) {
    val state by viewModel.uiState.collectAsStateWithLifecycle()
    val lifecycleOwner = LocalLifecycleOwner.current
    val lifecycleState by lifecycleOwner.lifecycle.currentStateAsState()

    LaunchedEffect(state.connected, state.selectedRendererLocation, state.isConnecting, lifecycleState) {
        viewModel.updatePlaybackEventSubscription(
            state.connected &&
                state.selectedRendererLocation.isNotBlank() &&
                !state.isConnecting &&
                lifecycleState.isAtLeast(Lifecycle.State.STARTED)
        )
    }

    if (!state.connected) {
        ServerSetupScreen(
            serverInput = state.serverInput,
            serverName = state.serverName,
            isConnecting = state.isConnecting,
            errorMessage = state.errorMessage,
            isDiscoveringServers = state.isDiscoveringServers,
            hasRunServerDiscovery = state.hasRunServerDiscovery,
            discoveredServers = state.discoveredServers,
            onServerInputChange = viewModel::updateServerInput,
            onConnect = { viewModel.connect() },
            onUseLocalCompanion = { viewModel.connectLocalCompanion() },
            onOpenLocalCompanion = viewModel::openLocalCompanion,
            onDiscoverServers = { viewModel.discoverServers() },
            onSelectDiscoveredServer = viewModel::selectDiscoveredServer,
        )
        return
    }

    MusicdRoot(
        state = state,
        onSelectTab = viewModel::selectTab,
        onSelectLibraryBrowseMode = viewModel::selectLibraryBrowseMode,
        onSelectLibrarySearchFacet = viewModel::selectLibrarySearchFacet,
        onRefresh = viewModel::refreshAll,
        onServerInputChange = viewModel::updateServerInput,
        onLastfmApiKeyChange = viewModel::updateLastfmApiKey,
        onLastfmSharedSecretChange = viewModel::updateLastfmSharedSecret,
        onBeginLastfmAuthentication = viewModel::beginLastfmAuthentication,
        onCompleteLastfmAuthentication = viewModel::completeLastfmAuthentication,
        onDisconnectLastfm = viewModel::disconnectLastfm,
        onOpenServerEditor = { viewModel.toggleServerEditor(true) },
        onDismissServerEditor = { viewModel.toggleServerEditor(false) },
        onConnect = { viewModel.connect() },
        onUseLocalCompanion = { viewModel.connectLocalCompanion() },
        onOpenLocalCompanion = viewModel::openLocalCompanion,
        onRetryConnection = viewModel::retryConnection,
        onDisconnectServer = viewModel::disconnectServer,
        onDiscoverServers = { viewModel.discoverServers() },
        onSelectDiscoveredServer = viewModel::selectDiscoveredServer,
        onOpenRendererPicker = { viewModel.toggleRendererPicker(true) },
        onDismissRendererPicker = { viewModel.toggleRendererPicker(false) },
        onDismissError = viewModel::dismissError,
        onDismissWarning = viewModel::dismissWarning,
        onSelectRenderer = viewModel::selectRenderer,
        onDiscoverRenderers = viewModel::discoverRenderers,
        onDeleteRendererGroup = viewModel::deleteRendererGroup,
        onRemoveRendererGroupMember = viewModel::removeRendererGroupMember,
        onQuickAddRendererToTarget = viewModel::quickAddRendererToTarget,
        onSetRendererVolume = viewModel::setSelectedRendererVolume,
        onPlay = viewModel::transportPlay,
        onPause = viewModel::transportPause,
        onStop = viewModel::transportStop,
        onNext = viewModel::transportNext,
        onPrevious = viewModel::transportPrevious,
        onSearchQueryChange = viewModel::updateSearchQuery,
        onRadioQueryChange = viewModel::updateRadioQuery,
        onRadioCountryCodeChange = viewModel::updateRadioCountryCode,
        onSearchRadio = viewModel::searchRadioStations,
        onPlayRadioStation = viewModel::playRadioStation,
        onTidalQueryChange = viewModel::updateTidalQuery,
        onSearchTidal = viewModel::searchTidalTracks,
        onPlayTidalAlbum = viewModel::playTidalAlbum,
        onAppendTidalAlbum = viewModel::appendTidalAlbum,
        onPlayNextTidalAlbum = viewModel::playNextTidalAlbum,
        onPlayTidalTrack = viewModel::playTidalTrack,
        onAppendTidalTrack = viewModel::appendTidalTrack,
        onPlayNextTidalTrack = viewModel::playNextTidalTrack,
        onOpenArtist = viewModel::openArtist,
        onOpenArtistByName = viewModel::openArtistByName,
        onCloseArtistDetail = viewModel::closeArtistDetail,
        onOpenAlbum = viewModel::openAlbum,
        onOpenAlbumPreservingArtist = { albumId -> viewModel.openAlbum(albumId, preserveArtistContext = true) },
        onCloseAlbumDetail = viewModel::closeAlbumDetail,
        onOpenAlbumArtworkPicker = viewModel::openAlbumArtworkPicker,
        onDismissAlbumArtworkPicker = viewModel::dismissAlbumArtworkPicker,
        onApplyAlbumArtwork = viewModel::applyAlbumArtwork,
        onPlayTrack = viewModel::playTrack,
        onPlayAlbum = viewModel::playAlbum,
        onLikeAlbum = viewModel::likeAlbum,
        onLikeTrack = viewModel::likeTrack,
        onAppendTrack = viewModel::appendTrack,
        onPlayNextTrack = viewModel::playNextTrack,
        onAppendAlbum = viewModel::appendAlbum,
        onPlayNextAlbum = viewModel::playNextAlbum,
        onMoveQueueEntryUp = viewModel::moveQueueEntryUp,
        onMoveQueueEntryDown = viewModel::moveQueueEntryDown,
        onRemoveQueueEntry = viewModel::removeQueueEntry,
        onClearQueue = viewModel::clearQueue,
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun MusicdRoot(
    state: MusicdUiState,
    onSelectTab: (MusicdTab) -> Unit,
    onSelectLibraryBrowseMode: (LibraryBrowseMode) -> Unit,
    onSelectLibrarySearchFacet: (LibrarySearchFacet) -> Unit,
    onRefresh: () -> Unit,
    onServerInputChange: (String) -> Unit,
    onLastfmApiKeyChange: (String) -> Unit,
    onLastfmSharedSecretChange: (String) -> Unit,
    onBeginLastfmAuthentication: () -> Unit,
    onCompleteLastfmAuthentication: () -> Unit,
    onDisconnectLastfm: () -> Unit,
    onOpenServerEditor: () -> Unit,
    onDismissServerEditor: () -> Unit,
    onConnect: () -> Unit,
    onUseLocalCompanion: () -> Unit,
    onOpenLocalCompanion: () -> Unit,
    onRetryConnection: () -> Unit,
    onDisconnectServer: () -> Unit,
    onDiscoverServers: () -> Unit,
    onSelectDiscoveredServer: (String) -> Unit,
    onOpenRendererPicker: () -> Unit,
    onDismissRendererPicker: () -> Unit,
    onDismissError: () -> Unit,
    onDismissWarning: () -> Unit,
    onSelectRenderer: (String) -> Unit,
    onDiscoverRenderers: () -> Unit,
    onDeleteRendererGroup: (String) -> Unit,
    onRemoveRendererGroupMember: (String, String) -> Unit,
    onQuickAddRendererToTarget: (String, String) -> Unit,
    onSetRendererVolume: (Int) -> Unit,
    onPlay: () -> Unit,
    onPause: () -> Unit,
    onStop: () -> Unit,
    onNext: () -> Unit,
    onPrevious: () -> Unit,
    onSearchQueryChange: (String) -> Unit,
    onRadioQueryChange: (String) -> Unit,
    onRadioCountryCodeChange: (String) -> Unit,
    onSearchRadio: () -> Unit,
    onPlayRadioStation: (RadioStationDto) -> Unit,
    onTidalQueryChange: (String) -> Unit,
    onSearchTidal: () -> Unit,
    onPlayTidalAlbum: (TidalAlbumDto) -> Unit,
    onAppendTidalAlbum: (TidalAlbumDto) -> Unit,
    onPlayNextTidalAlbum: (TidalAlbumDto) -> Unit,
    onPlayTidalTrack: (TidalTrackDto) -> Unit,
    onAppendTidalTrack: (TidalTrackDto) -> Unit,
    onPlayNextTidalTrack: (TidalTrackDto) -> Unit,
    onOpenArtist: (String) -> Unit,
    onOpenArtistByName: (String) -> Unit,
    onCloseArtistDetail: () -> Unit,
    onOpenAlbum: (String) -> Unit,
    onOpenAlbumPreservingArtist: (String) -> Unit,
    onCloseAlbumDetail: () -> Unit,
    onOpenAlbumArtworkPicker: () -> Unit,
    onDismissAlbumArtworkPicker: () -> Unit,
    onApplyAlbumArtwork: (String) -> Unit,
    onPlayTrack: (String) -> Unit,
    onPlayAlbum: (String) -> Unit,
    onLikeAlbum: (String) -> Unit,
    onLikeTrack: (String) -> Unit,
    onAppendTrack: (String) -> Unit,
    onPlayNextTrack: (String) -> Unit,
    onAppendAlbum: (String) -> Unit,
    onPlayNextAlbum: (String) -> Unit,
    onMoveQueueEntryUp: (Long) -> Unit,
    onMoveQueueEntryDown: (Long) -> Unit,
    onRemoveQueueEntry: (Long) -> Unit,
    onClearQueue: () -> Unit,
) {
    BackHandler(enabled = state.selectedAlbumDetail != null || state.selectedArtistDetail != null) {
        when {
            state.selectedAlbumDetail != null -> onCloseAlbumDetail()
            state.selectedArtistDetail != null -> onCloseArtistDetail()
        }
    }

    if (state.showServerEditor) {
        ModalBottomSheet(
            onDismissRequest = onDismissServerEditor,
            containerColor = MaterialTheme.colorScheme.surface,
        ) {
            ServerEditorSheet(
                serverInput = state.serverInput,
                serverName = state.serverName,
                connectedBaseUrl = if (state.sourceKind == MusicSourceKind.LocalCompanion) "" else state.baseUrl,
                isLocalCompanion = state.sourceKind == MusicSourceKind.LocalCompanion,
                isConnecting = state.isConnecting,
                errorMessage = state.errorMessage,
                isDiscoveringServers = state.isDiscoveringServers,
                hasRunServerDiscovery = state.hasRunServerDiscovery,
                discoveredServers = state.discoveredServers,
                lastfmApiKey = state.lastfmApiKey,
                lastfmSharedSecret = state.lastfmSharedSecret,
                lastfmUsername = state.lastfmUsername,
                lastfmPendingToken = state.lastfmPendingToken,
                isLastfmBusy = state.isLastfmBusy,
                onServerInputChange = onServerInputChange,
                onLastfmApiKeyChange = onLastfmApiKeyChange,
                onLastfmSharedSecretChange = onLastfmSharedSecretChange,
                onBeginLastfmAuthentication = onBeginLastfmAuthentication,
                onCompleteLastfmAuthentication = onCompleteLastfmAuthentication,
                onDisconnectLastfm = onDisconnectLastfm,
                onConnect = onConnect,
                onUseLocalCompanion = onUseLocalCompanion,
                onRetry = onRetryConnection,
                onDisconnect = onDisconnectServer,
                onDiscoverServers = onDiscoverServers,
                onSelectDiscoveredServer = onSelectDiscoveredServer,
            )
        }
    }

    if (state.showRendererPicker) {
        ModalBottomSheet(
            onDismissRequest = onDismissRendererPicker,
            containerColor = MaterialTheme.colorScheme.surface,
        ) {
            RendererPickerSheet(
                renderers = state.renderers,
                selectedRendererLocation = state.selectedRendererLocation,
                isDiscovering = state.isDiscovering,
                groupPlaybackWarning = state.nowPlaying
                    ?.session
                    ?.lastError
                    ?.takeIf { state.selectedRendererLocation.startsWith("group:") },
                isCreatingGroup = state.isCreatingRendererGroup,
                groupErrorMessage = state.rendererGroupErrorMessage,
                selectedRendererVolume = state.selectedRendererVolume,
                isLoadingRendererVolume = state.isLoadingRendererVolume,
                rendererVolumeErrorMessage = state.rendererVolumeErrorMessage,
                onSelectRenderer = onSelectRenderer,
                onSetRendererVolume = onSetRendererVolume,
                onDiscoverRenderers = onDiscoverRenderers,
                onDeleteGroup = onDeleteRendererGroup,
                onRemoveGroupMember = onRemoveRendererGroupMember,
                onQuickAddRenderer = onQuickAddRendererToTarget,
            )
        }
    }

    if (state.showAlbumArtworkPicker) {
        ModalBottomSheet(
            onDismissRequest = onDismissAlbumArtworkPicker,
            containerColor = MaterialTheme.colorScheme.surface,
        ) {
            AlbumArtworkPickerSheet(
                album = state.selectedAlbumDetail,
                candidates = state.albumArtworkCandidates,
                isSearching = state.isSearchingAlbumArtwork,
                isApplying = state.isApplyingAlbumArtwork,
                errorMessage = state.albumArtworkErrorMessage,
                onSelectCandidate = onApplyAlbumArtwork,
            )
        }
    }

    Scaffold(
        modifier = Modifier.fillMaxSize(),
        topBar = {
            TopAppBar(
                title = {
                    val selectedRenderer =
                        state.renderers.firstOrNull { it.location == state.selectedRendererLocation }
                    Column {
                        Text(currentTitle(state.selectedTab))
                        Text(
                            text = selectedRenderer?.let(::rendererDisplayName)
                                ?: state.selectedRendererLocation.ifBlank { "No renderer selected" },
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                },
                actions = {
                    IconButton(onClick = onRefresh) {
                        Icon(Icons.Rounded.Refresh, contentDescription = "Refresh")
                    }
                },
            )
        },
        bottomBar = {
            Column {
                if (shouldShowMiniPlayer(state)) {
                    MiniPlayerBar(
                        state = state,
                        onOpenQueue = { onSelectTab(MusicdTab.Queue) },
                        onPlay = onPlay,
                        onPause = onPause,
                        onNext = onNext,
                    )
                }
                NavigationBar(modifier = Modifier.navigationBarsPadding()) {
                    NavigationBarItem(
                        selected = state.selectedTab == MusicdTab.Home,
                        onClick = { onSelectTab(MusicdTab.Home) },
                        icon = { Icon(Icons.Rounded.Home, contentDescription = null) },
                        label = { Text("Home") },
                    )
                    NavigationBarItem(
                        selected = state.selectedTab == MusicdTab.Library,
                        onClick = { onSelectTab(MusicdTab.Library) },
                        icon = { Icon(Icons.Rounded.Album, contentDescription = null) },
                        label = { Text("Library") },
                    )
                    if (state.sourceKind != MusicSourceKind.LocalCompanion) {
                        NavigationBarItem(
                            selected = state.selectedTab == MusicdTab.Radio,
                            onClick = { onSelectTab(MusicdTab.Radio) },
                            icon = { Icon(Icons.Rounded.Wifi, contentDescription = null) },
                            label = { Text("Radio") },
                        )
                        NavigationBarItem(
                            selected = state.selectedTab == MusicdTab.Tidal,
                            onClick = { onSelectTab(MusicdTab.Tidal) },
                            icon = { Icon(Icons.Rounded.Album, contentDescription = null) },
                            label = { Text("TIDAL") },
                        )
                    }
                    NavigationBarItem(
                        selected = state.selectedTab == MusicdTab.Queue,
                        onClick = { onSelectTab(MusicdTab.Queue) },
                        icon = { Icon(Icons.AutoMirrored.Rounded.QueueMusic, contentDescription = null) },
                        label = { Text("Queue") },
                    )
                }
            }
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .background(MaterialTheme.colorScheme.background)
                .padding(innerPadding)
                .padding(horizontal = 16.dp),
        ) {
            Spacer(Modifier.height(8.dp))
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                AssistChip(
                    onClick = onOpenServerEditor,
                    label = { Text(serverLabel(state.serverName, state.baseUrl)) },
                )
                AssistChip(
                    onClick = if (state.sourceKind == MusicSourceKind.LocalCompanion) {
                        onOpenLocalCompanion
                    } else {
                        onOpenRendererPicker
                    },
                    modifier = Modifier.weight(1f),
                    label = {
                        Text(
                            state.renderers.firstOrNull { it.location == state.selectedRendererLocation }
                                ?.let(::rendererDisplayName)
                                ?: "Choose Renderer"
                        )
                    },
                )
                if (state.isLoading) {
                    CircularProgressIndicator(modifier = Modifier.height(24.dp), strokeWidth = 2.dp)
                }
            }
            state.errorMessage?.let {
                Spacer(Modifier.height(8.dp))
                InlineMessage(
                    text = it,
                    color = MaterialTheme.colorScheme.error,
                    actionLabel = "Dismiss",
                    onAction = onDismissError,
                )
            }
            state.warningMessage?.let {
                Spacer(Modifier.height(8.dp))
                InlineMessage(
                    text = it,
                    color = MaterialTheme.colorScheme.secondary,
                    actionLabel = "Hide",
                    onAction = onDismissWarning,
                )
            }
            state.infoMessage?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, color = MaterialTheme.colorScheme.secondary)
            }
            Spacer(Modifier.height(12.dp))
            when (state.selectedTab) {
                MusicdTab.Home -> HomeScreen(
                    state = state,
                    onPlay = onPlay,
                    onPause = onPause,
                    onStop = onStop,
                    onNext = onNext,
                    onPrevious = onPrevious,
                    onOpenAlbum = onOpenAlbum,
                    onPlayAlbum = onPlayAlbum,
                    onLikeAlbum = onLikeAlbum,
                    onAppendAlbum = onAppendAlbum,
                    onPlayNextAlbum = onPlayNextAlbum,
                    onPlayTidalAlbum = onPlayTidalAlbum,
                    onAppendTidalAlbum = onAppendTidalAlbum,
                    onPlayNextTidalAlbum = onPlayNextTidalAlbum,
                    onOpenRendererPicker = onOpenRendererPicker,
                    onDiscoverRenderers = onDiscoverRenderers,
                    onOpenServerEditor = onOpenServerEditor,
                )
                MusicdTab.Library -> LibraryScreen(
                    state = state,
                    onSelectLibraryBrowseMode = onSelectLibraryBrowseMode,
                    onSelectLibrarySearchFacet = onSelectLibrarySearchFacet,
                    onSearchQueryChange = onSearchQueryChange,
                    onOpenArtist = onOpenArtist,
                    onOpenArtistByName = onOpenArtistByName,
                    onCloseArtistDetail = onCloseArtistDetail,
                    onOpenAlbum = onOpenAlbum,
                    onOpenAlbumPreservingArtist = onOpenAlbumPreservingArtist,
                    onCloseAlbumDetail = onCloseAlbumDetail,
                    onOpenAlbumArtworkPicker = onOpenAlbumArtworkPicker,
                    onOpenLocalCompanion = onOpenLocalCompanion,
                    onPlayTrack = onPlayTrack,
                    onPlayAlbum = onPlayAlbum,
                    onLikeAlbum = onLikeAlbum,
                    onLikeTrack = onLikeTrack,
                    onAppendTrack = onAppendTrack,
                    onPlayNextTrack = onPlayNextTrack,
                    onAppendAlbum = onAppendAlbum,
                    onPlayNextAlbum = onPlayNextAlbum,
                    onPlayTidalAlbum = onPlayTidalAlbum,
                    onAppendTidalAlbum = onAppendTidalAlbum,
                    onPlayNextTidalAlbum = onPlayNextTidalAlbum,
                )
                MusicdTab.Radio -> RadioScreen(
                    state = state,
                    onRadioQueryChange = onRadioQueryChange,
                    onRadioCountryCodeChange = onRadioCountryCodeChange,
                    onSearchRadio = onSearchRadio,
                    onPlayRadioStation = onPlayRadioStation,
                    onOpenRendererPicker = onOpenRendererPicker,
                )
                MusicdTab.Tidal -> TidalScreen(
                    state = state,
                    onTidalQueryChange = onTidalQueryChange,
                    onSearchTidal = onSearchTidal,
                    onPlayTidalAlbum = onPlayTidalAlbum,
                    onAppendTidalAlbum = onAppendTidalAlbum,
                    onPlayNextTidalAlbum = onPlayNextTidalAlbum,
                    onPlayTidalTrack = onPlayTidalTrack,
                    onAppendTidalTrack = onAppendTidalTrack,
                    onPlayNextTidalTrack = onPlayNextTidalTrack,
                    onOpenRendererPicker = onOpenRendererPicker,
                )
                MusicdTab.Queue -> QueueScreen(
                    state = state,
                    onPlay = onPlay,
                    onPause = onPause,
                    onStop = onStop,
                    onNext = onNext,
                    onPrevious = onPrevious,
                    onMoveQueueEntryUp = onMoveQueueEntryUp,
                    onMoveQueueEntryDown = onMoveQueueEntryDown,
                    onRemoveQueueEntry = onRemoveQueueEntry,
                    onClearQueue = onClearQueue,
                    onOpenRendererPicker = onOpenRendererPicker,
                    onDiscoverRenderers = onDiscoverRenderers,
                )
            }
        }
    }
}

@Composable
private fun MiniPlayerBar(
    state: MusicdUiState,
    onOpenQueue: () -> Unit,
    onPlay: () -> Unit,
    onPause: () -> Unit,
    onNext: () -> Unit,
) {
    val track = state.nowPlaying?.currentTrack
    val queueEntry = currentPlaybackQueueEntry(state)
    val session = state.nowPlaying?.session
    val canNavigatePlayback = canRequestPlaybackNavigation(state)
    val canResumePlayback = canRequestPlaybackResume(state)
    val progress = sessionProgress(session)
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 12.dp, vertical = 6.dp)
            .clickable(onClick = onOpenQueue),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(22.dp),
    ) {
        Column {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 12.dp, vertical = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ArtworkSquare(
                    url = resolveUrl(state.baseUrl, track?.artworkUrl),
                    modifier = Modifier.size(52.dp),
                    fallbackText = track?.album
                        ?.ifBlank { track.title }
                        ?: queueEntry?.album?.ifBlank { queueEntry.title.orEmpty() }
                        ?: queueEntry?.title.orEmpty(),
                )
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        playbackTitle(state),
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        playbackSubtitle(state),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                val isPlaying = session?.transportState == "PLAYING" || session?.transportState == "TRANSITIONING"
                FilledIconButton(
                    onClick = if (isPlaying) onPause else onPlay,
                    enabled = isPlaying || canResumePlayback,
                ) {
                    Icon(
                        if (isPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                        contentDescription = if (isPlaying) "Pause" else "Play",
                    )
                }
                OutlinedIconButton(onClick = onNext, enabled = canNavigatePlayback) {
                    Icon(Icons.Rounded.SkipNext, contentDescription = "Next")
                }
            }
            if (progress != null && session != null) {
                LinearProgressIndicator(
                    progress = { progress },
                    modifier = Modifier.fillMaxWidth(),
                )
                PlaybackTimeRow(session = session)
            }
        }
    }
}

@Composable
private fun ServerSetupScreen(
    serverInput: String,
    serverName: String?,
    isConnecting: Boolean,
    errorMessage: String?,
    isDiscoveringServers: Boolean,
    hasRunServerDiscovery: Boolean,
    discoveredServers: List<DiscoveredServer>,
    onServerInputChange: (String) -> Unit,
    onConnect: () -> Unit,
    onUseLocalCompanion: () -> Unit,
    onOpenLocalCompanion: () -> Unit,
    onDiscoverServers: () -> Unit,
    onSelectDiscoveredServer: (String) -> Unit,
) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background)
            .padding(24.dp),
        contentAlignment = Alignment.Center,
    ) {
        Card(
            shape = RoundedCornerShape(28.dp),
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        ) {
            Column(modifier = Modifier.padding(24.dp)) {
                Text(
                    serverName?.takeIf { it.isNotBlank() } ?: "musicd",
                    style = MaterialTheme.typography.displaySmall,
                    fontWeight = FontWeight.Bold,
                )
                Spacer(Modifier.height(12.dp))
                Text(
                    "Connect to your local musicd server to browse the library, switch renderers, and control the queue.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(20.dp))
                OutlinedTextField(
                    value = serverInput,
                    onValueChange = onServerInputChange,
                    label = { Text("Server URL") },
                    placeholder = { Text("http://192.168.1.10:8787") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                errorMessage?.let {
                    Spacer(Modifier.height(8.dp))
                    Text(it, color = MaterialTheme.colorScheme.error)
                }
                Spacer(Modifier.height(16.dp))
                Button(onClick = onConnect, modifier = Modifier.fillMaxWidth(), enabled = !isConnecting) {
                    Text(if (isConnecting) "Connecting..." else "Connect")
                }
                Spacer(Modifier.height(10.dp))
                OutlinedButton(onClick = onUseLocalCompanion, modifier = Modifier.fillMaxWidth()) {
                    Icon(Icons.Rounded.PhoneAndroid, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.size(8.dp))
                    Text("Use local companion")
                }
                TextButton(onClick = onOpenLocalCompanion, modifier = Modifier.fillMaxWidth()) {
                    Text("Open musicd Companion")
                }
                Spacer(Modifier.height(20.dp))
                DiscoveredServersSection(
                    isDiscovering = isDiscoveringServers,
                    hasRunDiscovery = hasRunServerDiscovery,
                    discoveredServers = discoveredServers,
                    onDiscoverServers = onDiscoverServers,
                    onSelectDiscoveredServer = onSelectDiscoveredServer,
                )
            }
        }
    }
}

@Composable
private fun DiscoveredServersSection(
    isDiscovering: Boolean,
    hasRunDiscovery: Boolean,
    discoveredServers: List<DiscoveredServer>,
    onDiscoverServers: () -> Unit,
    onSelectDiscoveredServer: (String) -> Unit,
) {
    Column(modifier = Modifier.fillMaxWidth()) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "Servers on this network",
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.weight(1f),
            )
            if (isDiscovering) {
                CircularProgressIndicator(
                    modifier = Modifier.size(18.dp),
                    strokeWidth = 2.dp,
                )
            } else {
                TextButton(onClick = onDiscoverServers) {
                    Icon(Icons.Rounded.Wifi, contentDescription = null, modifier = Modifier.size(18.dp))
                    Spacer(Modifier.size(6.dp))
                    Text(if (hasRunDiscovery) "Search again" else "Find servers")
                }
            }
        }
        Spacer(Modifier.height(8.dp))
        when {
            discoveredServers.isNotEmpty() -> {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    discoveredServers.forEach { server ->
                        DiscoveredServerRow(
                            server = server,
                            onSelect = { onSelectDiscoveredServer(server.baseUrl) },
                        )
                    }
                }
            }
            isDiscovering -> {
                Text(
                    "Looking for musicd servers…",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            hasRunDiscovery -> {
                Text(
                    "No servers responded. Make sure the device is on the same Wi-Fi network as your musicd server.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            else -> {
                Text(
                    "Search for musicd servers on your local network.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun DiscoveredServerRow(
    server: DiscoveredServer,
    onSelect: () -> Unit,
) {
    OutlinedButton(
        onClick = onSelect,
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(14.dp),
    ) {
        Icon(Icons.Rounded.Wifi, contentDescription = null, modifier = Modifier.size(20.dp))
        Spacer(Modifier.size(12.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                server.name?.takeIf { it.isNotBlank() } ?: server.baseUrl,
                style = MaterialTheme.typography.bodyMedium,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (!server.name.isNullOrBlank()) {
                Text(
                    server.baseUrl,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun HomeScreen(
    state: MusicdUiState,
    onPlay: () -> Unit,
    onPause: () -> Unit,
    onStop: () -> Unit,
    onNext: () -> Unit,
    onPrevious: () -> Unit,
    onOpenAlbum: (String) -> Unit,
    onPlayAlbum: (String) -> Unit,
    onLikeAlbum: (String) -> Unit,
    onAppendAlbum: (String) -> Unit,
    onPlayNextAlbum: (String) -> Unit,
    onPlayTidalAlbum: (TidalAlbumDto) -> Unit,
    onAppendTidalAlbum: (TidalAlbumDto) -> Unit,
    onPlayNextTidalAlbum: (TidalAlbumDto) -> Unit,
    onOpenRendererPicker: () -> Unit,
    onDiscoverRenderers: () -> Unit,
    onOpenServerEditor: () -> Unit,
) {
    val spotlightDay = LocalDate.now()
    val spotlightAlbums = remember(state.albums, state.suppressedSpotlightAlbumIds, spotlightDay) {
        val availableAlbums = state.albums.filterNot { it.id in state.suppressedSpotlightAlbumIds }
        val eligibleAlbums = availableAlbums.filter { it.trackCount > 3 }
        if (eligibleAlbums.isEmpty()) {
            availableAlbums.take(3)
        } else {
            val dailySeed = eligibleAlbums
                .map { it.id }
                .sorted()
                .joinToString("|")
                .plus("|")
                .plus(spotlightDay.toString())
                .hashCode()
            val maxCount = minOf(5, eligibleAlbums.size)
            val minCount = minOf(3, maxCount)
            val random = Random(dailySeed)
            val targetCount = if (minCount == maxCount) {
                maxCount
            } else {
                random.nextInt(minCount, maxCount + 1)
            }
            eligibleAlbums.shuffled(random).take(targetCount)
        }
    }

    LazyColumn(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        item {
            ElevatedPanel {
                Text(homeGreeting(), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.secondary)
                Spacer(Modifier.height(6.dp))
                Text("What shall we listen to?", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(16.dp))
                NowPlayingContent(
                    state = state,
                    onPlay = onPlay,
                    onPause = onPause,
                    onStop = onStop,
                    onNext = onNext,
                    onPrevious = onPrevious,
                )
            }
        }
        if (state.renderers.isEmpty()) {
            item {
                EmptyRendererPanel(
                    onDiscoverRenderers = onDiscoverRenderers,
                    onChooseRenderer = onOpenRendererPicker,
                    onEditServer = onOpenServerEditor,
                )
            }
        }
        item {
            Text("Library spotlight", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        }
        items(spotlightAlbums, key = { "spotlight-album-${it.id}" }) { album ->
            AlbumRow(
                baseUrl = state.baseUrl,
                album = album,
                onOpenAlbum = { onOpenAlbum(album.id) },
                onPlayAlbum = { onPlayAlbum(album.id) },
                onLikeAlbum = { onLikeAlbum(album.id) },
                onAppendAlbum = { onAppendAlbum(album.id) },
                onPlayNextAlbum = { onPlayNextAlbum(album.id) },
            )
        }
        if (state.homeRecommendations.isNotEmpty()) {
            item {
                Text(
                    "Something to add to your collection",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
            }
            items(state.homeRecommendations, key = { "home-recommendation-${it.recommendationKey}" }) { recommendation ->
                AlbumRecommendationRow(
                    baseUrl = state.baseUrl,
                    recommendation = recommendation,
                    localAlbum = null,
                    supportingText = recommendationHomeReason(recommendation, state.albums),
                    canPlayTidalAlbum = state.selectedRendererLocation.isNotBlank(),
                    onPlayTidalAlbum = onPlayTidalAlbum,
                    onAppendTidalAlbum = onAppendTidalAlbum,
                    onPlayNextTidalAlbum = onPlayNextTidalAlbum,
                    onOpenAlbum = {},
                )
            }
        }
    }
}

private fun homeGreeting(now: LocalTime = LocalTime.now()): String =
    when {
        now.hour < 12 -> "Good morning"
        now.hour < 18 -> "Good afternoon"
        else -> "Good evening"
    }

@Composable
private fun RadioScreen(
    state: MusicdUiState,
    onRadioQueryChange: (String) -> Unit,
    onRadioCountryCodeChange: (String) -> Unit,
    onSearchRadio: () -> Unit,
    onPlayRadioStation: (RadioStationDto) -> Unit,
    onOpenRendererPicker: () -> Unit,
) {
    LazyColumn(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            ElevatedPanel {
                Text("Internet Radio", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(8.dp))
                Text(
                    "Search Radio Browser and send a live stream to the selected renderer.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(14.dp))
                OutlinedTextField(
                    value = state.radioQuery,
                    onValueChange = onRadioQueryChange,
                    label = { Text("Station search") },
                    placeholder = { Text("BBC, jazz, KEXP") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                Spacer(Modifier.height(10.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    OutlinedTextField(
                        value = state.radioCountryCode,
                        onValueChange = onRadioCountryCodeChange,
                        label = { Text("Country") },
                        placeholder = { Text("GB") },
                        modifier = Modifier.weight(0.35f),
                        singleLine = true,
                    )
                    Button(
                        onClick = onSearchRadio,
                        modifier = Modifier.weight(0.65f),
                        enabled = !state.isSearchingRadio,
                    ) {
                        if (state.isSearchingRadio) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(18.dp),
                                strokeWidth = 2.dp,
                                color = MaterialTheme.colorScheme.onPrimary,
                            )
                            Spacer(Modifier.size(8.dp))
                        }
                        Text(if (state.isSearchingRadio) "Searching" else "Search")
                    }
                }
                if (state.selectedRendererLocation.isBlank()) {
                    Spacer(Modifier.height(12.dp))
                    InlineMessage(
                        text = "Choose a renderer before starting a station.",
                        color = MaterialTheme.colorScheme.secondary,
                        actionLabel = "Choose",
                        onAction = onOpenRendererPicker,
                    )
                }
            }
        }

        when {
            state.radioStations.isNotEmpty() -> {
                item {
                    Text(
                        "${state.radioStations.size} stations",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                }
                items(state.radioStations, key = { "radio-${it.id}-${it.streamUrl}" }) { station ->
                    RadioStationRow(
                        station = station,
                        canPlay = state.selectedRendererLocation.isNotBlank(),
                        onPlay = { onPlayRadioStation(station) },
                    )
                }
            }
            state.isSearchingRadio -> {
                item {
                    LoadingPanel("Searching stations...")
                }
            }
            state.hasSearchedRadio -> {
                item {
                    EmptyPanel(
                        title = "No stations found",
                        body = "Try a broader station name, a different country code, or leave country blank.",
                    )
                }
            }
            else -> {
                item {
                    EmptyPanel(
                        title = "Find a station",
                        body = "Search by station name, genre, or leave the search box blank for popular stations.",
                    )
                }
            }
        }
    }
}

@Composable
private fun RadioStationRow(
    station: RadioStationDto,
    canPlay: Boolean,
    onPlay: () -> Unit,
) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(22.dp),
    ) {
        Row(
            modifier = Modifier.padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ArtworkSquare(
                url = station.artworkUrl,
                modifier = Modifier.size(64.dp),
                fallbackText = station.name.ifBlank { "Radio" },
                contentHeight = 64.dp,
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    station.name.ifBlank { "Untitled station" },
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    radioStationMeta(station),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                if (station.tags.isNotEmpty()) {
                    Spacer(Modifier.height(6.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        station.tags.take(3).forEach { tag ->
                            AssistChip(onClick = {}, label = { Text(tag, maxLines = 1) })
                        }
                    }
                }
            }
            FilledIconButton(onClick = onPlay, enabled = canPlay) {
                Icon(Icons.Rounded.PlayArrow, contentDescription = "Play station")
            }
        }
    }
}

private fun radioStationMeta(station: RadioStationDto): String =
    listOfNotNull(
        station.countryCode?.takeIf { it.isNotBlank() },
        station.language?.takeIf { it.isNotBlank() },
        station.codec?.takeIf { it.isNotBlank() },
        station.bitrate?.takeIf { it > 0 }?.let { "$it kbps" },
    ).joinToString(" · ").ifBlank { station.streamUrl }

@Composable
private fun TidalScreen(
    state: MusicdUiState,
    onTidalQueryChange: (String) -> Unit,
    onSearchTidal: () -> Unit,
    onPlayTidalAlbum: (TidalAlbumDto) -> Unit,
    onAppendTidalAlbum: (TidalAlbumDto) -> Unit,
    onPlayNextTidalAlbum: (TidalAlbumDto) -> Unit,
    onPlayTidalTrack: (TidalTrackDto) -> Unit,
    onAppendTidalTrack: (TidalTrackDto) -> Unit,
    onPlayNextTidalTrack: (TidalTrackDto) -> Unit,
    onOpenRendererPicker: () -> Unit,
) {
    LazyColumn(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            ElevatedPanel {
                Text("TIDAL", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(8.dp))
                Text(
                    "Search TIDAL and send albums or tracks to the selected renderer.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Spacer(Modifier.height(14.dp))
                OutlinedTextField(
                    value = state.tidalQuery,
                    onValueChange = onTidalQueryChange,
                    label = { Text("TIDAL search") },
                    placeholder = { Text("Artist, album, or song") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                Spacer(Modifier.height(10.dp))
                Button(
                    onClick = onSearchTidal,
                    modifier = Modifier.fillMaxWidth(),
                    enabled = !state.isSearchingTidal,
                ) {
                    if (state.isSearchingTidal) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(18.dp),
                            strokeWidth = 2.dp,
                            color = MaterialTheme.colorScheme.onPrimary,
                        )
                        Spacer(Modifier.size(8.dp))
                    }
                    Text(if (state.isSearchingTidal) "Searching" else "Search")
                }
                if (state.selectedRendererLocation.isBlank()) {
                    Spacer(Modifier.height(12.dp))
                    InlineMessage(
                        text = "Choose a renderer before starting TIDAL playback.",
                        color = MaterialTheme.colorScheme.secondary,
                        actionLabel = "Choose",
                        onAction = onOpenRendererPicker,
                    )
                }
            }
        }

        when {
            state.tidalAlbums.isNotEmpty() || state.tidalTracks.isNotEmpty() -> {
                if (state.tidalAlbums.isNotEmpty()) {
                    item {
                        Text(
                            "${state.tidalAlbums.size} albums",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                    }
                    items(state.tidalAlbums, key = { "tidal-album-${it.albumId}" }) { album ->
                        TidalAlbumRow(
                            album = album,
                            canPlay = state.selectedRendererLocation.isNotBlank(),
                            onPlay = { onPlayTidalAlbum(album) },
                            onAppend = { onAppendTidalAlbum(album) },
                            onPlayNext = { onPlayNextTidalAlbum(album) },
                        )
                    }
                }
                if (state.tidalTracks.isNotEmpty()) {
                    item {
                        Text(
                            "${state.tidalTracks.size} tracks",
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                    }
                    items(state.tidalTracks, key = { "tidal-${it.trackId}" }) { track ->
                        TidalTrackRow(
                            track = track,
                            canPlay = state.selectedRendererLocation.isNotBlank(),
                            onPlay = { onPlayTidalTrack(track) },
                            onAppend = { onAppendTidalTrack(track) },
                            onPlayNext = { onPlayNextTidalTrack(track) },
                        )
                    }
                }
            }
            state.isSearchingTidal -> {
                item {
                    LoadingPanel("Searching TIDAL...")
                }
            }
            state.hasSearchedTidal -> {
                item {
                    EmptyPanel(
                        title = "No TIDAL results",
                        body = "Try a broader artist, album, or track search.",
                    )
                }
            }
            else -> {
                item {
                    EmptyPanel(
                        title = "Find TIDAL music",
                        body = "Search by artist, album, or song title.",
                    )
                }
            }
        }
    }
}

@Composable
private fun TidalAlbumRow(
    album: TidalAlbumDto,
    canPlay: Boolean,
    onPlay: () -> Unit,
    onAppend: () -> Unit,
    onPlayNext: () -> Unit,
) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(22.dp),
    ) {
        Row(
            modifier = Modifier.padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ArtworkSquare(
                url = album.artworkUrl,
                modifier = Modifier.size(72.dp),
                fallbackText = album.title.ifBlank { "TIDAL" },
                contentHeight = 72.dp,
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    album.title.ifBlank { "Untitled album" },
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    tidalAlbumMeta(album),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilledIconButton(onClick = onPlay, enabled = canPlay) {
                        Icon(Icons.Rounded.PlayArrow, contentDescription = "Play TIDAL album")
                    }
                    OutlinedIconButton(onClick = onPlayNext, enabled = canPlay) {
                        Icon(Icons.Rounded.QueuePlayNext, contentDescription = "Play TIDAL album next")
                    }
                    OutlinedIconButton(onClick = onAppend, enabled = canPlay) {
                        Icon(Icons.Rounded.AddToQueue, contentDescription = "Add TIDAL album to queue")
                    }
                }
            }
        }
    }
}

@Composable
private fun TidalTrackRow(
    track: TidalTrackDto,
    canPlay: Boolean,
    onPlay: () -> Unit,
    onAppend: () -> Unit,
    onPlayNext: () -> Unit,
) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(22.dp),
    ) {
        Row(
            modifier = Modifier.padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ArtworkSquare(
                url = track.artworkUrl,
                modifier = Modifier.size(72.dp),
                fallbackText = track.title.ifBlank { "TIDAL" },
                contentHeight = 72.dp,
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    track.title.ifBlank { "Untitled track" },
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    tidalTrackMeta(track),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilledIconButton(onClick = onPlay, enabled = canPlay) {
                        Icon(Icons.Rounded.PlayArrow, contentDescription = "Play TIDAL track")
                    }
                    OutlinedIconButton(onClick = onPlayNext, enabled = canPlay) {
                        Icon(Icons.Rounded.QueuePlayNext, contentDescription = "Play TIDAL track next")
                    }
                    OutlinedIconButton(onClick = onAppend, enabled = canPlay) {
                        Icon(Icons.Rounded.AddToQueue, contentDescription = "Add TIDAL track to queue")
                    }
                }
            }
        }
    }
}

private fun tidalAlbumMeta(album: TidalAlbumDto): String =
    listOfNotNull(
        album.artist?.takeIf { it.isNotBlank() },
        album.trackCount?.takeIf { it > 0 }?.let { count -> "$count tracks" },
        album.releaseDate?.takeIf { it.isNotBlank() },
        album.durationSeconds?.let(::formatDuration),
    ).joinToString(" · ").ifBlank { album.albumId }

private fun tidalTrackMeta(track: TidalTrackDto): String =
    listOfNotNull(
        track.artist?.takeIf { it.isNotBlank() },
        track.album?.takeIf { it.isNotBlank() },
        track.durationSeconds?.let(::formatDuration),
    ).joinToString(" · ").ifBlank { track.trackId }

@Composable
private fun LibraryScreen(
    state: MusicdUiState,
    onSelectLibraryBrowseMode: (LibraryBrowseMode) -> Unit,
    onSelectLibrarySearchFacet: (LibrarySearchFacet) -> Unit,
    onSearchQueryChange: (String) -> Unit,
    onOpenArtist: (String) -> Unit,
    onOpenArtistByName: (String) -> Unit,
    onCloseArtistDetail: () -> Unit,
    onOpenAlbum: (String) -> Unit,
    onOpenAlbumPreservingArtist: (String) -> Unit,
    onCloseAlbumDetail: () -> Unit,
    onOpenAlbumArtworkPicker: () -> Unit,
    onOpenLocalCompanion: () -> Unit,
    onPlayTrack: (String) -> Unit,
    onPlayAlbum: (String) -> Unit,
    onLikeAlbum: (String) -> Unit,
    onLikeTrack: (String) -> Unit,
    onAppendTrack: (String) -> Unit,
    onPlayNextTrack: (String) -> Unit,
    onAppendAlbum: (String) -> Unit,
    onPlayNextAlbum: (String) -> Unit,
    onPlayTidalAlbum: (TidalAlbumDto) -> Unit,
    onAppendTidalAlbum: (TidalAlbumDto) -> Unit,
    onPlayNextTidalAlbum: (TidalAlbumDto) -> Unit,
) {
    val query = state.searchQuery.trim()
    val searchResults = rememberLibrarySearchResults(
        query = query,
        artists = state.artists,
        albums = state.albums,
        tracks = state.tracks,
    )
    val isSearching = query.isNotBlank()
    var showAllArtistResults by remember(query) { mutableStateOf(false) }
    var showAllAlbumResults by remember(query) { mutableStateOf(false) }
    var showAllTrackResults by remember(query) { mutableStateOf(false) }
    val listState = rememberLazyListState()
    val coroutineScope = rememberCoroutineScope()
    val alphabetJumpTargets = remember(isSearching, state.libraryBrowseMode, state.artists, state.albums) {
        if (isSearching) {
            emptyList()
        } else {
            when (state.libraryBrowseMode) {
                LibraryBrowseMode.Artists -> buildAlphabetJumpTargets(
                    labels = state.artists.map { it.name },
                    firstRowItemIndex = 2,
                )
                LibraryBrowseMode.Albums -> buildAlphabetJumpTargets(
                    labels = state.albums.map { it.title },
                    firstRowItemIndex = 2,
                )
            }
        }
    }

    state.selectedAlbumDetail?.let { album ->
        AlbumDetailScreen(
            baseUrl = state.baseUrl,
            album = album,
            libraryAlbums = state.albums,
            recommendations = state.selectedAlbumRecommendations,
            onBack = onCloseAlbumDetail,
            backLabel = if (state.selectedArtistDetail != null) "Back to artist" else "Back to library",
            onOpenArtist = { onOpenArtistByName(album.artist) },
            onOpenAlbum = onOpenAlbum,
            onPlayAlbum = { onPlayAlbum(album.id) },
            onLikeAlbum = { onLikeAlbum(album.id) },
            onAppendAlbum = { onAppendAlbum(album.id) },
            onPlayNextAlbum = { onPlayNextAlbum(album.id) },
            onPlayTrack = onPlayTrack,
            onLikeTrack = onLikeTrack,
            onAppendTrack = onAppendTrack,
            onPlayNextTrack = onPlayNextTrack,
            onOpenArtistByName = onOpenArtistByName,
            onOpenArtworkPicker = onOpenAlbumArtworkPicker,
            canPlayTidalAlbum = state.selectedRendererLocation.isNotBlank(),
            onPlayTidalAlbum = onPlayTidalAlbum,
            onAppendTidalAlbum = onAppendTidalAlbum,
            onPlayNextTidalAlbum = onPlayNextTidalAlbum,
        )
        return
    }

    state.selectedArtistDetail?.let { artist ->
        ArtistDetailScreen(
            baseUrl = state.baseUrl,
            artist = artist,
            onBack = onCloseArtistDetail,
            onOpenAlbum = onOpenAlbumPreservingArtist,
            onPlayAlbum = onPlayAlbum,
            onLikeAlbum = onLikeAlbum,
            onAppendAlbum = onAppendAlbum,
            onPlayNextAlbum = onPlayNextAlbum,
        )
        return
    }

    Box(modifier = Modifier.fillMaxSize()) {
        LazyColumn(
            state = listState,
            modifier = Modifier
                .fillMaxSize()
                .padding(start = if (alphabetJumpTargets.isEmpty()) 0.dp else 34.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            item {
                Text("Library", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(10.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilterChip(
                        selected = state.libraryBrowseMode == LibraryBrowseMode.Artists,
                        onClick = { onSelectLibraryBrowseMode(LibraryBrowseMode.Artists) },
                        label = { Text("Artists") },
                    )
                    FilterChip(
                        selected = state.libraryBrowseMode == LibraryBrowseMode.Albums,
                        onClick = { onSelectLibraryBrowseMode(LibraryBrowseMode.Albums) },
                        label = { Text("Albums") },
                    )
                }
                Spacer(Modifier.height(10.dp))
                OutlinedTextField(
                    value = state.searchQuery,
                    onValueChange = onSearchQueryChange,
                    label = { Text("Search library") },
                    placeholder = { Text("Artist, album, or track") },
                    modifier = Modifier.fillMaxWidth(),
                    singleLine = true,
                )
                Spacer(Modifier.height(12.dp))
                Text(
                    if (!isSearching && state.libraryBrowseMode == LibraryBrowseMode.Artists) {
                        "Browse artists, then drill into their albums."
                    } else if (!isSearching) {
                        "Browse albums or search for a specific track."
                    } else {
                        "${searchResults.artists.size} artists, ${searchResults.albums.size} albums, and ${searchResults.tracks.size} tracks match \"$query\"."
                    },
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (
                state.sourceKind == MusicSourceKind.LocalCompanion &&
                state.artists.isEmpty() &&
                state.albums.isEmpty() &&
                state.tracks.isEmpty()
            ) {
                item {
                    LocalCompanionLibraryPanel(onOpenLocalCompanion = onOpenLocalCompanion)
                }
            }
            if (isSearching) {
                if (searchResults.isEmpty()) {
                    item {
                        SearchEmptyState(query = query)
                    }
                }
                item {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        FilterChip(
                            selected = state.librarySearchFacet == LibrarySearchFacet.All,
                            onClick = { onSelectLibrarySearchFacet(LibrarySearchFacet.All) },
                            label = { Text("All") },
                        )
                        FilterChip(
                            selected = state.librarySearchFacet == LibrarySearchFacet.Artists,
                            onClick = { onSelectLibrarySearchFacet(LibrarySearchFacet.Artists) },
                            label = { Text("Artists") },
                        )
                        FilterChip(
                            selected = state.librarySearchFacet == LibrarySearchFacet.Albums,
                            onClick = { onSelectLibrarySearchFacet(LibrarySearchFacet.Albums) },
                            label = { Text("Albums") },
                        )
                        FilterChip(
                            selected = state.librarySearchFacet == LibrarySearchFacet.Tracks,
                            onClick = { onSelectLibrarySearchFacet(LibrarySearchFacet.Tracks) },
                            label = { Text("Tracks") },
                        )
                    }
                }
                if (state.librarySearchFacet == LibrarySearchFacet.All || state.librarySearchFacet == LibrarySearchFacet.Artists) {
                    val visibleArtists = if (showAllArtistResults) searchResults.artists else searchResults.artists.take(8)
                    item {
                        SearchSectionHeader("Artists", searchResults.artists.size)
                    }
                    items(visibleArtists, key = { "search-artist-${it.id}" }) { artist ->
                        ArtistRow(
                            baseUrl = state.baseUrl,
                            artist = artist,
                            onOpenArtist = { onOpenArtist(artist.id) },
                            onOpenAlbum = { onOpenAlbum(artist.firstAlbumId) },
                            highlightQuery = query,
                        )
                    }
                    item {
                        SearchExpandRow(
                            total = searchResults.artists.size,
                            defaultVisible = 8,
                            expanded = showAllArtistResults,
                            onToggle = { showAllArtistResults = !showAllArtistResults },
                        )
                    }
                }
                if (state.librarySearchFacet == LibrarySearchFacet.All || state.librarySearchFacet == LibrarySearchFacet.Albums) {
                    val visibleAlbums = if (showAllAlbumResults) searchResults.albums else searchResults.albums.take(12)
                    item {
                        SearchSectionHeader("Albums", searchResults.albums.size)
                    }
                    items(visibleAlbums, key = { "search-album-${it.id}" }) { album ->
                        AlbumRow(
                            baseUrl = state.baseUrl,
                            album = album,
                            onOpenAlbum = { onOpenAlbum(album.id) },
                            onOpenArtist = { onOpenArtistByName(album.artist) },
                            onPlayAlbum = { onPlayAlbum(album.id) },
                            onLikeAlbum = { onLikeAlbum(album.id) },
                            onAppendAlbum = { onAppendAlbum(album.id) },
                            onPlayNextAlbum = { onPlayNextAlbum(album.id) },
                            highlightQuery = query,
                        )
                    }
                    item {
                        SearchExpandRow(
                            total = searchResults.albums.size,
                            defaultVisible = 12,
                            expanded = showAllAlbumResults,
                            onToggle = { showAllAlbumResults = !showAllAlbumResults },
                        )
                    }
                }
                if (state.librarySearchFacet == LibrarySearchFacet.All || state.librarySearchFacet == LibrarySearchFacet.Tracks) {
                    val visibleTracks = if (showAllTrackResults) searchResults.tracks else searchResults.tracks.take(20)
                    item {
                        SearchSectionHeader("Tracks", searchResults.tracks.size)
                    }
                    items(visibleTracks, key = { "search-track-${it.id}" }) { track ->
                        TrackRow(
                            baseUrl = state.baseUrl,
                            track = track,
                            onPlayTrack = { onPlayTrack(track.id) },
                            onLikeTrack = { onLikeTrack(track.id) },
                            onAppendTrack = { onAppendTrack(track.id) },
                            onPlayNextTrack = { onPlayNextTrack(track.id) },
                            onOpenArtist = { onOpenArtistByName(track.artist) },
                            onOpenAlbum = { onOpenAlbum(track.albumId) },
                            highlightQuery = query,
                        )
                    }
                    item {
                        SearchExpandRow(
                            total = searchResults.tracks.size,
                            defaultVisible = 20,
                            expanded = showAllTrackResults,
                            onToggle = { showAllTrackResults = !showAllTrackResults },
                        )
                    }
                }
            } else if (state.libraryBrowseMode == LibraryBrowseMode.Artists) {
                item {
                    Text("Artists", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                }
                items(state.artists, key = { "artist-${it.id}" }) { artist ->
                    ArtistRow(
                        baseUrl = state.baseUrl,
                        artist = artist,
                        onOpenArtist = { onOpenArtist(artist.id) },
                        onOpenAlbum = { onOpenAlbum(artist.firstAlbumId) },
                        highlightQuery = null,
                    )
                }
                item {
                    Spacer(Modifier.height(24.dp))
                }
            } else {
                item {
                    Text("Albums", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                }
                items(state.albums, key = { "album-${it.id}" }) { album ->
                    AlbumRow(
                        baseUrl = state.baseUrl,
                        album = album,
                        onOpenAlbum = { onOpenAlbum(album.id) },
                        onOpenArtist = { onOpenArtistByName(album.artist) },
                        onPlayAlbum = { onPlayAlbum(album.id) },
                        onLikeAlbum = { onLikeAlbum(album.id) },
                        onAppendAlbum = { onAppendAlbum(album.id) },
                        onPlayNextAlbum = { onPlayNextAlbum(album.id) },
                        highlightQuery = null,
                    )
                }
                item {
                    Spacer(Modifier.height(24.dp))
                }
            }
        }

        if (alphabetJumpTargets.isNotEmpty()) {
            AlphabetJumpRail(
                targets = alphabetJumpTargets,
                listState = listState,
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .padding(top = 12.dp, bottom = 12.dp),
                onJumpToItem = { itemIndex ->
                    coroutineScope.launch {
                        listState.animateScrollToItem(itemIndex)
                    }
                },
            )
        }
    }
}

private fun buildAlphabetJumpTargets(
    labels: List<String>,
    firstRowItemIndex: Int,
): List<AlphabetJumpTarget> {
    if (labels.isEmpty()) {
        return emptyList()
    }

    val targets = mutableListOf<AlphabetJumpTarget>()
    val seen = mutableSetOf<String>()
    labels.forEachIndexed { index, label ->
        val normalizedLabel = alphabetBucket(label)
        if (seen.add(normalizedLabel)) {
            targets += AlphabetJumpTarget(
                label = normalizedLabel,
                itemIndex = firstRowItemIndex + index,
            )
        }
    }
    return targets
}

private fun alphabetBucket(label: String): String {
    val firstLetter = label.trim().firstOrNull()?.uppercaseChar()
    return if (firstLetter != null && firstLetter.isLetter()) {
        firstLetter.toString()
    } else {
        "#"
    }
}

@Composable
private fun AlphabetJumpRail(
    targets: List<AlphabetJumpTarget>,
    listState: LazyListState,
    modifier: Modifier = Modifier,
    onJumpToItem: (Int) -> Unit,
) {
    val activeTarget = remember(targets, listState.firstVisibleItemIndex) {
        targets.lastOrNull { it.itemIndex <= listState.firstVisibleItemIndex }
    }

    Card(
        modifier = modifier,
        shape = RoundedCornerShape(18.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.92f),
        ),
        border = BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.18f)),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 6.dp, vertical = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            targets.forEach { target ->
                val isActive = activeTarget?.label == target.label
                Text(
                    text = target.label,
                    modifier = Modifier
                        .clickable { onJumpToItem(target.itemIndex) }
                        .padding(horizontal = 4.dp, vertical = 1.dp),
                    style = MaterialTheme.typography.labelSmall,
                    fontWeight = if (isActive) FontWeight.Bold else FontWeight.Medium,
                    color = if (isActive) {
                        MaterialTheme.colorScheme.primary
                    } else {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    },
                )
            }
        }
    }
}

@Composable
private fun SearchSectionHeader(title: String, count: Int) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        Text(
            count.toString(),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.secondary,
        )
    }
}

@Composable
private fun SearchEmptyState(query: String) {
    ElevatedPanel {
        Text("No matches", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(8.dp))
        Text(
            "Nothing matched \"$query\" across artists, albums, or tracks.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun SearchExpandRow(
    total: Int,
    defaultVisible: Int,
    expanded: Boolean,
    onToggle: () -> Unit,
) {
    if (total <= defaultVisible) {
        return
    }
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.Center,
    ) {
        TextButton(onClick = onToggle) {
            Text(if (expanded) "Show fewer" else "Show more")
        }
    }
}

@Composable
private fun rememberLibrarySearchResults(
    query: String,
    artists: List<ArtistSummaryDto>,
    albums: List<AlbumSummaryDto>,
    tracks: List<TrackSummaryDto>,
): LibrarySearchResults = remember(query, artists, albums, tracks) {
    val tokens = queryTokens(query)
    if (tokens.isEmpty()) {
        LibrarySearchResults(emptyList(), emptyList(), emptyList())
    } else {
        LibrarySearchResults(
            artists = artists
                .mapNotNull { artist -> scoredArtistResult(artist, tokens) }
                .sortedByDescending { it.first }
                .map { it.second },
            albums = albums
                .mapNotNull { album -> scoredAlbumResult(album, tokens) }
                .sortedByDescending { it.first }
                .map { it.second },
            tracks = tracks
                .mapNotNull { track -> scoredTrackResult(track, tokens) }
                .sortedByDescending { it.first }
                .map { it.second },
        )
    }
}

private fun queryTokens(query: String): List<String> =
    query.lowercase()
        .split(Regex("\\s+"))
        .map(String::trim)
        .filter(String::isNotBlank)

private fun scoredArtistResult(
    artist: ArtistSummaryDto,
    tokens: List<String>,
): Pair<Int, ArtistSummaryDto>? {
    val fields = listOf(artist.name)
    val score = scoreSearchFields(fields, tokens) ?: return null
    return score to artist
}

private fun scoredAlbumResult(
    album: AlbumSummaryDto,
    tokens: List<String>,
): Pair<Int, AlbumSummaryDto>? {
    val fields = listOf(album.title, album.artist)
    val score = scoreSearchFields(fields, tokens) ?: return null
    return score to album
}

private fun scoredTrackResult(
    track: TrackSummaryDto,
    tokens: List<String>,
): Pair<Int, TrackSummaryDto>? {
    val fields = listOf(track.title, track.artist, track.album)
    val score = scoreSearchFields(fields, tokens) ?: return null
    return score to track
}

private fun scoreSearchFields(fields: List<String>, tokens: List<String>): Int? {
    val normalizedFields = fields.map { it.lowercase() }
    var score = 0
    for (token in tokens) {
        val tokenScore = normalizedFields.maxOfOrNull { field ->
            when {
                field == token -> 180
                field.startsWith(token) -> 140
                field.contains(token) -> 90
                else -> 0
            }
        } ?: 0
        if (tokenScore == 0) {
            return null
        }
        score += tokenScore
    }
    val phrase = tokens.joinToString(" ")
    if (phrase.isNotBlank()) {
        score += normalizedFields.maxOfOrNull { field ->
            when {
                field == phrase -> 220
                field.startsWith(phrase) -> 180
                field.contains(phrase) -> 120
                else -> 0
            }
        } ?: 0
    }
    return score
}

@Composable
private fun QueueScreen(
    state: MusicdUiState,
    onPlay: () -> Unit,
    onPause: () -> Unit,
    onStop: () -> Unit,
    onNext: () -> Unit,
    onPrevious: () -> Unit,
    onMoveQueueEntryUp: (Long) -> Unit,
    onMoveQueueEntryDown: (Long) -> Unit,
    onRemoveQueueEntry: (Long) -> Unit,
    onClearQueue: () -> Unit,
    onOpenRendererPicker: () -> Unit,
    onDiscoverRenderers: () -> Unit,
) {
    val entries = state.queue?.entries.orEmpty()
    val canNavigatePlayback = canRequestPlaybackNavigation(state)
    val currentEntryId = currentPlaybackQueueEntryId(state)
    val currentEntry = entries.firstOrNull {
        it.id == currentEntryId || isCurrentQueueEntryStatus(it.entryStatus)
    }
    val upcomingEntries = entries.filter { entry ->
        entry.id != currentEntry?.id && isUpcomingQueueEntryStatus(entry.entryStatus)
    }
    val hasVisibleQueueEntries = currentEntry != null || upcomingEntries.isNotEmpty()
    val trackLookup = remember(state.tracks) { state.tracks.associateBy { it.id } }
    val accentColor = Color(0xFFF5AF43)

    LazyColumn(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        item {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.Top,
                ) {
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            "Queue",
                            style = MaterialTheme.typography.displaySmall,
                            fontStyle = FontStyle.Italic,
                            fontWeight = FontWeight.Light,
                        )
                        Spacer(Modifier.height(4.dp))
                        Text(
                            queueSummaryLine(entries, currentEntry),
                            style = MaterialTheme.typography.titleMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    OutlinedButton(
                        onClick = onClearQueue,
                        enabled = entries.isNotEmpty(),
                        shape = RoundedCornerShape(99.dp),
                    ) {
                        Text("Clear")
                    }
                }
            }
        }
        if (state.renderers.isEmpty()) {
            item {
                EmptyRendererPanel(
                    onDiscoverRenderers = onDiscoverRenderers,
                    onChooseRenderer = onOpenRendererPicker,
                    onEditServer = null,
                )
            }
        }
        currentEntry?.let { entry ->
            item {
                QueueSectionHeader("Now Playing")
            }
            item {
                QueueEntryRow(
                    baseUrl = state.baseUrl,
                    entry = entry,
                    track = state.nowPlaying?.currentTrack ?: trackLookup[entry.trackId],
                    isCurrent = true,
                    accentColor = accentColor,
                    canMoveUp = false,
                    canMoveDown = false,
                    canRemove = false,
                    onMoveUp = {},
                    onMoveDown = {},
                    onRemove = {},
                )
            }
        }
        if (upcomingEntries.isNotEmpty()) {
            item {
                QueueSectionHeader("Up Next")
            }
        }
        itemsIndexed(upcomingEntries, key = { _, entry -> entry.id }) { index, entry ->
            QueueEntryRow(
                baseUrl = state.baseUrl,
                entry = entry,
                track = trackLookup[entry.trackId],
                isCurrent = false,
                accentColor = accentColor,
                canMoveUp = index > 0,
                canMoveDown = index < upcomingEntries.lastIndex,
                canRemove = true,
                onMoveUp = { onMoveQueueEntryUp(entry.id) },
                onMoveDown = { onMoveQueueEntryDown(entry.id) },
                onRemove = { onRemoveQueueEntry(entry.id) },
            )
        }
        if (!hasVisibleQueueEntries) {
            item {
                ElevatedPanel {
                    Text(
                        if (entries.isEmpty()) "Queue is empty" else "Queue has finished",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Spacer(Modifier.height(8.dp))
                    Text(
                        if (entries.isEmpty()) {
                            "Play an album or add tracks from the library to start building a queue."
                        } else {
                            "There is nothing left queued to play. Clear the finished queue or add more music from the library."
                        },
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Spacer(Modifier.height(14.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(onClick = onPrevious, enabled = canNavigatePlayback) {
                            Icon(Icons.Rounded.SkipPrevious, contentDescription = "Previous")
                        }
                        FilledIconButton(onClick = onPlay, enabled = canRequestPlaybackResume(state)) {
                            Icon(Icons.Rounded.PlayArrow, contentDescription = "Play")
                        }
                        OutlinedIconButton(onClick = onPause) {
                            Icon(Icons.Rounded.Pause, contentDescription = "Pause")
                        }
                        OutlinedIconButton(onClick = onStop) {
                            Icon(Icons.Rounded.Stop, contentDescription = "Stop")
                        }
                        OutlinedIconButton(onClick = onNext, enabled = canNavigatePlayback) {
                            Icon(Icons.Rounded.SkipNext, contentDescription = "Next")
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ServerEditorSheet(
    serverInput: String,
    serverName: String?,
    connectedBaseUrl: String,
    isLocalCompanion: Boolean,
    isConnecting: Boolean,
    errorMessage: String?,
    isDiscoveringServers: Boolean,
    hasRunServerDiscovery: Boolean,
    discoveredServers: List<DiscoveredServer>,
    lastfmApiKey: String,
    lastfmSharedSecret: String,
    lastfmUsername: String,
    lastfmPendingToken: String,
    isLastfmBusy: Boolean,
    onServerInputChange: (String) -> Unit,
    onLastfmApiKeyChange: (String) -> Unit,
    onLastfmSharedSecretChange: (String) -> Unit,
    onBeginLastfmAuthentication: () -> Unit,
    onCompleteLastfmAuthentication: () -> Unit,
    onDisconnectLastfm: () -> Unit,
    onConnect: () -> Unit,
    onUseLocalCompanion: () -> Unit,
    onRetry: () -> Unit,
    onDisconnect: () -> Unit,
    onDiscoverServers: () -> Unit,
    onSelectDiscoveredServer: (String) -> Unit,
) {
    Column(modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp)) {
        Text("Server", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(6.dp))
        Text(
            if (isLocalCompanion) {
                "Switch from the local companion to a musicd server on the local network."
            } else {
                "Point the app at your musicd server on the local network."
            },
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(16.dp))
        OutlinedTextField(
            value = serverInput,
            onValueChange = onServerInputChange,
            label = { Text("Server URL") },
            placeholder = { Text("http://192.168.1.10:8787") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        if (connectedBaseUrl.isNotBlank()) {
            Spacer(Modifier.height(10.dp))
            serverName?.takeIf { it.isNotBlank() }?.let { name ->
                Text(
                    "Connected to: $name",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                Spacer(Modifier.height(4.dp))
            }
            Text(
                "Current server: $connectedBaseUrl",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.secondary,
            )
        }
        errorMessage?.let {
            Spacer(Modifier.height(10.dp))
            Text(it, color = MaterialTheme.colorScheme.error)
        }
        Spacer(Modifier.height(16.dp))
        Button(onClick = onConnect, enabled = !isConnecting, modifier = Modifier.fillMaxWidth()) {
            Text(if (isConnecting) "Connecting..." else "Save and reconnect")
        }
        if (!isLocalCompanion) {
            Spacer(Modifier.height(8.dp))
            OutlinedButton(onClick = onRetry, enabled = !isConnecting, modifier = Modifier.fillMaxWidth()) {
                Text("Retry current server")
            }
            Spacer(Modifier.height(8.dp))
            OutlinedButton(onClick = onUseLocalCompanion, enabled = !isConnecting, modifier = Modifier.fillMaxWidth()) {
                Icon(Icons.Rounded.PhoneAndroid, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.size(8.dp))
                Text("Use local companion")
            }
        }
        Spacer(Modifier.height(16.dp))
        DiscoveredServersSection(
            isDiscovering = isDiscoveringServers,
            hasRunDiscovery = hasRunServerDiscovery,
            discoveredServers = discoveredServers,
            onDiscoverServers = onDiscoverServers,
            onSelectDiscoveredServer = onSelectDiscoveredServer,
        )
        Spacer(Modifier.height(18.dp))
        LastfmSettingsSection(
            apiKey = lastfmApiKey,
            sharedSecret = lastfmSharedSecret,
            username = lastfmUsername,
            pendingToken = lastfmPendingToken,
            isBusy = isLastfmBusy,
            onApiKeyChange = onLastfmApiKeyChange,
            onSharedSecretChange = onLastfmSharedSecretChange,
            onBeginAuthentication = onBeginLastfmAuthentication,
            onCompleteAuthentication = onCompleteLastfmAuthentication,
            onDisconnect = onDisconnectLastfm,
        )
        Spacer(Modifier.height(8.dp))
        TextButton(onClick = onDisconnect, modifier = Modifier.fillMaxWidth()) {
            Text("Disconnect")
        }
        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun LastfmSettingsSection(
    apiKey: String,
    sharedSecret: String,
    username: String,
    pendingToken: String,
    isBusy: Boolean,
    onApiKeyChange: (String) -> Unit,
    onSharedSecretChange: (String) -> Unit,
    onBeginAuthentication: () -> Unit,
    onCompleteAuthentication: () -> Unit,
    onDisconnect: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("Last.fm", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        Text(
            if (username.isNotBlank()) {
                "Scrobbling to $username."
            } else {
                "Add your Last.fm API app credentials, then authorize scrobbling."
            },
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        OutlinedTextField(
            value = apiKey,
            onValueChange = onApiKeyChange,
            label = { Text("API key") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
        )
        OutlinedTextField(
            value = sharedSecret,
            onValueChange = onSharedSecretChange,
            label = { Text("Shared secret") },
            modifier = Modifier.fillMaxWidth(),
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
        )
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
            Button(
                onClick = onBeginAuthentication,
                enabled = !isBusy && apiKey.isNotBlank() && sharedSecret.isNotBlank(),
                modifier = Modifier.weight(1f),
            ) {
                Icon(Icons.AutoMirrored.Rounded.OpenInNew, contentDescription = null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.size(8.dp))
                Text(if (pendingToken.isBlank()) "Sign in" else "Restart")
            }
            OutlinedButton(
                onClick = onCompleteAuthentication,
                enabled = !isBusy && pendingToken.isNotBlank(),
                modifier = Modifier.weight(1f),
            ) {
                Text(if (isBusy) "Working" else "Complete")
            }
        }
        if (username.isNotBlank()) {
            OutlinedButton(onClick = onDisconnect, enabled = !isBusy, modifier = Modifier.fillMaxWidth()) {
                Text("Disconnect Last.fm")
            }
        }
    }
}

@Composable
private fun EmptyRendererPanel(
    onDiscoverRenderers: () -> Unit,
    onChooseRenderer: () -> Unit,
    onEditServer: (() -> Unit)?,
) {
    ElevatedPanel {
        Text("No renderer selected", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(8.dp))
        Text(
            "Discover devices on your network, or choose one from the saved renderer list.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(14.dp))
        Button(onClick = onDiscoverRenderers, modifier = Modifier.fillMaxWidth()) {
            Text("Discover Renderers")
        }
        Spacer(Modifier.height(8.dp))
        OutlinedButton(onClick = onChooseRenderer, modifier = Modifier.fillMaxWidth()) {
            Text("Choose Saved Renderer")
        }
        onEditServer?.let {
            Spacer(Modifier.height(8.dp))
            TextButton(onClick = it, modifier = Modifier.fillMaxWidth()) {
                Text("Edit Server")
            }
        }
    }
}

@Composable
private fun InlineMessage(
    text: String,
    color: androidx.compose.ui.graphics.Color,
    actionLabel: String,
    onAction: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = text,
            color = color,
            modifier = Modifier.weight(1f),
        )
        TextButton(onClick = onAction) {
            Text(actionLabel)
        }
    }
}

@Composable
private fun RendererPickerSheet(
    renderers: List<RendererDto>,
    selectedRendererLocation: String,
    isDiscovering: Boolean,
    groupPlaybackWarning: String?,
    isCreatingGroup: Boolean,
    groupErrorMessage: String?,
    selectedRendererVolume: Int?,
    isLoadingRendererVolume: Boolean,
    rendererVolumeErrorMessage: String?,
    onSelectRenderer: (String) -> Unit,
    onSetRendererVolume: (Int) -> Unit,
    onDiscoverRenderers: () -> Unit,
    onDeleteGroup: (String) -> Unit,
    onRemoveGroupMember: (String, String) -> Unit,
    onQuickAddRenderer: (String, String) -> Unit,
) {
    val accentColor = Color(0xFFF5AF43)
    val accentContainer = Color(0xFF4B3B2B)
    val sheetBackground = Color(0xFF1F1F25)
    val physicalRenderers = renderers.filter { it.kind != "group" && it.directAccess }
    val groupRenderers = renderers.filter { it.kind == "group" }
    val selectedRenderer = renderers.firstOrNull { it.location == selectedRendererLocation }
    var pendingDeleteGroup by remember { mutableStateOf<RendererDto?>(null) }
    val targetSummary = listOfNotNull(
        groupRenderers.size.takeIf { it > 0 }?.let { "$it GROUP${if (it == 1) "" else "S"}" },
        physicalRenderers.size.takeIf { it > 0 }?.let { "$it RENDERER${if (it == 1) "" else "S"}" },
    ).joinToString(" / ").ifBlank { "0 TARGETS" }

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(sheetBackground)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp, vertical = 8.dp)
    ) {
        Box(
            modifier = Modifier
                .align(Alignment.CenterHorizontally)
                .padding(top = 4.dp)
                .background(
                    color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.18f),
                    shape = RoundedCornerShape(99.dp),
                )
                .size(width = 80.dp, height = 6.dp)
        )
        Spacer(Modifier.height(18.dp))
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                "Play on",
                style = MaterialTheme.typography.displaySmall,
                fontStyle = FontStyle.Italic,
                fontWeight = FontWeight.Light,
            )
            Text(
                text = targetSummary,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(Modifier.height(20.dp))
        selectedRenderer
            ?.takeIf { it.kind == "upnp" && it.directAccess }
            ?.let { renderer ->
                RendererVolumeControl(
                    renderer = renderer,
                    volume = selectedRendererVolume,
                    isLoading = isLoadingRendererVolume,
                    errorMessage = rendererVolumeErrorMessage,
                    onSetVolume = onSetRendererVolume,
                )
                Spacer(Modifier.height(14.dp))
            }
        if (renderers.isEmpty()) {
            ElevatedPanel {
                Text(
                    "No saved renderers yet. Run discovery to look for devices on your network.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(14.dp))
        }
        if (groupRenderers.isNotEmpty()) {
            RendererSectionHeader(
                title = "Groups",
                meta = "${groupRenderers.size}",
                modifier = Modifier.padding(bottom = 10.dp),
            )
        }
        groupRenderers.forEach { renderer ->
            RendererGroupPanel(
                renderer = renderer,
                renderers = renderers,
                isSelected = renderer.location == selectedRendererLocation,
                isBusy = isCreatingGroup,
                playbackWarning = groupPlaybackWarning.takeIf {
                    renderer.location == selectedRendererLocation
                },
                sheetBackground = sheetBackground,
                selectedContainer = accentContainer,
                selectedAccent = accentColor,
                onSelectRenderer = onSelectRenderer,
                onDeleteGroup = { pendingDeleteGroup = renderer },
                onRemoveMember = { memberLocation ->
                    onRemoveGroupMember(renderer.location, memberLocation)
                },
                onQuickAddRenderer = onQuickAddRenderer,
            )
        }
        if (physicalRenderers.isNotEmpty()) {
            RendererSectionHeader(
                title = "Renderers",
                meta = "${physicalRenderers.size}",
                modifier = Modifier.padding(top = if (groupRenderers.isEmpty()) 0.dp else 6.dp, bottom = 10.dp),
            )
        }
        physicalRenderers.forEach { renderer ->
            RendererPickerRow(
                renderer = renderer,
                isSelected = renderer.location == selectedRendererLocation,
                groupNames = rendererGroupNames(renderer, groupRenderers),
                sheetBackground = sheetBackground,
                selectedContainer = accentContainer,
                selectedAccent = accentColor,
                onSelectRenderer = onSelectRenderer,
                quickAddCandidates = physicalRenderers.filter { it.location != renderer.location },
                isBusy = isCreatingGroup,
                onQuickAddRenderer = onQuickAddRenderer,
            )
        }
        groupErrorMessage?.let {
            Text(
                it,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(vertical = 8.dp),
            )
        }
        pendingDeleteGroup?.let { group ->
            AlertDialog(
                onDismissRequest = { pendingDeleteGroup = null },
                title = { Text("Delete group?") },
                text = { Text("Delete ${rendererDisplayName(group)} and its group queue.") },
                confirmButton = {
                    TextButton(
                        onClick = {
                            pendingDeleteGroup = null
                            onDeleteGroup(group.location)
                        },
                    ) {
                        Text("Delete")
                    }
                },
                dismissButton = {
                    TextButton(onClick = { pendingDeleteGroup = null }) {
                        Text("Cancel")
                    }
                },
            )
        }
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onDiscoverRenderers)
                .padding(vertical = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (isDiscovering) {
                CircularProgressIndicator(
                    modifier = Modifier.size(20.dp),
                    strokeWidth = 2.dp,
                    color = accentColor,
                )
            } else {
                Icon(
                    Icons.Rounded.Add,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                if (isDiscovering) "Scanning for renderers..." else "Scan for new renderers",
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun RendererVolumeControl(
    renderer: RendererDto,
    volume: Int?,
    isLoading: Boolean,
    errorMessage: String?,
    onSetVolume: (Int) -> Unit,
) {
    var sliderValue by remember(renderer.location) {
        mutableStateOf((volume ?: 0).toFloat())
    }
    LaunchedEffect(renderer.location, volume) {
        volume?.let { sliderValue = it.toFloat() }
    }
    Card(
        modifier = Modifier.fillMaxWidth(),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.5f),
        ),
        shape = RoundedCornerShape(8.dp),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 14.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier.weight(1f),
                ) {
                    Icon(
                        Icons.Rounded.Speaker,
                        contentDescription = null,
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(
                        rendererDisplayName(renderer),
                        style = MaterialTheme.typography.titleMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                if (isLoading && volume == null) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(18.dp),
                        strokeWidth = 2.dp,
                    )
                } else {
                    Text(
                        "${sliderValue.roundToInt().coerceIn(0, 100)}%",
                        style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            Slider(
                value = sliderValue.coerceIn(0f, 100f),
                onValueChange = { sliderValue = it.coerceIn(0f, 100f) },
                onValueChangeFinished = {
                    onSetVolume(sliderValue.roundToInt().coerceIn(0, 100))
                },
                valueRange = 0f..100f,
                enabled = volume != null && !isLoading,
            )
            errorMessage?.let {
                Text(
                    it,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        }
    }
}

@Composable
private fun RendererGroupPanel(
    renderer: RendererDto,
    renderers: List<RendererDto>,
    isSelected: Boolean,
    isBusy: Boolean,
    playbackWarning: String?,
    sheetBackground: Color,
    selectedContainer: Color,
    selectedAccent: Color,
    onSelectRenderer: (String) -> Unit,
    onDeleteGroup: (String) -> Unit,
    onRemoveMember: (String) -> Unit,
    onQuickAddRenderer: (String, String) -> Unit,
) {
    val group = renderer.group
    val members = group?.members.orEmpty()
    val physicalByLocation = renderers
        .filter { it.kind != "group" }
        .associateBy { it.location }
    val memberIssueCount = members.count { member ->
        val memberRenderer = physicalByLocation[member.rendererLocation]
        memberRenderer == null ||
            memberRenderer.health?.reachable == false ||
            memberRenderer.health?.lastError != null
    }
    val memberStatusSummary = when {
        members.isEmpty() -> "No members"
        memberIssueCount == 0 -> "All members available"
        memberIssueCount == 1 -> "1 member needs attention"
        else -> "$memberIssueCount members need attention"
    }
    val leaderLocation = members
        .map { it.rendererLocation }
        .firstOrNull { physicalByLocation[it]?.kind != "android_local" }
        ?: members.firstOrNull()?.rendererLocation
    val quickAddCandidates = renderers
        .filter { it.kind != "group" && it.location !in members.map { member -> member.rendererLocation } }

    Card(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 14.dp),
        colors = CardDefaults.cardColors(
            containerColor = if (isSelected) selectedContainer else sheetBackground,
        ),
        border = BorderStroke(
            width = if (isSelected) 1.5.dp else 1.dp,
            color = if (isSelected) {
                selectedAccent.copy(alpha = 0.75f)
            } else {
                MaterialTheme.colorScheme.onSurface.copy(alpha = 0.08f)
            },
        ),
        shape = RoundedCornerShape(24.dp),
    ) {
        Column(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 18.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(
                    modifier = Modifier
                        .size(58.dp)
                        .background(Color(0xFF17181F), RoundedCornerShape(18.dp))
                        .border(
                            width = 1.dp,
                            color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.08f),
                            shape = RoundedCornerShape(18.dp),
                        ),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        Icons.AutoMirrored.Rounded.QueueMusic,
                        contentDescription = null,
                        tint = if (isSelected) selectedAccent else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        rendererDisplayName(renderer),
                        style = MaterialTheme.typography.headlineSmall,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        rendererDescriptor(renderer),
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(
                        memberStatusSummary,
                        style = MaterialTheme.typography.labelLarge,
                        color = if (memberIssueCount > 0) {
                            MaterialTheme.colorScheme.error
                        } else {
                            MaterialTheme.colorScheme.onSurfaceVariant
                        },
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                if (isSelected) {
                    Row(
                        modifier = Modifier
                            .background(selectedAccent, RoundedCornerShape(99.dp))
                            .padding(horizontal = 14.dp, vertical = 10.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(
                            modifier = Modifier
                                .size(12.dp)
                                .background(Color(0xFF1B140D), CircleShape)
                        )
                        Text(
                            "Active",
                            color = Color(0xFF1B140D),
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                        )
                    }
                }
            }

            if (!playbackWarning.isNullOrBlank()) {
                Text(
                    playbackWarning,
                    color = MaterialTheme.colorScheme.error,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }

            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        "Members",
                        style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    QuickAddRendererMenu(
                        targetRenderer = renderer,
                        candidates = quickAddCandidates,
                        isBusy = isBusy,
                        selectedAccent = selectedAccent,
                        onQuickAddRenderer = onQuickAddRenderer,
                    )
                }
                members.forEach { member ->
                    val memberRenderer = physicalByLocation[member.rendererLocation]
                    RendererGroupMemberRow(
                        memberLocation = member.rendererLocation,
                        memberRenderer = memberRenderer,
                        isLeader = member.rendererLocation == leaderLocation,
                        isBusy = isBusy,
                        onRemove = onRemoveMember,
                    )
                }
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                if (!isSelected) {
                    Button(
                        onClick = { onSelectRenderer(renderer.location) },
                        modifier = Modifier.weight(1f),
                    ) {
                        Text("Use")
                    }
                }
                TextButton(
                    onClick = { onDeleteGroup(renderer.location) },
                    modifier = Modifier.weight(1f),
                ) {
                    Text("Delete")
                }
            }
        }
    }
}

@Composable
private fun RendererGroupMemberRow(
    memberLocation: String,
    memberRenderer: RendererDto?,
    isLeader: Boolean,
    isBusy: Boolean,
    onRemove: (String) -> Unit,
) {
    val statusLabel = rendererHealthLabel(memberRenderer)
    val statusColor = rendererHealthColor(memberRenderer)
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            rendererIcon(memberRenderer),
            contentDescription = null,
            tint = statusColor,
            modifier = Modifier.size(18.dp),
        )
        Column(modifier = Modifier.weight(1f)) {
            Text(
                memberRenderer?.let(::rendererDisplayName)
                    ?: rendererLocationHost(memberLocation)
                    ?: memberLocation,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            memberRenderer
                ?.health
                ?.lastError
                ?.takeIf { it.isNotBlank() }
                ?.let { error ->
                    Text(
                        error,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
        }
        if (isLeader) {
            AssistChip(
                onClick = {},
                label = { Text("Leader") },
            )
        }
        Text(
            statusLabel,
            style = MaterialTheme.typography.labelLarge,
            color = statusColor,
        )
        IconButton(
            onClick = { onRemove(memberLocation) },
            enabled = !isBusy,
        ) {
            Icon(
                Icons.Rounded.Close,
                contentDescription = "Remove from group",
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun RendererPickerRow(
    renderer: RendererDto,
    isSelected: Boolean,
    groupNames: List<String>,
    quickAddCandidates: List<RendererDto>,
    isBusy: Boolean,
    sheetBackground: Color,
    selectedContainer: Color,
    selectedAccent: Color,
    onSelectRenderer: (String) -> Unit,
    onQuickAddRenderer: (String, String) -> Unit,
) {
    val membershipLabel = rendererMembershipLabel(groupNames)
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .padding(bottom = 14.dp)
            .clickable { onSelectRenderer(renderer.location) },
        colors = CardDefaults.cardColors(
            containerColor = if (isSelected) {
                selectedContainer
            } else {
                sheetBackground
            }
        ),
        border = BorderStroke(
            width = if (isSelected) 1.5.dp else 1.dp,
            color = if (isSelected) selectedAccent.copy(alpha = 0.75f)
            else MaterialTheme.colorScheme.onSurface.copy(alpha = 0.08f),
        ),
        shape = RoundedCornerShape(24.dp),
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 18.dp),
            horizontalArrangement = Arrangement.spacedBy(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(58.dp)
                    .background(
                        color = Color(0xFF17181F),
                        shape = RoundedCornerShape(18.dp),
                    )
                    .border(
                        width = 1.dp,
                        color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.08f),
                        shape = RoundedCornerShape(18.dp),
                    ),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    rendererIcon(renderer),
                    contentDescription = null,
                    tint = if (isSelected) selectedAccent else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    rendererDisplayName(renderer),
                    style = MaterialTheme.typography.headlineSmall,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    rendererDescriptor(renderer),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                membershipLabel?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.labelLarge,
                        color = if (isSelected) selectedAccent else MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            if (isSelected) {
                Row(
                    modifier = Modifier
                        .background(selectedAccent, RoundedCornerShape(99.dp))
                        .padding(horizontal = 14.dp, vertical = 10.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(
                        modifier = Modifier
                            .size(12.dp)
                            .background(Color(0xFF1B140D), CircleShape)
                    )
                    Text(
                        "Active",
                        color = Color(0xFF1B140D),
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                }
            } else {
                Text(
                    "Tap to switch",
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            QuickAddRendererMenu(
                targetRenderer = renderer,
                candidates = quickAddCandidates,
                isBusy = isBusy,
                selectedAccent = selectedAccent,
                onQuickAddRenderer = onQuickAddRenderer,
            )
        }
    }
}

@Composable
private fun QuickAddRendererMenu(
    targetRenderer: RendererDto,
    candidates: List<RendererDto>,
    isBusy: Boolean,
    selectedAccent: Color,
    onQuickAddRenderer: (String, String) -> Unit,
) {
    if (candidates.isEmpty()) {
        return
    }
    var expanded by remember { mutableStateOf(false) }
    Box {
        IconButton(
            onClick = { expanded = true },
            enabled = !isBusy,
        ) {
            Icon(
                Icons.Rounded.Add,
                contentDescription = "Add renderer",
                tint = selectedAccent,
            )
        }
        DropdownMenu(
            expanded = expanded,
            onDismissRequest = { expanded = false },
        ) {
            candidates.forEach { candidate ->
                DropdownMenuItem(
                    text = {
                        Column {
                            Text(
                                rendererDisplayName(candidate),
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                            Text(
                                rendererDescriptor(candidate),
                                style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    },
                    leadingIcon = {
                        Icon(
                            rendererIcon(candidate),
                            contentDescription = null,
                            tint = rendererHealthColor(candidate),
                        )
                    },
                    onClick = {
                        expanded = false
                        onQuickAddRenderer(targetRenderer.location, candidate.location)
                    },
                )
            }
        }
    }
}

@Composable
private fun RendererSectionHeader(
    title: String,
    meta: String,
    modifier: Modifier = Modifier,
) {
    Row(
        modifier = modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            title,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            meta,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

private fun rendererDescriptor(renderer: RendererDto): String {
    if (renderer.kind == "group") {
        val memberCount = renderer.group?.memberCount ?: renderer.group?.members?.size ?: 0
        return if (memberCount > 0) {
            "$memberCount renderers"
        } else {
            "Renderer group"
        }
    }
    if (renderer.kind == "android_local") {
        return "Local playback on this Android device"
    }
    val displayName = rendererDisplayName(renderer)
    val parts = listOfNotNull(
        renderer.manufacturer?.takeIf { it.isNotBlank() },
        renderer.modelName?.takeIf { it.isNotBlank() && it != displayName },
        renderer.kind.takeIf { it.isNotBlank() }?.uppercase(),
    )
    return parts.joinToString(" · ").ifBlank { "Renderer" }
}

private fun rendererGroupNames(renderer: RendererDto, groups: List<RendererDto>): List<String> =
    groups
        .filter { group ->
            group.group?.members.orEmpty().any { member ->
                member.rendererLocation == renderer.location
            }
        }
        .map(::rendererDisplayName)

private fun rendererMembershipLabel(groupNames: List<String>): String? =
    when (groupNames.size) {
        0 -> null
        1 -> "In ${groupNames.first()}"
        else -> "In ${groupNames.size} groups"
    }

private fun rendererHealthLabel(renderer: RendererDto?): String =
    when {
        renderer == null -> "Missing"
        renderer.health?.reachable == false || renderer.health?.lastError != null -> "Issue"
        renderer.health?.reachable == true -> "Online"
        else -> "Saved"
    }

@Composable
private fun rendererHealthColor(renderer: RendererDto?): Color =
    when {
        renderer == null -> MaterialTheme.colorScheme.error
        renderer.health?.reachable == false || renderer.health?.lastError != null -> {
            MaterialTheme.colorScheme.error
        }
        renderer.health?.reachable == true -> Color(0xFF70D49A)
        else -> MaterialTheme.colorScheme.onSurfaceVariant
    }

private fun rendererIcon(renderer: RendererDto?) =
    when (renderer?.kind) {
        "android_local" -> Icons.Rounded.PhoneAndroid
        "group" -> Icons.AutoMirrored.Rounded.QueueMusic
        else -> Icons.Rounded.Speaker
    }

private fun rendererDisplayName(renderer: RendererDto): String {
    val name = renderer.name.trim()
    val model = renderer.modelName?.trim().orEmpty()
    val host = rendererLocationHost(renderer.location)
    val nameLooksLikeFallback = name.isBlank() ||
        name == renderer.location ||
        looksLikeIpAddress(name) ||
        host?.equals(name, ignoreCase = true) == true

    return when {
        !nameLooksLikeFallback -> name
        model.isNotBlank() -> model
        !host.isNullOrBlank() -> host
        else -> "Renderer"
    }
}

private fun rendererLocationHost(location: String): String? {
    val remainder = location.substringAfter("://", location)
    val authority = remainder.substringBefore('/').trim()
    if (authority.isBlank()) {
        return null
    }
    if (authority.startsWith('[')) {
        val closing = authority.indexOf(']')
        if (closing > 1) {
            return authority.substring(1, closing)
        }
    }
    return authority.substringBefore(':').takeIf { it.isNotBlank() }
}

private fun looksLikeIpAddress(value: String): Boolean {
    val trimmed = value.trim()
    return Regex("""^\d{1,3}(\.\d{1,3}){3}$""").matches(trimmed) ||
        (trimmed.contains(':') && trimmed.all { it.isDigit() || it.lowercaseChar() in 'a'..'f' || it == ':' })
}

@Composable
private fun AlbumRow(
    baseUrl: String,
    album: AlbumSummaryDto,
    onOpenAlbum: () -> Unit,
    onOpenArtist: (() -> Unit)? = null,
    onPlayAlbum: () -> Unit,
    onLikeAlbum: () -> Unit,
    onAppendAlbum: () -> Unit,
    onPlayNextAlbum: () -> Unit,
    highlightQuery: String? = null,
) {
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onOpenAlbum)
                .padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ArtworkSquare(
                url = resolveUrl(baseUrl, album.artworkUrl),
                modifier = Modifier.size(76.dp),
                fallbackText = album.title,
            )
            Column(modifier = Modifier.weight(1f)) {
                HighlightedText(
                    text = album.title,
                    query = highlightQuery,
                    color = MaterialTheme.colorScheme.onSurface,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(Modifier.height(2.dp))
                HighlightedText(
                    text = album.artist,
                    query = highlightQuery,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(Modifier.height(2.dp))
                Text(
                    "${album.trackCount} tracks",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.secondary,
                )
                Spacer(Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    OutlinedButton(onClick = onPlayAlbum) { Text("Play") }
                    TextButton(
                        enabled = !album.likedByClient,
                        onClick = onLikeAlbum,
                    ) {
                        Text("🦥 ${album.likeCount}")
                    }
                    onOpenArtist?.let { TextButton(onClick = it) { Text("Artist") } }
                    OutlinedIconButton(onClick = onPlayNextAlbum) {
                        Icon(Icons.Rounded.QueuePlayNext, contentDescription = "Play album next")
                    }
                    OutlinedIconButton(onClick = onAppendAlbum) {
                        Icon(Icons.Rounded.AddToQueue, contentDescription = "Add album to queue")
                    }
                }
            }
        }
    }
}

@Composable
private fun ArtistRow(
    baseUrl: String,
    artist: ArtistSummaryDto,
    onOpenArtist: () -> Unit,
    onOpenAlbum: () -> Unit,
    highlightQuery: String? = null,
) {
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onOpenArtist)
                .padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ArtworkSquare(
                url = resolveUrl(baseUrl, artist.artworkUrl),
                modifier = Modifier.size(76.dp),
                fallbackText = artist.name,
            )
            Column(modifier = Modifier.weight(1f)) {
                HighlightedText(
                    text = artist.name,
                    query = highlightQuery,
                    color = MaterialTheme.colorScheme.onSurface,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Spacer(Modifier.height(2.dp))
                Text(
                    "${artist.albumCount} albums · ${artist.trackCount} tracks",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.secondary,
                )
                Spacer(Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    OutlinedButton(onClick = onOpenArtist) { Text("View") }
                    TextButton(onClick = onOpenAlbum) { Text("Open First Album") }
                }
            }
        }
    }
}

@Composable
private fun ArtistDetailScreen(
    baseUrl: String,
    artist: ArtistDetailDto,
    onBack: () -> Unit,
    onOpenAlbum: (String) -> Unit,
    onPlayAlbum: (String) -> Unit,
    onLikeAlbum: (String) -> Unit,
    onAppendAlbum: (String) -> Unit,
    onPlayNextAlbum: (String) -> Unit,
) {
    LazyColumn(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            ElevatedPanel {
                TextButton(onClick = onBack) { Text("Back to library") }
                Spacer(Modifier.height(4.dp))
                ArtworkSquare(
                    url = resolveUrl(baseUrl, artist.artworkUrl),
                    modifier = Modifier.fillMaxWidth(),
                    fallbackText = artist.name,
                )
                Spacer(Modifier.height(14.dp))
                Text(artist.name, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
                Text(
                    "${artist.albumCount} albums · ${artist.trackCount} tracks",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.secondary,
                )
            }
        }
        item {
            Text("Albums", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        }
        items(artist.albums, key = { it.id }) { album ->
            AlbumRow(
                baseUrl = baseUrl,
                album = album,
                onOpenAlbum = { onOpenAlbum(album.id) },
                onPlayAlbum = { onPlayAlbum(album.id) },
                onLikeAlbum = { onLikeAlbum(album.id) },
                onAppendAlbum = { onAppendAlbum(album.id) },
                onPlayNextAlbum = { onPlayNextAlbum(album.id) },
                highlightQuery = null,
            )
        }
    }
}

@Composable
private fun AlbumDetailScreen(
    baseUrl: String,
    album: AlbumDetailDto,
    libraryAlbums: List<AlbumSummaryDto>,
    recommendations: List<AlbumRecommendationDto>,
    onBack: () -> Unit,
    backLabel: String,
    onOpenArtist: (() -> Unit)?,
    onOpenAlbum: (String) -> Unit,
    onOpenArtworkPicker: () -> Unit,
    onPlayAlbum: () -> Unit,
    onLikeAlbum: () -> Unit,
    onAppendAlbum: () -> Unit,
    onPlayNextAlbum: () -> Unit,
    onPlayTrack: (String) -> Unit,
    onLikeTrack: (String) -> Unit,
    onAppendTrack: (String) -> Unit,
    onPlayNextTrack: (String) -> Unit,
    onOpenArtistByName: (String) -> Unit,
    canPlayTidalAlbum: Boolean,
    onPlayTidalAlbum: (TidalAlbumDto) -> Unit,
    onAppendTidalAlbum: (TidalAlbumDto) -> Unit,
    onPlayNextTidalAlbum: (TidalAlbumDto) -> Unit,
) {
    val totalDurationLabel = albumTotalDurationLabel(album)
    val secondaryMeta = listOfNotNull(
        albumFormatLabel(album.tracks.firstOrNull()?.mimeType),
        totalDurationLabel,
    ).joinToString(" · ")

    LazyColumn(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            ElevatedPanel {
                Row(modifier = Modifier.fillMaxWidth()) {
                    TextButton(onClick = onBack) { Text(backLabel) }
                }
                Spacer(Modifier.height(4.dp))
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    val hasArtwork = album.artworkUrl.isNotBlank()
                    Column(
                        modifier = Modifier
                            .fillMaxWidth(0.8f)
                            .then(
                                if (!hasArtwork) {
                                    Modifier.clickable(onClick = onOpenArtworkPicker)
                                } else {
                                    Modifier
                                }
                            ),
                        horizontalAlignment = Alignment.CenterHorizontally,
                    ) {
                        ArtworkSquare(
                            url = resolveUrl(baseUrl, album.artworkUrl),
                            modifier = Modifier.fillMaxWidth(),
                            fallbackText = album.title,
                            contentHeight = 280.dp,
                        )
                        if (!hasArtwork) {
                            Spacer(Modifier.height(8.dp))
                            Text(
                                "Tap to find artwork",
                                style = MaterialTheme.typography.bodyMedium,
                                color = MaterialTheme.colorScheme.secondary,
                            )
                        }
                    }
                    Spacer(Modifier.height(18.dp))
                    Text(
                        text = album.title,
                        style = MaterialTheme.typography.displaySmall,
                        fontStyle = FontStyle.Italic,
                        fontWeight = FontWeight.Light,
                        textAlign = TextAlign.Center,
                    )
                    Spacer(Modifier.height(6.dp))
                    Text(
                        text = listOfNotNull(
                            album.artist.takeIf { it.isNotBlank() },
                            album.metadata.releaseDate?.takeIf { it.isNotBlank() },
                            "${album.trackCount} tracks",
                        ).joinToString(" · "),
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = TextAlign.Center,
                    )
                    if (secondaryMeta.isNotBlank()) {
                        Spacer(Modifier.height(4.dp))
                        Text(
                            text = secondaryMeta,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.secondary,
                            textAlign = TextAlign.Center,
                        )
                    }
                    Spacer(Modifier.height(18.dp))
                    Button(onClick = onPlayAlbum, modifier = Modifier.fillMaxWidth(0.58f)) {
                        Text("Play Album")
                    }
                    Spacer(Modifier.height(8.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        OutlinedButton(
                            enabled = !album.likedByClient,
                            onClick = onLikeAlbum,
                        ) {
                            Text("🦥 ${album.likeCount}")
                        }
                        onOpenArtist?.let {
                            OutlinedButton(onClick = it) {
                                Text("Artist")
                            }
                        }
                        OutlinedIconButton(onClick = onPlayNextAlbum) {
                            Icon(Icons.Rounded.QueuePlayNext, contentDescription = "Play album next")
                        }
                        OutlinedIconButton(onClick = onAppendAlbum) {
                            Icon(Icons.Rounded.AddToQueue, contentDescription = "Add album to queue")
                        }
                    }
                }
            }
        }
        items(album.tracks, key = { it.id }) { track ->
            AlbumTrackRow(
                track = track,
                onPlayTrack = { onPlayTrack(track.id) },
                onLikeTrack = { onLikeTrack(track.id) },
                onAppendTrack = { onAppendTrack(track.id) },
                onPlayNextTrack = { onPlayNextTrack(track.id) },
                onOpenArtist = { onOpenArtistByName(track.artist) },
            )
        }
        if (recommendations.isNotEmpty()) {
            item {
                Text(
                    "Recommended from this album",
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                )
            }
            items(recommendations, key = { it.recommendationKey }) { recommendation ->
                val localAlbum = findLibraryAlbumForRecommendation(recommendation, libraryAlbums)
                AlbumRecommendationRow(
                    baseUrl = baseUrl,
                    recommendation = recommendation,
                    localAlbum = localAlbum,
                    canPlayTidalAlbum = canPlayTidalAlbum,
                    onPlayTidalAlbum = onPlayTidalAlbum,
                    onAppendTidalAlbum = onAppendTidalAlbum,
                    onPlayNextTidalAlbum = onPlayNextTidalAlbum,
                    onOpenAlbum = { localAlbum?.id?.let(onOpenAlbum) },
                )
            }
        }
    }
}

@Composable
private fun AlbumRecommendationRow(
    baseUrl: String,
    recommendation: AlbumRecommendationDto,
    localAlbum: AlbumSummaryDto?,
    supportingText: String? = null,
    canPlayTidalAlbum: Boolean,
    onPlayTidalAlbum: (TidalAlbumDto) -> Unit,
    onAppendTidalAlbum: (TidalAlbumDto) -> Unit,
    onPlayNextTidalAlbum: (TidalAlbumDto) -> Unit,
    onOpenAlbum: () -> Unit,
) {
    val uriHandler = LocalUriHandler.current
    val tidalUrl = recommendation.tidalUrl?.takeIf(::isWebUrl)
    val tidalAlbum = tidalAlbumFromRecommendation(recommendation)
    val hasUnplayableTidalUrl = tidalUrl != null && tidalAlbum == null
    val externalUrl = tidalUrl ?: recommendation.externalUrl?.takeIf(::isWebUrl)
    val openDescription = if (tidalUrl != null) "Open in TIDAL" else "Open recommendation"
    var isContextExpanded by remember(recommendation.recommendationKey, supportingText) {
        mutableStateOf(false)
    }
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .then(
                    if (localAlbum != null) {
                        Modifier.clickable(onClick = onOpenAlbum)
                    } else {
                        Modifier
                    }
                )
                .padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ArtworkSquare(
                url = recommendationArtworkUrl(baseUrl, recommendation),
                modifier = Modifier.size(72.dp),
                fallbackText = recommendation.suggestedTitle,
                contentHeight = 72.dp,
            )
            Column(modifier = Modifier.weight(1f)) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.Top,
                ) {
                    Column(modifier = Modifier.weight(1f)) {
                        Text(
                            recommendation.suggestedTitle,
                            style = MaterialTheme.typography.titleMedium,
                            fontWeight = FontWeight.SemiBold,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Text(
                            recommendation.suggestedArtist,
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    externalUrl?.let { url ->
                        IconButton(onClick = { uriHandler.openUri(url) }) {
                            Icon(Icons.AutoMirrored.Rounded.OpenInNew, contentDescription = openDescription)
                        }
                    }
                }
                val meta = recommendationMetaLabel(
                    recommendation = recommendation,
                    isInLibrary = localAlbum != null,
                    hasUnplayableTidalUrl = hasUnplayableTidalUrl,
                )
                if (meta.isNotBlank()) {
                    Spacer(Modifier.height(4.dp))
                    Text(
                        meta,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.secondary,
                    )
                }
                val recommendationContext = supportingText
                    ?: recommendation.rationale?.takeIf { it.isNotBlank() }
                recommendationContext?.let { text ->
                    Spacer(Modifier.height(6.dp))
                    Text(
                        text,
                        modifier = Modifier.clickable {
                            isContextExpanded = !isContextExpanded
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = if (isContextExpanded) Int.MAX_VALUE else 3,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                tidalAlbum?.let { album ->
                    Spacer(Modifier.height(8.dp))
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        FilledIconButton(onClick = { onPlayTidalAlbum(album) }, enabled = canPlayTidalAlbum) {
                            Icon(Icons.Rounded.PlayArrow, contentDescription = "Play TIDAL recommendation")
                        }
                        OutlinedIconButton(onClick = { onPlayNextTidalAlbum(album) }, enabled = canPlayTidalAlbum) {
                            Icon(Icons.Rounded.QueuePlayNext, contentDescription = "Play TIDAL recommendation next")
                        }
                        OutlinedIconButton(onClick = { onAppendTidalAlbum(album) }, enabled = canPlayTidalAlbum) {
                            Icon(Icons.Rounded.AddToQueue, contentDescription = "Add TIDAL recommendation to queue")
                        }
                    }
                }
            }
        }
    }
}

private fun recommendationHomeReason(
    recommendation: AlbumRecommendationDto,
    albums: List<AlbumSummaryDto>,
): String? {
    val seedAlbum = albums.firstOrNull { it.id == recommendation.seedAlbumId } ?: return null
    val reason = "Because you have ${seedAlbum.title} by ${seedAlbum.artist}"
    val rationale = recommendation.rationale?.takeIf { it.isNotBlank() }
    return listOfNotNull(reason, rationale).joinToString("\n")
}

private fun recommendationArtworkUrl(
    baseUrl: String,
    recommendation: AlbumRecommendationDto,
): String? =
    resolveUrl(baseUrl, recommendation.artworkUrl)
        ?: recommendation.suggestedMusicbrainzReleaseId
            ?.takeIf { it.isNotBlank() }
            ?.let { "https://coverartarchive.org/release/$it/front-250" }
        ?: recommendation.suggestedMusicbrainzReleaseGroupId
            ?.takeIf { it.isNotBlank() }
            ?.let { "https://coverartarchive.org/release-group/$it/front-250" }

private fun tidalAlbumFromRecommendation(recommendation: AlbumRecommendationDto): TidalAlbumDto? {
    val albumId = tidalAlbumIdFromUrl(recommendation.tidalUrl) ?: return null
    return TidalAlbumDto(
        albumId = albumId,
        title = recommendation.suggestedTitle,
        artist = recommendation.suggestedArtist,
        artworkUrl = recommendation.artworkUrl,
    )
}

private fun tidalAlbumIdFromUrl(value: String?): String? {
    val url = value?.trim()?.takeIf { it.isNotBlank() } ?: return null
    val match = Regex("""^https?://(?:www\.)?(?:listen\.)?tidal\.com/(?:browse/)?album/([0-9]+)(?:[/?#].*)?$""")
        .matchEntire(url)
    return match?.groupValues?.getOrNull(1)
}

@Composable
private fun AlbumArtworkPickerSheet(
    album: AlbumDetailDto?,
    candidates: List<AlbumArtworkCandidateDto>,
    isSearching: Boolean,
    isApplying: Boolean,
    errorMessage: String?,
    onSelectCandidate: (String) -> Unit,
) {
    Column(modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp)) {
        Text("Find artwork", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(6.dp))
        Text(
            album?.let { "Search results for ${it.artist} · ${it.title}" }
                ?: "Search results for this album.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(16.dp))
        when {
            isSearching -> {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    CircularProgressIndicator(modifier = Modifier.size(22.dp), strokeWidth = 2.dp)
                    Text("Searching MusicBrainz and Cover Art Archive…")
                }
            }
            !errorMessage.isNullOrBlank() -> {
                Text(errorMessage, color = MaterialTheme.colorScheme.error)
            }
            candidates.isEmpty() -> {
                Text(
                    "No matching artwork candidates were found for this album.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            else -> {
                candidates.forEach { candidate ->
                    Card(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(bottom = 12.dp)
                            .clickable(enabled = !isApplying) { onSelectCandidate(candidate.releaseId) },
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
                        shape = RoundedCornerShape(22.dp),
                    ) {
                        Row(
                            modifier = Modifier.padding(12.dp),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            AsyncImage(
                                model = candidate.thumbnailUrl,
                                contentDescription = null,
                                modifier = Modifier
                                    .size(84.dp)
                                    .background(
                                        MaterialTheme.colorScheme.surface,
                                        RoundedCornerShape(18.dp),
                                    ),
                            )
                            Column(modifier = Modifier.weight(1f)) {
                                Text(
                                    candidate.title,
                                    style = MaterialTheme.typography.titleMedium,
                                    fontWeight = FontWeight.SemiBold,
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                Text(
                                    listOfNotNull(
                                        candidate.artist.takeIf { it.isNotBlank() },
                                        candidate.date,
                                        candidate.country,
                                    ).joinToString(" · "),
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                    maxLines = 2,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                Spacer(Modifier.height(6.dp))
                                Text(
                                    "Match score ${candidate.score}",
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.secondary,
                                )
                            }
                        }
                    }
                }
            }
        }
        if (isApplying) {
            Spacer(Modifier.height(8.dp))
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
                Text("Saving artwork…")
            }
        }
        Spacer(Modifier.height(20.dp))
    }
}

@Composable
private fun AlbumTrackRow(
    track: TrackSummaryDto,
    onPlayTrack: () -> Unit,
    onLikeTrack: () -> Unit,
    onAppendTrack: () -> Unit,
    onPlayNextTrack: () -> Unit,
    onOpenArtist: (() -> Unit)? = null,
) {
    var menuExpanded by remember { mutableStateOf(false) }
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(20.dp),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onPlayTrack)
                .padding(horizontal = 14.dp, vertical = 10.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = trackListNumberLabel(track),
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.secondary,
                )
                Text(
                    text = track.title,
                    modifier = Modifier.weight(1f),
                    style = MaterialTheme.typography.titleMedium,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                track.durationSeconds?.let { duration ->
                    Text(
                        text = formatDuration(duration),
                        style = MaterialTheme.typography.titleSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Box {
                    IconButton(onClick = { menuExpanded = true }) {
                        Icon(
                            Icons.Rounded.MoreVert,
                            contentDescription = "Track actions",
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    DropdownMenu(
                        expanded = menuExpanded,
                        onDismissRequest = { menuExpanded = false },
                    ) {
                        onOpenArtist?.let {
                            DropdownMenuItem(
                                text = { Text("Open Artist") },
                                onClick = {
                                    menuExpanded = false
                                    it()
                                },
                            )
                        }
                        DropdownMenuItem(
                            text = { Text("Play Next") },
                            onClick = {
                                menuExpanded = false
                                onPlayNextTrack()
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("Add Queue") },
                            onClick = {
                                menuExpanded = false
                                onAppendTrack()
                            },
                        )
                    }
                }
                TextButton(
                    enabled = !track.likedByClient,
                    onClick = onLikeTrack,
                ) {
                    Text("🦥 ${track.likeCount}")
                }
            }
            if (track.discNumber != null || track.artist.isNotBlank()) {
                Spacer(Modifier.height(4.dp))
                Text(
                    text = buildAlbumTrackMeta(track),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun TrackRow(
    baseUrl: String,
    track: TrackSummaryDto,
    onPlayTrack: () -> Unit,
    onLikeTrack: () -> Unit,
    onAppendTrack: () -> Unit,
    onPlayNextTrack: () -> Unit,
    onOpenArtist: (() -> Unit)?,
    onOpenAlbum: (() -> Unit)?,
    highlightQuery: String? = null,
) {
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(14.dp),
            horizontalArrangement = Arrangement.spacedBy(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ArtworkSquare(
                url = resolveUrl(baseUrl, track.artworkUrl),
                modifier = Modifier.size(92.dp),
                fallbackText = track.album.ifBlank { track.title },
            )
            Column(modifier = Modifier.weight(1f)) {
                HighlightedText(
                    text = track.title,
                    query = highlightQuery,
                    color = MaterialTheme.colorScheme.onSurface,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                HighlightedText(
                    text = listOfNotNull(track.artist, track.album.takeIf { it.isNotBlank() }).joinToString(" · "),
                    query = highlightQuery,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    trackOrderLabel(track),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.secondary,
                )
                Spacer(Modifier.height(10.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Button(onClick = onPlayTrack) {
                        Text("Play")
                    }
                    onOpenArtist?.let {
                        TextButton(onClick = it) { Text("Artist") }
                    }
                    onOpenAlbum?.let {
                        TextButton(onClick = it) { Text("Album") }
                    }
                }
                Spacer(Modifier.height(6.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    TextButton(
                        enabled = !track.likedByClient,
                        onClick = onLikeTrack,
                    ) {
                        Text("🦥 ${track.likeCount}")
                    }
                    OutlinedIconButton(onClick = onPlayNextTrack) {
                        Icon(Icons.Rounded.QueuePlayNext, contentDescription = "Play track next")
                    }
                    OutlinedIconButton(onClick = onAppendTrack) {
                        Icon(Icons.Rounded.AddToQueue, contentDescription = "Add track to queue")
                    }
                }
            }
        }
    }
}

@Composable
private fun QueueEntryRow(
    baseUrl: String,
    entry: QueueEntryDto,
    track: TrackSummaryDto?,
    isCurrent: Boolean,
    accentColor: Color,
    canMoveUp: Boolean,
    canMoveDown: Boolean,
    canRemove: Boolean,
    onMoveUp: () -> Unit,
    onMoveDown: () -> Unit,
    onRemove: () -> Unit,
) {
    var menuExpanded by remember(entry.id) { mutableStateOf(false) }
    val title = entry.title ?: track?.title ?: "Unknown Track"
    val subtitle = listOfNotNull(
        entry.artist ?: track?.artist,
        entry.album ?: track?.album,
    ).joinToString(" · ")
    val duration = entry.durationSeconds ?: track?.durationSeconds

    Card(
        colors = CardDefaults.cardColors(
            containerColor = if (isCurrent) MaterialTheme.colorScheme.surface else Color.Transparent,
        ),
        border = if (isCurrent) {
            BorderStroke(1.dp, MaterialTheme.colorScheme.outline.copy(alpha = 0.22f))
        } else {
            null
        },
        shape = RoundedCornerShape(24.dp),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = if (isCurrent) 14.dp else 6.dp, vertical = if (isCurrent) 12.dp else 8.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            QueueHandleDots(
                tint = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.55f),
            )
            ArtworkSquare(
                url = resolveUrl(baseUrl, track?.artworkUrl),
                modifier = Modifier.size(if (isCurrent) 64.dp else 60.dp),
                fallbackText = track?.album?.ifBlank { title } ?: title,
                contentHeight = if (isCurrent) 64.dp else 60.dp,
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = title,
                    style = if (isCurrent) MaterialTheme.typography.headlineSmall else MaterialTheme.typography.titleLarge,
                    fontWeight = FontWeight.SemiBold,
                    color = if (isCurrent) accentColor else MaterialTheme.colorScheme.onSurface,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (subtitle.isNotBlank()) {
                    Text(
                        text = subtitle,
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            duration?.let {
                Text(
                    text = formatDuration(it),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (!isCurrent) {
                Box {
                    IconButton(onClick = { menuExpanded = true }) {
                        Icon(
                            Icons.Rounded.MoreVert,
                            contentDescription = "Queue entry actions",
                            tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    DropdownMenu(
                        expanded = menuExpanded,
                        onDismissRequest = { menuExpanded = false },
                    ) {
                        DropdownMenuItem(
                            text = { Text("Move up") },
                            enabled = canMoveUp,
                            onClick = {
                                menuExpanded = false
                                onMoveUp()
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("Move down") },
                            enabled = canMoveDown,
                            onClick = {
                                menuExpanded = false
                                onMoveDown()
                            },
                        )
                        DropdownMenuItem(
                            text = { Text("Remove") },
                            enabled = canRemove,
                            onClick = {
                                menuExpanded = false
                                onRemove()
                            },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun QueueSectionHeader(title: String) {
    Text(
        text = title.uppercase(),
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        fontWeight = FontWeight.Medium,
    )
}

@Composable
private fun QueueHandleDots(
    tint: Color,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier.padding(horizontal = 2.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        repeat(3) {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                repeat(2) {
                    Box(
                        modifier = Modifier
                            .size(4.dp)
                            .background(tint, CircleShape)
                    )
                }
            }
        }
    }
}

private fun queueSummaryLine(
    entries: List<QueueEntryDto>,
    currentEntry: QueueEntryDto?,
): String {
    val remainingEntries = entries.filter { entry ->
        entry.id == currentEntry?.id || isUpcomingQueueEntryStatus(entry.entryStatus)
    }
    val trackCount = remainingEntries.size
    val trackCountLabel = "${trackCount} ${if (trackCount == 1) "track" else "tracks"}"
    val remainingSeconds = if (currentEntry != null) {
        remainingEntries.mapNotNull { it.durationSeconds }.sum()
    } else {
        remainingEntries.mapNotNull { it.durationSeconds }.sum()
    }

    if (remainingSeconds <= 0L) {
        return trackCountLabel
    }

    val totalMinutes = remainingSeconds / 60
    val remainingLabel = if (totalMinutes >= 60) {
        val hours = totalMinutes / 60
        val minutes = totalMinutes % 60
        if (minutes == 0L) {
            "$hours hr remaining"
        } else {
            "$hours hr $minutes min remaining"
        }
    } else {
        "${totalMinutes.coerceAtLeast(1)} min remaining"
    }

    return "$trackCountLabel · $remainingLabel"
}

private fun isCurrentQueueEntryStatus(status: String?): Boolean =
    status.equals("playing", ignoreCase = true)

private fun isUpcomingQueueEntryStatus(status: String?): Boolean =
    when (status?.trim()?.lowercase()) {
        null, "", "pending" -> true
        "queued" -> true
        else -> false
    }

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
        state.nowPlaying?.currentTrack != null ||
        state.nowPlaying?.session?.currentTrackUri?.isNotBlank() == true

private fun playbackTitle(state: MusicdUiState): String =
    state.nowPlaying?.currentTrack?.title
        ?: state.nowPlaying?.session?.title?.takeIf { it.isNotBlank() }
        ?: currentPlaybackQueueEntry(state)?.title?.takeIf { it.isNotBlank() }
        ?: "Nothing playing"

private fun playbackSubtitle(state: MusicdUiState): String {
    val track = state.nowPlaying?.currentTrack
    if (track != null) {
        return listOfNotNull(track.artist, track.album).joinToString(" · ")
    }
    val session = state.nowPlaying?.session
    val queueEntry = currentPlaybackQueueEntry(state)
    return listOfNotNull(
        session?.artist?.takeIf { it.isNotBlank() },
        session?.album?.takeIf { it.isNotBlank() },
        queueEntry?.artist?.takeIf { session?.artist.isNullOrBlank() && it.isNotBlank() },
        queueEntry?.album?.takeIf { session?.album.isNullOrBlank() && it.isNotBlank() },
    ).joinToString(" · ").ifBlank {
        humanizeTransportState(session?.transportState)
    }
}

private fun currentPlaybackQueueEntry(state: MusicdUiState): QueueEntryDto? {
    val queue = state.queue ?: return null
    val sessionEntryId = currentPlaybackQueueEntryId(state)
    if (sessionEntryId != null) {
        return queue.entries.firstOrNull { entry -> entry.id == sessionEntryId }
    }
    val currentEntryId = queue.currentEntryId
    return queue.entries.firstOrNull { entry ->
        entry.id == currentEntryId && isCurrentQueueEntryStatus(entry.entryStatus)
    }
}

private fun currentPlaybackQueueEntryId(state: MusicdUiState): Long? =
    state.nowPlaying?.session?.queueEntryId
        ?: state.queue?.session?.queueEntryId
        ?: state.queue?.currentEntryId

private fun humanizeTransportState(state: String?): String =
    when (state?.trim()?.uppercase()) {
        null, "", "IDLE" -> "Idle"
        "PLAYING" -> "Playing"
        "PAUSED_PLAYBACK" -> "Paused"
        "STOPPED" -> "Stopped"
        "TRANSITIONING" -> "Changing track"
        "NO_MEDIA_PRESENT" -> "No media"
        else -> state
            .trim()
            .lowercase()
            .replace('_', ' ')
            .replaceFirstChar { if (it.isLowerCase()) it.titlecase() else it.toString() }
    }

@Composable
private fun NowPlayingContent(
    state: MusicdUiState,
    onPlay: () -> Unit,
    onPause: () -> Unit,
    onStop: () -> Unit,
    onNext: () -> Unit,
    onPrevious: () -> Unit,
) {
    val canNavigatePlayback = canRequestPlaybackNavigation(state)
    val canResumePlayback = canRequestPlaybackResume(state)
    val track = state.nowPlaying?.currentTrack
    val queueEntry = currentPlaybackQueueEntry(state)
    Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        ArtworkSquare(
            url = resolveUrl(state.baseUrl, track?.artworkUrl),
            modifier = Modifier.weight(0.34f),
            fallbackText = track?.album
                ?.ifBlank { track.title }
                ?: queueEntry?.album?.ifBlank { queueEntry.title.orEmpty() }
                ?: queueEntry?.title.orEmpty(),
        )
        Column(modifier = Modifier.weight(0.66f)) {
            Text(playbackTitle(state), style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Text(
                playbackSubtitle(state).ifBlank { "Choose a renderer and start playback." },
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(10.dp))
            Text(
                humanizeTransportState(state.nowPlaying?.session?.transportState),
                color = MaterialTheme.colorScheme.secondary,
                style = MaterialTheme.typography.labelLarge,
            )
            state.nowPlaying?.session?.let { session ->
                val progress = sessionProgress(session)
                if (progress != null) {
                    Spacer(Modifier.height(10.dp))
                    LinearProgressIndicator(
                        progress = { progress },
                        modifier = Modifier.fillMaxWidth(),
                    )
                    PlaybackTimeRow(session = session)
                }
            }
            Spacer(Modifier.height(14.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedIconButton(onClick = onPrevious, enabled = canNavigatePlayback) {
                    Icon(Icons.Rounded.SkipPrevious, contentDescription = "Previous")
                }
                FilledIconButton(onClick = onPlay, enabled = canResumePlayback) {
                    Icon(Icons.Rounded.PlayArrow, contentDescription = "Play")
                }
                OutlinedIconButton(onClick = onPause) {
                    Icon(Icons.Rounded.Pause, contentDescription = "Pause")
                }
                OutlinedIconButton(onClick = onStop) {
                    Icon(Icons.Rounded.Stop, contentDescription = "Stop")
                }
                OutlinedIconButton(onClick = onNext, enabled = canNavigatePlayback) {
                    Icon(Icons.Rounded.SkipNext, contentDescription = "Next")
                }
            }
        }
    }
}

@Composable
private fun PlaybackTimeRow(session: io.musicd.android.data.SessionDto) {
    val elapsed = session.positionSeconds?.let(::formatDuration) ?: "--:--"
    val total = session.durationSeconds?.let(::formatDuration) ?: "--:--"
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 4.dp, vertical = 4.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            elapsed,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            total,
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun ArtworkSquare(
    url: String?,
    modifier: Modifier = Modifier,
    fallbackText: String = "No Artwork",
    contentHeight: Dp = 120.dp,
) {
    Card(
        modifier = modifier,
        shape = RoundedCornerShape(22.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
    ) {
        if (!url.isNullOrBlank()) {
            AsyncImage(
                model = url,
                contentDescription = null,
                modifier = Modifier
                    .fillMaxWidth()
                    .height(contentHeight),
            )
        } else {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(contentHeight),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = placeholderLabel(fallbackText),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.padding(12.dp),
                )
            }
        }
    }
}

private fun placeholderLabel(text: String): String =
    text.trim().ifBlank { "No Artwork" }

private fun trackListNumberLabel(track: TrackSummaryDto): String =
    track.trackNumber?.toString() ?: "•"

private fun buildAlbumTrackMeta(track: TrackSummaryDto): String =
    listOfNotNull(
        track.discNumber?.takeIf { it > 1 }?.let { "Disc $it" },
        track.artist.takeIf { it.isNotBlank() },
    ).joinToString(" · ")

private fun albumFormatLabel(mimeType: String?): String? =
    when (mimeType?.lowercase()) {
        "audio/flac" -> "FLAC"
        "audio/mpeg", "audio/mp3" -> "MP3"
        "audio/aac" -> "AAC"
        "audio/ogg" -> "Ogg"
        "audio/wav", "audio/wave", "audio/x-wav" -> "WAV"
        "audio/mp4", "audio/x-m4a", "audio/alac" -> "MP4"
        else -> null
    }

private fun albumTotalDurationLabel(album: AlbumDetailDto): String? {
    val durations = album.tracks.mapNotNull { it.durationSeconds }
    if (durations.isEmpty()) {
        return null
    }
    return formatDuration(durations.sum())
}

private fun recommendationMetaLabel(
    recommendation: AlbumRecommendationDto,
    isInLibrary: Boolean,
    hasUnplayableTidalUrl: Boolean = false,
): String =
    listOfNotNull(
        "In library".takeIf { isInLibrary },
        "TIDAL link needs cleanup".takeIf { hasUnplayableTidalUrl },
        recommendation.confidence?.let(::confidenceLabel),
        recommendation.status.takeUnless { it.equals("suggested", ignoreCase = true) },
    ).joinToString(" · ")

private fun confidenceLabel(confidence: Double): String {
    val percentage = if (confidence <= 1.0) confidence * 100.0 else confidence
    return "%.0f%% match".format(percentage.coerceIn(0.0, 100.0))
}

private fun findLibraryAlbumForRecommendation(
    recommendation: AlbumRecommendationDto,
    albums: List<AlbumSummaryDto>,
): AlbumSummaryDto? {
    recommendation.suggestedMusicbrainzReleaseId?.takeIf { it.isNotBlank() }?.let { releaseId ->
        albums.firstOrNull { it.metadata?.musicbrainzReleaseId == releaseId }?.let { return it }
    }
    recommendation.suggestedMusicbrainzReleaseGroupId?.takeIf { it.isNotBlank() }?.let { releaseGroupId ->
        albums.firstOrNull { it.metadata?.musicbrainzReleaseGroupId == releaseGroupId }?.let { return it }
    }
    recommendation.artworkUrl?.let(::albumIdFromArtworkUrl)?.let { albumId ->
        albums.firstOrNull { it.id == albumId }?.let { return it }
    }

    val recommendedArtist = normalizeRecommendationMatchText(recommendation.suggestedArtist)
    val recommendedTitle = normalizeRecommendationMatchText(recommendation.suggestedTitle)
    return albums.firstOrNull {
        normalizeRecommendationMatchText(it.artist) == recommendedArtist &&
            normalizeRecommendationMatchText(it.title) == recommendedTitle
    }
}

private fun albumIdFromArtworkUrl(url: String): String? =
    url
        .substringBefore('?')
        .trimEnd('/')
        .substringAfterLast('/')
        .takeIf { url.contains("/artwork/album/") && it.isNotBlank() }

private fun normalizeRecommendationMatchText(value: String): String =
    value.trim().lowercase().replace(Regex("""\s+"""), " ")

@Composable
private fun HighlightedText(
    text: String,
    query: String?,
    color: Color,
    modifier: Modifier = Modifier,
    fontWeight: FontWeight? = null,
    maxLines: Int = Int.MAX_VALUE,
    overflow: TextOverflow = TextOverflow.Clip,
) {
    Text(
        text = buildHighlightedText(
            text = text,
            query = query,
            defaultColor = color,
            highlightColor = MaterialTheme.colorScheme.primary,
            baseFontWeight = fontWeight,
        ),
        modifier = modifier,
        maxLines = maxLines,
        overflow = overflow,
    )
}

private fun buildHighlightedText(
    text: String,
    query: String?,
    defaultColor: Color,
    highlightColor: Color,
    baseFontWeight: FontWeight?,
) = buildAnnotatedString {
    val normalizedText = text.lowercase()
    val tokens = queryTokens(query.orEmpty())
    if (text.isBlank() || tokens.isEmpty()) {
        pushStyle(SpanStyle(color = defaultColor, fontWeight = baseFontWeight))
        append(text)
        pop()
        return@buildAnnotatedString
    }

    val highlightMask = BooleanArray(text.length)
    tokens.forEach { token ->
        var startIndex = normalizedText.indexOf(token)
        while (startIndex >= 0) {
            val endIndex = (startIndex + token.length).coerceAtMost(highlightMask.size)
            for (index in startIndex until endIndex) {
                highlightMask[index] = true
            }
            startIndex = normalizedText.indexOf(token, startIndex + 1)
        }
    }

    var index = 0
    while (index < text.length) {
        val highlighted = highlightMask[index]
        val segmentStart = index
        while (index < text.length && highlightMask[index] == highlighted) {
            index += 1
        }
        pushStyle(
            SpanStyle(
                color = if (highlighted) highlightColor else defaultColor,
                fontWeight = if (highlighted) FontWeight.SemiBold else baseFontWeight,
            )
        )
        append(text.substring(segmentStart, index))
        pop()
    }
}

private fun trackOrderLabel(track: TrackSummaryDto): String {
    val disc = track.discNumber?.let { "Disc $it" }
    val number = track.trackNumber?.let { "Track $it" }
    val duration = track.durationSeconds?.let(::formatDuration)
    return listOfNotNull(disc, number, duration).joinToString(" · ").ifBlank { "Track" }
}

private fun formatDuration(totalSeconds: Long): String {
    val minutes = totalSeconds / 60
    val seconds = totalSeconds % 60
    return "%d:%02d".format(minutes, seconds)
}

private fun shouldShowMiniPlayer(state: MusicdUiState): Boolean =
    state.nowPlaying?.currentTrack != null ||
        !state.queue?.entries.isNullOrEmpty() ||
        state.nowPlaying?.session?.currentTrackUri?.isNotBlank() == true

private fun sessionProgress(session: io.musicd.android.data.SessionDto?): Float? {
    val position = session?.positionSeconds?.toFloat() ?: return null
    val duration = session.durationSeconds?.toFloat() ?: return null
    if (duration <= 0f) return null
    return (position / duration).coerceIn(0f, 1f)
}

private fun resolveUrl(baseUrl: String, path: String?): String? {
    if (path.isNullOrBlank()) {
        return null
    }
    if (path.startsWith("http://") || path.startsWith("https://")) {
        return path
    }
    return "${baseUrl.trimEnd('/')}/${path.trimStart('/')}"
}

private fun isWebUrl(value: String): Boolean =
    value.startsWith("http://") || value.startsWith("https://")

private fun serverLabel(serverName: String?, baseUrl: String): String =
    serverName?.takeIf { it.isNotBlank() } ?: if (baseUrl == "http://127.0.0.1:8788") {
        "Local companion"
    } else if (baseUrl.isBlank()) {
        "Set Server"
    } else {
        baseUrl.removePrefix("http://").removePrefix("https://")
    }

@Composable
private fun ElevatedPanel(content: @Composable ColumnScope.() -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(28.dp),
    ) {
        Column(modifier = Modifier.padding(18.dp), content = content)
    }
}

@Composable
private fun EmptyPanel(
    title: String,
    body: String,
) {
    ElevatedPanel {
        Text(title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(6.dp))
        Text(body, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun LocalCompanionLibraryPanel(onOpenLocalCompanion: () -> Unit) {
    ElevatedPanel {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Rounded.PhoneAndroid, contentDescription = null, modifier = Modifier.size(28.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text("Local companion library", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                Text(
                    "Open musicd Companion, add a music folder, run Scan music folders, then return here. The controller refreshes local tracks when it comes back into focus.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Spacer(Modifier.height(12.dp))
        Button(onClick = onOpenLocalCompanion, modifier = Modifier.fillMaxWidth()) {
            Text("Open musicd Companion")
        }
    }
}

@Composable
private fun LoadingPanel(message: String) {
    ElevatedPanel {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            CircularProgressIndicator(modifier = Modifier.size(22.dp), strokeWidth = 2.dp)
            Text(message, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

private fun currentTitle(tab: MusicdTab): String = when (tab) {
    MusicdTab.Home -> "Home"
    MusicdTab.Library -> "Library"
    MusicdTab.Radio -> "Radio"
    MusicdTab.Tidal -> "TIDAL"
    MusicdTab.Queue -> "Queue"
}
