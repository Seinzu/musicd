package io.musicd.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
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
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.QueueMusic
import androidx.compose.material.icons.rounded.Album
import androidx.compose.material.icons.rounded.Home
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.TextButton
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import coil.compose.AsyncImage
import io.musicd.android.data.AlbumDetailDto
import io.musicd.android.data.AlbumSummaryDto
import io.musicd.android.data.QueueEntryDto
import io.musicd.android.data.RendererDto
import io.musicd.android.data.TrackSummaryDto

@Composable
fun MusicdApp(viewModel: MusicdViewModel) {
    val state by viewModel.uiState.collectAsStateWithLifecycle()

    if (!state.connected) {
        ServerSetupScreen(
            serverInput = state.serverInput,
            errorMessage = state.errorMessage,
            onServerInputChange = viewModel::updateServerInput,
            onConnect = { viewModel.connect() },
        )
        return
    }

    MusicdRoot(
        state = state,
        onSelectTab = viewModel::selectTab,
        onRefresh = viewModel::refreshAll,
        onOpenRendererPicker = { viewModel.toggleRendererPicker(true) },
        onDismissRendererPicker = { viewModel.toggleRendererPicker(false) },
        onSelectRenderer = viewModel::selectRenderer,
        onDiscoverRenderers = viewModel::discoverRenderers,
        onPlay = viewModel::transportPlay,
        onPause = viewModel::transportPause,
        onStop = viewModel::transportStop,
        onNext = viewModel::transportNext,
        onPrevious = viewModel::transportPrevious,
        onSearchQueryChange = viewModel::updateSearchQuery,
        onOpenAlbum = viewModel::openAlbum,
        onCloseAlbumDetail = viewModel::closeAlbumDetail,
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
    onRefresh: () -> Unit,
    onOpenRendererPicker: () -> Unit,
    onDismissRendererPicker: () -> Unit,
    onSelectRenderer: (String) -> Unit,
    onDiscoverRenderers: () -> Unit,
    onPlay: () -> Unit,
    onPause: () -> Unit,
    onStop: () -> Unit,
    onNext: () -> Unit,
    onPrevious: () -> Unit,
    onSearchQueryChange: (String) -> Unit,
    onOpenAlbum: (String) -> Unit,
    onCloseAlbumDetail: () -> Unit,
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
    BackHandler(enabled = state.selectedAlbumDetail != null) {
        onCloseAlbumDetail()
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
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                AssistChip(
                    onClick = onOpenRendererPicker,
                    label = {
                        Text(
                            state.renderers.firstOrNull { it.location == state.selectedRendererLocation }?.name
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
                Text(it, color = MaterialTheme.colorScheme.error)
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
                )
                MusicdTab.Library -> LibraryScreen(
                    state = state,
                    onSearchQueryChange = onSearchQueryChange,
                    onOpenAlbum = onOpenAlbum,
                    onCloseAlbumDetail = onCloseAlbumDetail,
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
                )
            }
        }
    }
}

@Composable
private fun ServerSetupScreen(
    serverInput: String,
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
                Text("musicd", style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Bold)
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
                Button(onClick = onConnect, modifier = Modifier.fillMaxWidth()) {
                    Text("Connect")
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
) {
    LazyColumn(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        item {
            ElevatedPanel {
                Text("Good evening", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.secondary)
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
        item {
            Text("Library spotlight", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        }
        items(state.albums.take(6), key = { it.id }) { album ->
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

@Composable
private fun LibraryScreen(
    state: MusicdUiState,
    onSearchQueryChange: (String) -> Unit,
    onOpenAlbum: (String) -> Unit,
    onCloseAlbumDetail: () -> Unit,
    onPlayTrack: (String) -> Unit,
    onPlayAlbum: (String) -> Unit,
    onAppendTrack: (String) -> Unit,
    onPlayNextTrack: (String) -> Unit,
    onAppendAlbum: (String) -> Unit,
    onPlayNextAlbum: (String) -> Unit,
) {
    val query = state.searchQuery.trim()
    val normalizedQuery = query.lowercase()
    val filteredAlbums = if (normalizedQuery.isBlank()) {
        state.albums
    } else {
        state.albums.filter { album ->
            album.title.lowercase().contains(normalizedQuery) ||
                album.artist.lowercase().contains(normalizedQuery)
        }
    }
    val filteredTracks = if (normalizedQuery.isBlank()) {
        emptyList()
    } else {
        state.tracks.filter { track ->
            track.title.lowercase().contains(normalizedQuery) ||
                track.artist.lowercase().contains(normalizedQuery) ||
                track.album.lowercase().contains(normalizedQuery)
        }
    }

    state.selectedAlbumDetail?.let { album ->
        AlbumDetailScreen(
            baseUrl = state.baseUrl,
            album = album,
            onBack = onCloseAlbumDetail,
            onPlayAlbum = { onPlayAlbum(album.id) },
            onAppendAlbum = { onAppendAlbum(album.id) },
            onPlayNextAlbum = { onPlayNextAlbum(album.id) },
            onPlayTrack = onPlayTrack,
            onAppendTrack = onAppendTrack,
            onPlayNextTrack = onPlayNextTrack,
        )
        return
    }

    LazyColumn(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            Text("Library", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(10.dp))
            OutlinedTextField(
                value = state.searchQuery,
                onValueChange = onSearchQueryChange,
                label = { Text("Search albums or tracks") },
                placeholder = { Text("Artist, album, or track") },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
            )
            Spacer(Modifier.height(12.dp))
            Text(
                if (normalizedQuery.isBlank()) {
                    "Browse albums or search for a specific track."
                } else {
                    "${filteredAlbums.size} albums and ${filteredTracks.size} tracks match \"$query\"."
                },
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        item {
            Text("Albums", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        }
        items(filteredAlbums, key = { it.id }) { album ->
            AlbumRow(
                baseUrl = state.baseUrl,
                album = album,
                onOpenAlbum = { onOpenAlbum(album.id) },
                onPlayAlbum = { onPlayAlbum(album.id) },
                onAppendAlbum = { onAppendAlbum(album.id) },
                onPlayNextAlbum = { onPlayNextAlbum(album.id) },
            )
        }
        if (normalizedQuery.isNotBlank()) {
            item {
                Spacer(Modifier.height(6.dp))
                Text("Tracks", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            }
            items(filteredTracks.take(40), key = { it.id }) { track ->
                TrackRow(
                    baseUrl = state.baseUrl,
                    track = track,
                    onPlayTrack = { onPlayTrack(track.id) },
                    onAppendTrack = { onAppendTrack(track.id) },
                    onPlayNextTrack = { onPlayNextTrack(track.id) },
                    onOpenAlbum = { onOpenAlbum(track.albumId) },
                )
            }
        }
    }
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
) {
    LazyColumn(verticalArrangement = Arrangement.spacedBy(14.dp)) {
        item {
            ElevatedPanel {
                Text("Queue", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.height(12.dp))
                NowPlayingContent(
                    state = state,
                    onPlay = onPlay,
                    onPause = onPause,
                    onStop = onStop,
                    onNext = onNext,
                    onPrevious = onPrevious,
                )
                Spacer(Modifier.height(14.dp))
                OutlinedButton(onClick = onClearQueue, modifier = Modifier.fillMaxWidth()) {
                    Text("Clear Queue")
                }
            }
        }
        items(state.queue?.entries.orEmpty(), key = { it.id }) { entry ->
            QueueEntryRow(
                entry = entry,
                onMoveUp = { onMoveQueueEntryUp(entry.id) },
                onMoveDown = { onMoveQueueEntryDown(entry.id) },
                onRemove = { onRemoveQueueEntry(entry.id) },
            )
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
    Column(modifier = Modifier.padding(horizontal = 20.dp, vertical = 8.dp)) {
        Text("Play on", style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(6.dp))
        Text(
            "Switch the active renderer for queue and transport control.",
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(16.dp))
        renderers.forEach { renderer ->
            Card(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(bottom = 10.dp)
                    .clickable { onSelectRenderer(renderer.location) },
                colors = CardDefaults.cardColors(
                    containerColor = if (renderer.location == selectedRendererLocation) {
                        MaterialTheme.colorScheme.surfaceVariant
                    } else {
                        MaterialTheme.colorScheme.surface
                    }
                ),
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text(renderer.name, fontWeight = FontWeight.SemiBold)
                    Text(
                        listOfNotNull(renderer.manufacturer, renderer.modelName, renderer.kind.uppercase())
                            .joinToString(" · "),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        OutlinedButton(onClick = onDiscoverRenderers, modifier = Modifier.fillMaxWidth()) {
            Text(if (isDiscovering) "Scanning..." else "Scan for new renderers")
        }
        Spacer(Modifier.height(24.dp))
    }
}

@Composable
private fun AlbumRow(
    baseUrl: String,
    album: AlbumSummaryDto,
    onOpenAlbum: () -> Unit,
    onPlayAlbum: () -> Unit,
    onAppendAlbum: () -> Unit,
    onPlayNextAlbum: () -> Unit,
) {
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable(onClick = onOpenAlbum)
                .padding(14.dp),
            horizontalArrangement = Arrangement.spacedBy(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ArtworkSquare(url = resolveUrl(baseUrl, album.artworkUrl))
            Column(modifier = Modifier.weight(1f)) {
                Text(album.title, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(album.artist, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("${album.trackCount} tracks", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.secondary)
                Spacer(Modifier.height(10.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    TextButton(onClick = onOpenAlbum) { Text("Details") }
                    OutlinedButton(onClick = onPlayAlbum) { Text("Play Album") }
                }
                Spacer(Modifier.height(6.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    TextButton(onClick = onPlayNextAlbum) { Text("Play Next") }
                    TextButton(onClick = onAppendAlbum) { Text("Add Queue") }
                }
            }
        }
    }
}

@Composable
private fun AlbumDetailScreen(
    baseUrl: String,
    album: AlbumDetailDto,
    onBack: () -> Unit,
    onPlayAlbum: () -> Unit,
    onAppendAlbum: () -> Unit,
    onPlayNextAlbum: () -> Unit,
    onPlayTrack: (String) -> Unit,
    onAppendTrack: (String) -> Unit,
    onPlayNextTrack: (String) -> Unit,
) {
    LazyColumn(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        item {
            ElevatedPanel {
                TextButton(onClick = onBack) { Text("Back to library") }
                Spacer(Modifier.height(4.dp))
                ArtworkSquare(
                    url = resolveUrl(baseUrl, album.artworkUrl),
                    modifier = Modifier.fillMaxWidth(),
                )
                Spacer(Modifier.height(14.dp))
                Text(album.title, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
                Text(album.artist, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(
                    "${album.trackCount} tracks",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.secondary,
                )
                Spacer(Modifier.height(14.dp))
                Button(onClick = onPlayAlbum, modifier = Modifier.fillMaxWidth()) {
                    Text("Play Album")
                }
                Spacer(Modifier.height(8.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = onPlayNextAlbum, modifier = Modifier.weight(1f)) {
                        Text("Play Next")
                    }
                    OutlinedButton(onClick = onAppendAlbum, modifier = Modifier.weight(1f)) {
                        Text("Add Queue")
                    }
                }
            }
        }
        items(album.tracks, key = { it.id }) { track ->
            TrackRow(
                baseUrl = baseUrl,
                track = track,
                onPlayTrack = { onPlayTrack(track.id) },
                onAppendTrack = { onAppendTrack(track.id) },
                onPlayNextTrack = { onPlayNextTrack(track.id) },
                onOpenAlbum = null,
            )
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
    onOpenAlbum: (() -> Unit)?,
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
            )
            Column(modifier = Modifier.weight(1f)) {
                Text(track.title, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    listOfNotNull(track.artist, track.album.takeIf { it.isNotBlank() }).joinToString(" · "),
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
    entry: QueueEntryDto,
    onMoveUp: () -> Unit,
    onMoveDown: () -> Unit,
    onRemove: () -> Unit,
) {
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(entry.title ?: "Unknown Track", fontWeight = FontWeight.SemiBold)
            Text(
                listOfNotNull(entry.artist, entry.album).joinToString(" · "),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(6.dp))
            Text(
                entry.entryStatus.replace('_', ' '),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.secondary,
            )
            Spacer(Modifier.height(10.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TextButton(onClick = onMoveUp) { Text("Up") }
                TextButton(onClick = onMoveDown) { Text("Down") }
                TextButton(onClick = onRemove) { Text("Remove") }
            }
        }
    }
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
    val track = state.nowPlaying?.currentTrack
    Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
        ArtworkSquare(
            url = resolveUrl(state.baseUrl, track?.artworkUrl),
            modifier = Modifier.weight(0.34f),
        )
        Column(modifier = Modifier.weight(0.66f)) {
            Text(track?.title ?: "Nothing playing", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Text(
                listOfNotNull(track?.artist, track?.album).joinToString(" · ").ifBlank { "Choose a renderer and start playback." },
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(10.dp))
            Text(
                state.nowPlaying?.session?.transportState ?: "IDLE",
                color = MaterialTheme.colorScheme.secondary,
                style = MaterialTheme.typography.labelLarge,
            )
            Spacer(Modifier.height(14.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = onPrevious) { Text("Prev") }
                Button(onClick = onPlay) { Icon(Icons.Rounded.PlayArrow, contentDescription = null) }
                OutlinedButton(onClick = onPause) { Text("Pause") }
            }
            Spacer(Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = onStop) { Text("Stop") }
                OutlinedButton(onClick = onNext) { Text("Next") }
            }
        }
    }
}

@Composable
private fun ArtworkSquare(url: String?, modifier: Modifier = Modifier) {
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
                    .height(120.dp),
            )
        } else {
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .height(120.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text("No Art", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
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

private fun resolveUrl(baseUrl: String, path: String?): String? {
    if (path.isNullOrBlank()) {
        return null
    }
    if (path.startsWith("http://") || path.startsWith("https://")) {
        return path
    }
    return "${baseUrl.trimEnd('/')}/${path.trimStart('/')}"
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
