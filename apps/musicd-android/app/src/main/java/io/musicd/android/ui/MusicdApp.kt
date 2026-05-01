package io.musicd.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
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
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Album
import androidx.compose.material.icons.rounded.Home
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.foundation.BorderStroke
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
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.font.FontWeight
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
import io.musicd.android.data.AlbumSummaryDto
import io.musicd.android.data.ArtistDetailDto
import io.musicd.android.data.ArtistSummaryDto
import io.musicd.android.data.QueueEntryDto
import io.musicd.android.data.RendererDto
import io.musicd.android.data.TrackSummaryDto
import java.time.LocalDate
import java.time.LocalTime
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
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
        if (!state.connected || state.selectedRendererLocation.isBlank() || state.isConnecting) {
            return@LaunchedEffect
        }
        if (!lifecycleState.isAtLeast(Lifecycle.State.STARTED)) {
            return@LaunchedEffect
        }
        while (true) {
            viewModel.refreshPlaybackState()
            delay(2_500)
        }
    }

    if (!state.connected) {
        ServerSetupScreen(
            serverInput = state.serverInput,
            serverName = state.serverName,
            isConnecting = state.isConnecting,
            errorMessage = state.errorMessage,
            onServerInputChange = viewModel::updateServerInput,
            onConnect = { viewModel.connect() },
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
        onOpenServerEditor = { viewModel.toggleServerEditor(true) },
        onDismissServerEditor = { viewModel.toggleServerEditor(false) },
        onConnect = { viewModel.connect() },
        onRetryConnection = viewModel::retryConnection,
        onDisconnectServer = viewModel::disconnectServer,
        onOpenRendererPicker = { viewModel.toggleRendererPicker(true) },
        onDismissRendererPicker = { viewModel.toggleRendererPicker(false) },
        onDismissError = viewModel::dismissError,
        onDismissWarning = viewModel::dismissWarning,
        onSelectRenderer = viewModel::selectRenderer,
        onDiscoverRenderers = viewModel::discoverRenderers,
        onPlay = viewModel::transportPlay,
        onPause = viewModel::transportPause,
        onStop = viewModel::transportStop,
        onNext = viewModel::transportNext,
        onPrevious = viewModel::transportPrevious,
        onSearchQueryChange = viewModel::updateSearchQuery,
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
    onOpenServerEditor: () -> Unit,
    onDismissServerEditor: () -> Unit,
    onConnect: () -> Unit,
    onRetryConnection: () -> Unit,
    onDisconnectServer: () -> Unit,
    onOpenRendererPicker: () -> Unit,
    onDismissRendererPicker: () -> Unit,
    onDismissError: () -> Unit,
    onDismissWarning: () -> Unit,
    onSelectRenderer: (String) -> Unit,
    onDiscoverRenderers: () -> Unit,
    onPlay: () -> Unit,
    onPause: () -> Unit,
    onStop: () -> Unit,
    onNext: () -> Unit,
    onPrevious: () -> Unit,
    onSearchQueryChange: (String) -> Unit,
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
                connectedBaseUrl = state.baseUrl,
                isConnecting = state.isConnecting,
                errorMessage = state.errorMessage,
                onServerInputChange = onServerInputChange,
                onConnect = onConnect,
                onRetry = onRetryConnection,
                onDisconnect = onDisconnectServer,
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
                onSelectRenderer = onSelectRenderer,
                onDiscoverRenderers = onDiscoverRenderers,
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
                    Column {
                        Text(currentTitle(state.selectedTab))
                        Text(
                            text = state.selectedRendererLocation.ifBlank { "No renderer selected" },
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
                    onClick = onOpenRendererPicker,
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
                    onAppendAlbum = onAppendAlbum,
                    onPlayNextAlbum = onPlayNextAlbum,
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
                    onPlayTrack = onPlayTrack,
                    onPlayAlbum = onPlayAlbum,
                    onAppendTrack = onAppendTrack,
                    onPlayNextTrack = onPlayNextTrack,
                    onAppendAlbum = onAppendAlbum,
                    onPlayNextAlbum = onPlayNextAlbum,
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
    val session = state.nowPlaying?.session
    val canNavigatePlayback = canRequestPlaybackNavigation(state)
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
                    fallbackText = track?.album?.ifBlank { track?.title.orEmpty() }.orEmpty(),
                )
                Column(modifier = Modifier.weight(1f)) {
                    Text(
                        track?.title ?: "Nothing playing",
                        fontWeight = FontWeight.SemiBold,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        listOfNotNull(track?.artist, track?.album).joinToString(" · ")
                            .ifBlank { humanizeTransportState(state.nowPlaying?.session?.transportState) },
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        style = MaterialTheme.typography.bodySmall,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                val isPlaying = session?.transportState == "PLAYING"
                FilledIconButton(onClick = if (isPlaying) onPause else onPlay) {
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
    onServerInputChange: (String) -> Unit,
    onConnect: () -> Unit,
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
    onAppendAlbum: (String) -> Unit,
    onPlayNextAlbum: (String) -> Unit,
    onOpenRendererPicker: () -> Unit,
    onDiscoverRenderers: () -> Unit,
    onOpenServerEditor: () -> Unit,
) {
    val spotlightDay = LocalDate.now()
    val spotlightAlbums = remember(state.albums, spotlightDay) {
        val eligibleAlbums = state.albums.filter { it.trackCount > 3 }
        if (eligibleAlbums.isEmpty()) {
            state.albums.take(3)
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
                onAppendAlbum = { onAppendAlbum(album.id) },
                onPlayNextAlbum = { onPlayNextAlbum(album.id) },
            )
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
    onPlayTrack: (String) -> Unit,
    onPlayAlbum: (String) -> Unit,
    onAppendTrack: (String) -> Unit,
    onPlayNextTrack: (String) -> Unit,
    onAppendAlbum: (String) -> Unit,
    onPlayNextAlbum: (String) -> Unit,
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
            onBack = onCloseAlbumDetail,
            backLabel = if (state.selectedArtistDetail != null) "Back to artist" else "Back to library",
            onOpenArtist = { onOpenArtistByName(album.artist) },
            onPlayAlbum = { onPlayAlbum(album.id) },
            onAppendAlbum = { onAppendAlbum(album.id) },
            onPlayNextAlbum = { onPlayNextAlbum(album.id) },
            onPlayTrack = onPlayTrack,
            onAppendTrack = onAppendTrack,
            onPlayNextTrack = onPlayNextTrack,
            onOpenArtistByName = onOpenArtistByName,
            onOpenArtworkPicker = onOpenAlbumArtworkPicker,
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
    val currentEntryId = state.queue?.currentEntryId
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
                        FilledIconButton(onClick = onPlay) {
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
    isConnecting: Boolean,
    errorMessage: String?,
    onServerInputChange: (String) -> Unit,
    onConnect: () -> Unit,
    onRetry: () -> Unit,
    onDisconnect: () -> Unit,
) {
    Column(modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp)) {
        Text("Server", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(6.dp))
        Text(
            "Point the app at your musicd server on the local network.",
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
        Spacer(Modifier.height(8.dp))
        OutlinedButton(onClick = onRetry, enabled = !isConnecting, modifier = Modifier.fillMaxWidth()) {
            Text("Retry current server")
        }
        Spacer(Modifier.height(8.dp))
        TextButton(onClick = onDisconnect, modifier = Modifier.fillMaxWidth()) {
            Text("Disconnect")
        }
        Spacer(Modifier.height(24.dp))
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
    onSelectRenderer: (String) -> Unit,
    onDiscoverRenderers: () -> Unit,
) {
    val accentColor = Color(0xFFF5AF43)
    val accentContainer = Color(0xFF4B3B2B)
    val sheetBackground = Color(0xFF1F1F25)

    Column(
        modifier = Modifier
            .fillMaxWidth()
            .background(sheetBackground)
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
                text = "${renderers.size} AVAILABLE",
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(Modifier.height(20.dp))
        if (renderers.isEmpty()) {
            ElevatedPanel {
                Text(
                    "No saved renderers yet. Run discovery to look for devices on your network.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Spacer(Modifier.height(14.dp))
        }
        renderers.forEach { renderer ->
            val isSelected = renderer.location == selectedRendererLocation
            Card(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(bottom = 14.dp)
                    .clickable { onSelectRenderer(renderer.location) },
                colors = CardDefaults.cardColors(
                    containerColor = if (isSelected) {
                        accentContainer
                    } else {
                        sheetBackground
                    }
                ),
                border = BorderStroke(
                    width = if (isSelected) 1.5.dp else 1.dp,
                    color = if (isSelected) accentColor.copy(alpha = 0.75f)
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
                            Icons.Rounded.Speaker,
                            contentDescription = null,
                            tint = if (isSelected) accentColor else MaterialTheme.colorScheme.onSurfaceVariant,
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
                    }
                    if (isSelected) {
                        Row(
                            modifier = Modifier
                                .background(accentColor, RoundedCornerShape(99.dp))
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
                }
            }
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

private fun rendererDescriptor(renderer: RendererDto): String {
    val displayName = rendererDisplayName(renderer)
    val parts = listOfNotNull(
        renderer.manufacturer?.takeIf { it.isNotBlank() },
        renderer.modelName?.takeIf { it.isNotBlank() && it != displayName },
        renderer.kind.takeIf { it.isNotBlank() }?.uppercase(),
    )
    return parts.joinToString(" · ").ifBlank { "Renderer" }
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
                    onOpenArtist?.let { TextButton(onClick = it) { Text("Artist") } }
                    TextButton(onClick = onPlayNextAlbum) { Text("Play Next") }
                    TextButton(onClick = onAppendAlbum) { Text("Add Queue") }
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
    onBack: () -> Unit,
    backLabel: String,
    onOpenArtist: (() -> Unit)?,
    onOpenArtworkPicker: () -> Unit,
    onPlayAlbum: () -> Unit,
    onAppendAlbum: () -> Unit,
    onPlayNextAlbum: () -> Unit,
    onPlayTrack: (String) -> Unit,
    onAppendTrack: (String) -> Unit,
    onPlayNextTrack: (String) -> Unit,
    onOpenArtistByName: (String) -> Unit,
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
                        text = listOf(album.artist, "${album.trackCount} tracks").joinToString(" · "),
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
                        onOpenArtist?.let {
                            OutlinedButton(onClick = it) {
                                Text("Artist")
                            }
                        }
                        OutlinedButton(onClick = onPlayNextAlbum) {
                            Text("Play Next")
                        }
                        OutlinedButton(onClick = onAppendAlbum) {
                            Text("Add Queue")
                        }
                    }
                }
            }
        }
        items(album.tracks, key = { it.id }) { track ->
            AlbumTrackRow(
                track = track,
                onPlayTrack = { onPlayTrack(track.id) },
                onAppendTrack = { onAppendTrack(track.id) },
                onPlayNextTrack = { onPlayNextTrack(track.id) },
                onOpenArtist = { onOpenArtistByName(track.artist) },
            )
        }
    }
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
                    TextButton(onClick = onPlayNextTrack) { Text("Play Next") }
                    TextButton(onClick = onAppendTrack) { Text("Add Queue") }
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
    val track = state.nowPlaying?.currentTrack
    Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        ArtworkSquare(
            url = resolveUrl(state.baseUrl, track?.artworkUrl),
            modifier = Modifier.weight(0.34f),
            fallbackText = track?.album?.ifBlank { track?.title.orEmpty() }.orEmpty(),
        )
        Column(modifier = Modifier.weight(0.66f)) {
            Text(track?.title ?: "Nothing playing", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Text(
                listOfNotNull(track?.artist, track?.album).joinToString(" · ").ifBlank { "Choose a renderer and start playback." },
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
                FilledIconButton(onClick = onPlay) {
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
    state.nowPlaying?.currentTrack != null || !state.queue?.entries.isNullOrEmpty()

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

private fun serverLabel(serverName: String?, baseUrl: String): String =
    serverName?.takeIf { it.isNotBlank() } ?: if (baseUrl.isBlank()) {
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

private fun currentTitle(tab: MusicdTab): String = when (tab) {
    MusicdTab.Home -> "Home"
    MusicdTab.Library -> "Library"
    MusicdTab.Queue -> "Queue"
}
