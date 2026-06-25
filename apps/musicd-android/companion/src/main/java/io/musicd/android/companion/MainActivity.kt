package io.musicd.android.companion

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.LibraryMusic
import androidx.compose.material.icons.rounded.Refresh
import androidx.compose.material.icons.rounded.RemoveCircleOutline
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private lateinit var repository: CompanionRepository
    private lateinit var pairingStore: PairingTokenStore

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        repository = CompanionRepository(this)
        pairingStore = PairingTokenStore(this)
        startService(
            Intent(this, MusicdCompanionService::class.java)
                .setAction(MusicdCompanionService.ACTION_START_CONTROL_SURFACE),
        )

        setContent {
            MaterialTheme {
                var roots by remember { mutableStateOf(emptyList<SafRoot>()) }
                var scanMessage by remember { mutableStateOf<String?>(null) }
                var pairedClientId by remember { mutableStateOf<String?>(null) }
                var notificationsAllowed by remember { mutableStateOf(hasNotificationPermission()) }
                val isScanning = roots.any { it.lastScanStatus == "scanning" }
                val picker = rememberLauncherForActivityResult(
                    ActivityResultContracts.OpenDocumentTree(),
                ) { uri ->
                    if (uri != null) {
                        lifecycleScope.launch {
                            repository.persistRoot(uri)
                            roots = repository.loadRoots()
                        }
                    }
                }
                val notificationPermissionLauncher = rememberLauncherForActivityResult(
                    ActivityResultContracts.RequestPermission(),
                ) { granted ->
                    notificationsAllowed = granted || hasNotificationPermission()
                }

                fun refreshRoots() {
                    lifecycleScope.launch {
                        roots = repository.loadRoots()
                        pairedClientId = pairingStore.authorizedControllerId()
                        notificationsAllowed = hasNotificationPermission()
                    }
                }

                fun scanRoots() {
                    startService(
                        Intent(this, MusicdCompanionService::class.java)
                            .setAction(MusicdCompanionService.ACTION_SCAN_LIBRARY),
                    )
                    scanMessage = "Scan is running in the companion service. You can leave this screen."
                    lifecycleScope.launch {
                        delay(300)
                        roots = repository.loadRoots()
                    }
                }

                androidx.compose.runtime.LaunchedEffect(Unit) {
                    roots = repository.loadRoots()
                    pairedClientId = pairingStore.authorizedControllerId()
                    notificationsAllowed = hasNotificationPermission()
                }

                androidx.compose.runtime.LaunchedEffect(isScanning, scanMessage) {
                    var idlePolls = 0
                    while (isScanning || scanMessage != null) {
                        val refreshedRoots = repository.loadRoots()
                        roots = refreshedRoots
                        val scanStillActive = refreshedRoots.any { it.lastScanStatus == "scanning" }
                        idlePolls = if (scanStillActive) 0 else idlePolls + 1
                        if (!scanStillActive && idlePolls >= 3) {
                            scanMessage = null
                        }
                        delay(if (scanStillActive) 500 else 1_000)
                    }
                }

                CompanionScreen(
                    roots = roots,
                    isScanning = isScanning,
                    scanMessage = scanMessage,
                    pairedClientId = pairedClientId,
                    notificationsAllowed = notificationsAllowed,
                    onAddRoot = { picker.launch(null) },
                    onRemoveRoot = { root ->
                        lifecycleScope.launch {
                            repository.removeRoot(root)
                            roots = repository.loadRoots()
                        }
                    },
                    onRefresh = ::refreshRoots,
                    onScan = ::scanRoots,
                    onCancelScan = {
                        startService(
                            Intent(this, MusicdCompanionService::class.java)
                                .setAction(MusicdCompanionService.ACTION_CANCEL_SCAN),
                        )
                        scanMessage = "Canceling scan..."
                    },
                    onResetPairing = {
                        pairingStore.clear()
                        pairedClientId = null
                    },
                    onRequestNotifications = {
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                            notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                        }
                    },
                )
            }
        }
    }

    private fun hasNotificationPermission(): Boolean =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(this, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
}

@Composable
private fun CompanionScreen(
    roots: List<SafRoot>,
    isScanning: Boolean,
    scanMessage: String?,
    pairedClientId: String?,
    notificationsAllowed: Boolean,
    onAddRoot: () -> Unit,
    onRemoveRoot: (SafRoot) -> Unit,
    onRefresh: () -> Unit,
    onScan: () -> Unit,
    onCancelScan: () -> Unit,
    onResetPairing: () -> Unit,
    onRequestNotifications: () -> Unit,
) {
    Surface(
        modifier = Modifier
            .fillMaxSize()
            .background(MaterialTheme.colorScheme.background),
    ) {
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 20.dp, vertical = 28.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            item {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(Icons.Rounded.LibraryMusic, contentDescription = null, modifier = Modifier.size(36.dp))
                    Column(modifier = Modifier.weight(1f)) {
                        Text("feltsloth Companion", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold)
                        Text(
                            "Local storage roots for the Android standalone engine.",
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    IconButton(onClick = onRefresh) {
                        Icon(Icons.Rounded.Refresh, contentDescription = "Refresh")
                    }
                }
            }

            item {
                PairingPanel(
                    pairedClientId = pairedClientId,
                    onResetPairing = onResetPairing,
                )
            }

            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                item {
                    NotificationPanel(
                        notificationsAllowed = notificationsAllowed,
                        onRequestNotifications = onRequestNotifications,
                    )
                }
            }

            item {
                Card(
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                    shape = RoundedCornerShape(20.dp),
                ) {
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(18.dp),
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Text("Music folders", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                        Text(
                            "Choose one or more folders from internal storage, SD card, or USB storage. Scanning and playback will be added on top of these persisted permissions.",
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        Button(onClick = onAddRoot, modifier = Modifier.fillMaxWidth()) {
                            Icon(Icons.Rounded.FolderOpen, contentDescription = null)
                            Spacer(Modifier.size(8.dp))
                            Text("Add music folder")
                        }
                        OutlinedButton(
                            onClick = if (isScanning) onCancelScan else onScan,
                            modifier = Modifier.fillMaxWidth(),
                            enabled = roots.isNotEmpty(),
                        ) {
                            Text(if (isScanning) "Cancel scan" else "Scan music folders")
                        }
                        if (isScanning) {
                            LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                            Text(
                                roots.scanHeartbeatText(),
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                        scanMessage?.let {
                            Text(it, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                }
            }

            if (roots.isEmpty()) {
                item {
                    EmptyRootsPanel(onAddRoot = onAddRoot)
                }
            } else {
                items(roots, key = { it.uri }) { root ->
                    RootRow(root = root, onRemove = { onRemoveRoot(root) })
                }
            }
        }
    }
}

@Composable
private fun NotificationPanel(
    notificationsAllowed: Boolean,
    onRequestNotifications: () -> Unit,
) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(18.dp),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(18.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text("Playback notification", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Text(
                if (notificationsAllowed) {
                    "Notifications are enabled. Playback controls can appear while the companion is running."
                } else {
                    "Allow notifications so Android can show playback controls while the companion is running."
                },
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Button(
                onClick = onRequestNotifications,
                enabled = !notificationsAllowed,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(if (notificationsAllowed) "Notifications enabled" else "Allow notifications")
            }
        }
    }
}

@Composable
private fun PairingPanel(pairedClientId: String?, onResetPairing: () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(18.dp),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(18.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text("Controller pairing", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Text(
                if (pairedClientId == null) {
                    "No controller is paired yet. Open local companion mode in the main feltsloth app to allow playback and queue changes."
                } else {
                    "Paired with controller ...${pairedClientId.takeLast(8)}. Only that controller can send playback and queue changes."
                },
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            TextButton(
                onClick = onResetPairing,
                enabled = pairedClientId != null,
            ) {
                Text("Reset pairing")
            }
        }
    }
}

@Composable
private fun EmptyRootsPanel(onAddRoot: () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceVariant),
        shape = RoundedCornerShape(18.dp),
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(18.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Text("No folders selected", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Text(
                "Add a music folder to give the companion app long-lived read access through Android's Storage Access Framework.",
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedButton(onClick = onAddRoot) {
                Text("Add music folder")
            }
        }
    }
}

@Composable
private fun RootRow(root: SafRoot, onRemove: () -> Unit) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
        shape = RoundedCornerShape(16.dp),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 14.dp, vertical = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Rounded.FolderOpen, contentDescription = null)
            Column(modifier = Modifier.weight(1f)) {
                Text(root.label, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (root.lastScanStatus == "scanning") {
                    LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                    Spacer(Modifier.height(4.dp))
                }
                Text(
                    root.scanStatusText(),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                root.lastScanError?.let {
                    Text(it, color = MaterialTheme.colorScheme.error, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                if (root.lastScanStatus == "scanning") {
                    root.scanCurrentItem?.let { currentItem ->
                        Text(
                            currentItem,
                            color = MaterialTheme.colorScheme.secondary,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    Text(
                        root.scanHeartbeatDetail(),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
            IconButton(onClick = onRemove) {
                Icon(Icons.Rounded.RemoveCircleOutline, contentDescription = "Remove folder")
            }
        }
    }
}

private fun SafRoot.scanStatusText(): String =
    when (lastScanStatus) {
        "scanning" -> "${scanTracksFound} tracks found · ${scanFilesVisited} files · ${scanFoldersVisited} folders"
        "complete" -> "${trackCount} tracks · ${scanFilesVisited} files checked · complete"
        "canceled" -> "${trackCount} tracks · canceled after ${scanFilesVisited} files"
        "error" -> "${trackCount} tracks · error after ${scanFilesVisited} files"
        "never_scanned" -> "${trackCount} tracks · never scanned"
        else -> "$trackCount tracks · ${lastScanStatus.replace('_', ' ')}"
    }

private fun SafRoot.scanHeartbeatDetail(): String {
    val lastProgress = scanLastProgressUnix ?: return "Scanning..."
    val ageSeconds = (nowUnix() - lastProgress).coerceAtLeast(0)
    return if (ageSeconds <= 1) {
        "Updated just now"
    } else {
        "Updated ${ageSeconds}s ago"
    }
}

private fun List<SafRoot>.scanHeartbeatText(): String {
    val active = firstOrNull { it.lastScanStatus == "scanning" }
        ?: return "Scanning selected folders..."
    return buildString {
        append("Scanning ")
        append(active.label)
        append(": ")
        append(active.scanTracksFound)
        append(" tracks, ")
        append(active.scanFilesVisited)
        append(" files checked")
    }
}
