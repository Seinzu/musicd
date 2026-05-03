package io.musicd.android.playback

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.os.Build
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import androidx.core.graphics.drawable.toBitmap
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import androidx.media.app.NotificationCompat.MediaStyle
import coil.imageLoader
import coil.request.ImageRequest
import coil.request.SuccessResult
import io.musicd.android.MainActivity
import io.musicd.android.data.MusicdRepository
import io.musicd.android.data.PlaybackEventDto
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MusicdPlaybackNotificationService : Service() {
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val repository by lazy { MusicdRepository(applicationContext) }

    private lateinit var mediaSession: MediaSessionCompat
    private lateinit var localPlayer: ExoPlayer
    private var observerJob: Job? = null
    private var localPlaybackReportJob: Job? = null
    private var currentBaseUrl: String = ""
    private var currentRendererLocation: String = ""
    private var currentServerName: String? = null
    private var latestPlaybackEvent: PlaybackEventDto? = null
    private var latestArtworkUrl: String? = null
    private var latestArtworkBitmap: Bitmap? = null
    private var suppressLocalPlayerSync = false
    private var lastReportedLocalSessionSignature: String? = null
    private var lastAppliedServerSeekPositionMs: Long? = null
    private var pendingServerAdvanceTrackUri: String? = null

    override fun onCreate() {
        super.onCreate()
        ensureNotificationChannel()
        val audioAttributes = AudioAttributes.Builder()
            .setUsage(C.USAGE_MEDIA)
            .setContentType(C.AUDIO_CONTENT_TYPE_MUSIC)
            .build()
        localPlayer = ExoPlayer.Builder(this).build().apply {
            setAudioAttributes(audioAttributes, true)
            setHandleAudioBecomingNoisy(true)
            addListener(
                object : Player.Listener {
                    override fun onIsPlayingChanged(isPlaying: Boolean) {
                        if (!suppressLocalPlayerSync) {
                            updateLocalPlaybackReportingLoop()
                            reportLocalSession()
                        }
                    }

                    override fun onPlaybackStateChanged(playbackState: Int) {
                        if (!suppressLocalPlayerSync) {
                            updateLocalPlaybackReportingLoop()
                            if (playbackState == Player.STATE_ENDED) {
                                reportLocalCompletion()
                            } else {
                                reportLocalSession()
                            }
                        }
                    }

                    override fun onPositionDiscontinuity(
                        oldPosition: Player.PositionInfo,
                        newPosition: Player.PositionInfo,
                        reason: Int,
                    ) {
                        if (!suppressLocalPlayerSync && reason == Player.DISCONTINUITY_REASON_SEEK) {
                            reportLocalSession(force = true)
                        }
                    }

                    override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
                        if (suppressLocalPlayerSync || !isLocalRendererActive()) return
                        lastAppliedServerSeekPositionMs = null
                        if (reason == Player.MEDIA_ITEM_TRANSITION_REASON_AUTO) {
                            pendingServerAdvanceTrackUri =
                                mediaItem?.localConfiguration?.uri?.toString()
                            reportLocalCompletion()
                            reportLocalSession(force = true)
                        }
                    }
                },
            )
        }
        mediaSession = MediaSessionCompat(this, MEDIA_SESSION_TAG).apply {
            setCallback(
                object : MediaSessionCompat.Callback() {
                    override fun onPlay() {
                        performTransportAction(ACTION_PLAY)
                    }

                    override fun onPause() {
                        performTransportAction(ACTION_PAUSE)
                    }

                    override fun onSkipToNext() {
                        performTransportAction(ACTION_NEXT)
                    }

                    override fun onSkipToPrevious() {
                        performTransportAction(ACTION_PREVIOUS)
                    }

                    override fun onStop() {
                        performTransportAction(ACTION_STOP_TRANSPORT)
                    }

                    override fun onSeekTo(pos: Long) {
                        if (!isLocalRendererActive() || localPlayer.currentMediaItem == null) {
                            return
                        }
                        val boundedPosition = pos.coerceAtLeast(0L)
                        suppressLocalPlayerSync = true
                        try {
                            localPlayer.seekTo(boundedPosition)
                            lastAppliedServerSeekPositionMs = boundedPosition
                        } finally {
                            suppressLocalPlayerSync = false
                        }
                        reportLocalSession(force = true)
                        latestPlaybackEvent?.let { event ->
                            startForeground(
                                NOTIFICATION_ID,
                                buildPlaybackNotification(event, latestArtworkBitmap),
                            )
                        }
                    }
                },
            )
            isActive = true
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> {
                currentBaseUrl = intent.getStringExtra(EXTRA_BASE_URL).orEmpty()
                currentRendererLocation = intent.getStringExtra(EXTRA_RENDERER_LOCATION).orEmpty()
                currentServerName = intent.getStringExtra(EXTRA_SERVER_NAME)

                if (currentBaseUrl.isBlank() || currentRendererLocation.isBlank()) {
                    stopServiceNow()
                    return START_NOT_STICKY
                }

                startForeground(NOTIFICATION_ID, buildBootstrapNotification())
                startObserving()
                return START_STICKY
            }

            ACTION_STOP_SERVICE -> {
                stopServiceNow()
                return START_NOT_STICKY
            }

            ACTION_PLAY,
            ACTION_PAUSE,
            ACTION_TOGGLE_PLAYBACK,
            ACTION_NEXT,
            ACTION_PREVIOUS,
            ACTION_STOP_TRANSPORT,
            -> {
                performTransportAction(intent.action.orEmpty())
                return START_STICKY
            }

            else -> return START_NOT_STICKY
        }
    }

    override fun onDestroy() {
        observerJob?.cancel()
        localPlaybackReportJob?.cancel()
        serviceScope.cancel()
        localPlayer.release()
        mediaSession.release()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startObserving() {
        observerJob?.cancel()
        observerJob = serviceScope.launch {
            while (isActive) {
                try {
                    repository.observePlaybackEvents(
                        baseUrl = currentBaseUrl,
                        rendererLocation = currentRendererLocation,
                    ) { event ->
                        serviceScope.launch {
                            handlePlaybackEvent(event)
                        }
                    }
                    if (!isActive) break
                    delay(1_000)
                } catch (_: Throwable) {
                    if (!isActive) break
                    delay(3_000)
                }
            }
        }
    }

    private fun handlePlaybackEvent(event: PlaybackEventDto) {
        latestPlaybackEvent = event
        if (!hasDisplayablePlayback(event)) {
            stopServiceNow()
            return
        }

        serviceScope.launch {
            syncLocalPlayback(event)
            val artworkBitmap = loadArtworkBitmap(event)
            updateMediaSession(event, artworkBitmap)
            startForeground(NOTIFICATION_ID, buildPlaybackNotification(event, artworkBitmap))
        }
    }

    private fun performTransportAction(action: String) {
        if (currentBaseUrl.isBlank() || currentRendererLocation.isBlank()) return

        serviceScope.launch {
            runCatching {
                when (action) {
                    ACTION_PLAY -> repository.transportPlay(currentBaseUrl, currentRendererLocation)
                    ACTION_PAUSE -> repository.transportPause(currentBaseUrl, currentRendererLocation)
                    ACTION_NEXT -> repository.transportNext(currentBaseUrl, currentRendererLocation)
                    ACTION_PREVIOUS -> repository.transportPrevious(currentBaseUrl, currentRendererLocation)
                    ACTION_STOP_TRANSPORT -> repository.transportStop(currentBaseUrl, currentRendererLocation)
                    ACTION_TOGGLE_PLAYBACK -> {
                        if (isCurrentlyPlaying(latestPlaybackEvent)) {
                            repository.transportPause(currentBaseUrl, currentRendererLocation)
                        } else {
                            repository.transportPlay(currentBaseUrl, currentRendererLocation)
                        }
                    }
                }
            }
        }
    }

    private fun updateMediaSession(event: PlaybackEventDto, artworkBitmap: Bitmap?) {
        val currentTrack = event.nowPlaying.currentTrack
        val subtitle = listOfNotNull(currentTrack?.artist, currentTrack?.album)
            .filter { it.isNotBlank() }
            .joinToString(" • ")
        val durationMs = playbackDurationMillis(event)

        val metadata = MediaMetadataCompat.Builder()
            .putString(
                MediaMetadataCompat.METADATA_KEY_TITLE,
                currentTrack?.title ?: "musicd",
            )
            .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, currentTrack?.artist)
            .putString(MediaMetadataCompat.METADATA_KEY_ALBUM, currentTrack?.album)
            .putString(MediaMetadataCompat.METADATA_KEY_DISPLAY_SUBTITLE, subtitle)
            .putLong(MediaMetadataCompat.METADATA_KEY_DURATION, durationMs)
        artworkBitmap?.let { bitmap ->
            metadata.putBitmap(MediaMetadataCompat.METADATA_KEY_ALBUM_ART, bitmap)
            metadata.putBitmap(MediaMetadataCompat.METADATA_KEY_ART, bitmap)
            metadata.putBitmap(MediaMetadataCompat.METADATA_KEY_DISPLAY_ICON, bitmap)
        }
        mediaSession.setMetadata(metadata.build())

        val transportState = resolvedTransportState(event)
        val state = when (transportState) {
            "PLAYING" -> PlaybackStateCompat.STATE_PLAYING
            "PAUSED_PLAYBACK" -> PlaybackStateCompat.STATE_PAUSED
            "TRANSITIONING" -> PlaybackStateCompat.STATE_BUFFERING
            "COMPLETED" -> PlaybackStateCompat.STATE_SKIPPING_TO_NEXT
            "STOPPED", "NO_MEDIA_PRESENT" -> PlaybackStateCompat.STATE_STOPPED
            else -> PlaybackStateCompat.STATE_NONE
        }
        val positionMs = playbackPositionMillis(event)
        val playbackSpeed = if (state == PlaybackStateCompat.STATE_PLAYING) 1f else 0f
        val actions = PlaybackStateCompat.ACTION_PLAY or
            PlaybackStateCompat.ACTION_PAUSE or
            PlaybackStateCompat.ACTION_PLAY_PAUSE or
            PlaybackStateCompat.ACTION_SKIP_TO_NEXT or
            PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS or
            PlaybackStateCompat.ACTION_STOP or
            if (isLocalRendererActive()) PlaybackStateCompat.ACTION_SEEK_TO else 0L

        mediaSession.setPlaybackState(
            PlaybackStateCompat.Builder()
                .setActions(actions)
                .setState(state, positionMs, playbackSpeed)
                .build(),
        )
    }

    private fun buildBootstrapNotification() =
        NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(currentServerName ?: "musicd")
            .setContentText("Connecting to playback updates…")
            .setSubText(notificationContextLine())
            .setContentIntent(appLaunchPendingIntent())
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .build()

    private fun buildPlaybackNotification(event: PlaybackEventDto, artworkBitmap: Bitmap?) =
        run {
            val currentlyPlaying = resolvedTransportState(event) in setOf("PLAYING", "TRANSITIONING")
            NotificationCompat.Builder(this, CHANNEL_ID)
                .setSmallIcon(android.R.drawable.ic_media_play)
                .setContentTitle(notificationTitle(event))
                .setContentText(notificationText(event))
                .setSubText(notificationContextLine(event))
                .setContentIntent(appLaunchPendingIntent())
                .setDeleteIntent(servicePendingIntent(ACTION_STOP_SERVICE))
                .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
                .setCategory(NotificationCompat.CATEGORY_TRANSPORT)
                .setOnlyAlertOnce(true)
                .setOngoing(currentlyPlaying)
                .setLargeIcon(artworkBitmap)
                .addAction(
                    android.R.drawable.ic_media_previous,
                    "Previous",
                    servicePendingIntent(ACTION_PREVIOUS),
                )
                .addAction(
                    if (currentlyPlaying) android.R.drawable.ic_media_pause else android.R.drawable.ic_media_play,
                    if (currentlyPlaying) "Pause" else "Play",
                    servicePendingIntent(
                        if (currentlyPlaying) ACTION_PAUSE else ACTION_PLAY,
                    ),
                )
                .addAction(
                    android.R.drawable.ic_media_next,
                    "Next",
                    servicePendingIntent(ACTION_NEXT),
                )
                .addAction(
                    android.R.drawable.ic_menu_close_clear_cancel,
                    "Stop",
                    servicePendingIntent(ACTION_STOP_TRANSPORT),
                )
                .setStyle(
                    MediaStyle()
                        .setMediaSession(mediaSession.sessionToken)
                        .setShowActionsInCompactView(0, 1, 2),
                )
                .build()
        }

    private fun notificationTitle(event: PlaybackEventDto): String =
        event.nowPlaying.currentTrack?.title
            ?: event.queue.entries.firstOrNull()?.title
            ?: currentServerName
            ?: "musicd"

    private fun notificationText(event: PlaybackEventDto): String {
        val currentTrack = event.nowPlaying.currentTrack
        if (currentTrack != null) {
            return listOfNotNull(currentTrack.artist, currentTrack.album)
                .filter { it.isNotBlank() }
                .joinToString(" • ")
                .ifBlank { currentRendererLocation }
        }

        return when {
            event.queue.entries.isNotEmpty() -> "${event.queue.entries.size} items queued"
            !currentRendererLocation.isBlank() -> currentRendererLocation
            else -> currentServerName ?: "musicd"
        }
    }

    private fun notificationContextLine(event: PlaybackEventDto? = latestPlaybackEvent): String {
        val transportState = humanizeTransportState(resolvedTransportState(event))
        val rendererName = event?.nowPlaying?.renderer?.name
            ?.takeIf { it.isNotBlank() }
            ?: currentServerName
            ?: "musicd"
        return listOfNotNull(transportState, rendererName)
            .joinToString(" • ")
    }

    private suspend fun loadArtworkBitmap(event: PlaybackEventDto): Bitmap? {
        val artworkUrl = resolveUrl(currentBaseUrl, event.nowPlaying.currentTrack?.artworkUrl)
        if (artworkUrl.isNullOrBlank()) {
            latestArtworkUrl = null
            latestArtworkBitmap = null
            return null
        }
        if (artworkUrl == latestArtworkUrl && latestArtworkBitmap != null) {
            return latestArtworkBitmap
        }

        val loadedBitmap: Bitmap? = withContext(Dispatchers.IO) {
            runCatching {
                val request = ImageRequest.Builder(this@MusicdPlaybackNotificationService)
                    .data(artworkUrl)
                    .allowHardware(false)
                    .size(512)
                    .build()
                val result = applicationContext.imageLoader.execute(request)
                (result as? SuccessResult)?.drawable?.toBitmap()
            }.getOrNull()
        }
        latestArtworkUrl = artworkUrl
        latestArtworkBitmap = loadedBitmap
        return loadedBitmap
    }

    private suspend fun syncLocalPlayback(event: PlaybackEventDto) {
        if (!isLocalRendererActive()) {
            stopLocalPlayer()
            return
        }

        val track = event.nowPlaying.currentTrack ?: run {
            stopLocalPlayer()
            return
        }
        val streamUrl = currentTrackStreamUrl(track.id) ?: return
        val nextStreamUrl = nextQueuedTrackStreamUrl(event)
        val currentUri = localPlayer.currentMediaItem?.localConfiguration?.uri?.toString()
        val transportState = event.nowPlaying.session?.transportState.orEmpty()
        val serverPositionMs = event.nowPlaying.session?.positionSeconds?.times(1000L)

        if (currentUri == streamUrl) {
            pendingServerAdvanceTrackUri = null
        } else if (
            pendingServerAdvanceTrackUri != null &&
            currentUri == pendingServerAdvanceTrackUri &&
            currentUri == nextStreamUrl
        ) {
            updateLocalPlaybackReportingLoop()
            return
        } else if (pendingServerAdvanceTrackUri != null) {
            pendingServerAdvanceTrackUri = null
        }

        suppressLocalPlayerSync = true
        try {
            val desiredPlaylist = buildDesiredLocalPlaylist(track.id, nextQueuedTrackId(event))
            if (!playlistMatchesDesired(desiredPlaylist)) {
                val startPositionMs = if (currentUri == streamUrl) {
                    localPlayer.currentPosition.coerceAtLeast(0L)
                } else {
                    serverPositionMs ?: 0L
                }
                localPlayer.setMediaItems(desiredPlaylist, 0, startPositionMs)
                localPlayer.prepare()
                if (currentUri != streamUrl) {
                    lastAppliedServerSeekPositionMs = startPositionMs.takeIf { it > 0L }
                }
            } else if (shouldApplyServerSeek(localPlayer.currentPosition, serverPositionMs, transportState)) {
                localPlayer.seekTo(serverPositionMs ?: 0L)
                lastAppliedServerSeekPositionMs = serverPositionMs
            }

            when (transportState) {
                "PLAYING", "TRANSITIONING" -> localPlayer.play()
                "PAUSED_PLAYBACK" -> {
                    if (localPlayer.playbackState == Player.STATE_IDLE) {
                        localPlayer.prepare()
                    }
                    localPlayer.pause()
                }
                "STOPPED", "NO_MEDIA_PRESENT", "COMPLETED" -> stopLocalPlayer()
            }
        } finally {
            suppressLocalPlayerSync = false
        }
        updateLocalPlaybackReportingLoop()
    }

    private fun stopLocalPlayer() {
        suppressLocalPlayerSync = true
        try {
            localPlayer.stop()
            localPlayer.clearMediaItems()
            lastAppliedServerSeekPositionMs = null
            pendingServerAdvanceTrackUri = null
        } finally {
            suppressLocalPlayerSync = false
        }
        updateLocalPlaybackReportingLoop()
    }

    private fun updateLocalPlaybackReportingLoop() {
        localPlaybackReportJob?.cancel()
        if (!isLocalRendererActive() || !localPlayer.isPlaying) {
            localPlaybackReportJob = null
            return
        }
        localPlaybackReportJob = serviceScope.launch {
            while (isActive && isLocalRendererActive() && localPlayer.isPlaying) {
                reportLocalSession(force = true)
                delay(5_000)
            }
        }
    }

    private fun reportLocalSession(force: Boolean = false) {
        if (!isLocalRendererActive() || currentBaseUrl.isBlank() || currentRendererLocation.isBlank()) {
            return
        }
        val currentTrackUri = localPlayer.currentMediaItem?.localConfiguration?.uri?.toString()
        val transportState = localTransportState()
        val durationSeconds = localPlayer.duration.takeIf { it > 0L }?.div(1000L)
        val positionSeconds = localPlayer.currentPosition.takeIf { it >= 0L }?.div(1000L)
        val signature = listOf(
            transportState,
            currentTrackUri.orEmpty(),
            positionSeconds?.toString().orEmpty(),
            durationSeconds?.toString().orEmpty(),
        ).joinToString("|")
        if (!force && signature == lastReportedLocalSessionSignature) {
            return
        }
        lastReportedLocalSessionSignature = signature
        serviceScope.launch {
            runCatching {
                repository.reportAndroidLocalSession(
                    baseUrl = currentBaseUrl,
                    rendererLocation = currentRendererLocation,
                    transportState = transportState,
                    currentTrackUri = currentTrackUri,
                    positionSeconds = positionSeconds,
                    durationSeconds = durationSeconds,
                )
            }
        }
    }

    private fun playbackPositionMillis(event: PlaybackEventDto): Long =
        if (isLocalRendererActive()) {
            localPlayer.currentPosition.coerceAtLeast(0L)
        } else {
            (event.nowPlaying.session?.positionSeconds ?: 0L) * 1000L
        }

    private fun playbackDurationMillis(event: PlaybackEventDto): Long =
        if (isLocalRendererActive()) {
            localPlayer.duration.takeIf { it > 0L }
                ?: ((event.nowPlaying.session?.durationSeconds
                    ?: event.nowPlaying.currentTrack?.durationSeconds
                    ?: 0L) * 1000L)
        } else {
            ((event.nowPlaying.session?.durationSeconds
                ?: event.nowPlaying.currentTrack?.durationSeconds
                ?: 0L) * 1000L)
        }

    private fun resolvedTransportState(event: PlaybackEventDto?): String =
        if (isLocalRendererActive()) {
            localTransportState()
        } else {
            event?.nowPlaying?.session?.transportState.orEmpty()
        }

    private fun shouldApplyServerSeek(
        currentPositionMs: Long,
        serverPositionMs: Long?,
        transportState: String,
    ): Boolean {
        val targetPositionMs = serverPositionMs ?: return false
        if (targetPositionMs <= 0L || localPlayer.currentMediaItem == null) {
            return false
        }
        if (targetPositionMs == lastAppliedServerSeekPositionMs) {
            return false
        }
        if (kotlin.math.abs(currentPositionMs - targetPositionMs) < SERVER_SEEK_SYNC_TOLERANCE_MS) {
            return false
        }
        return !localPlayer.isPlaying ||
            transportState == "PAUSED_PLAYBACK" ||
            transportState == "TRANSITIONING"
    }

    private fun buildDesiredLocalPlaylist(
        currentTrackId: String,
        nextTrackId: String?,
    ): List<MediaItem> =
        buildList {
            currentTrackStreamUrl(currentTrackId)?.let { streamUrl ->
                add(
                    MediaItem.Builder()
                        .setMediaId(currentTrackId)
                        .setUri(streamUrl)
                        .build(),
                )
            }
            nextTrackId
                ?.takeIf { it.isNotBlank() }
                ?.let { queuedTrackId ->
                    currentTrackStreamUrl(queuedTrackId)?.let { streamUrl ->
                        add(
                            MediaItem.Builder()
                                .setMediaId(queuedTrackId)
                                .setUri(streamUrl)
                                .build(),
                        )
                    }
                }
        }

    private fun playlistMatchesDesired(desiredPlaylist: List<MediaItem>): Boolean {
        if (localPlayer.currentMediaItemIndex != 0) return false
        if (localPlayer.mediaItemCount != desiredPlaylist.size) return false
        return desiredPlaylist.indices.all { index ->
            localPlayer.getMediaItemAt(index).localConfiguration?.uri?.toString() ==
                desiredPlaylist[index].localConfiguration?.uri?.toString()
        }
    }

    private fun nextQueuedTrackId(event: PlaybackEventDto): String? {
        val entries = event.queue.entries
        val currentEntryId = event.queue.currentEntryId
        val currentIndex = currentEntryId?.let { targetId ->
            entries.indexOfFirst { it.id == targetId }
                .takeIf { index -> index >= 0 }
        } ?: entries.indexOfFirst { entry ->
            entry.entryStatus == "playing" ||
                entry.trackId == event.nowPlaying.currentTrack?.id
        }.takeIf { index -> index >= 0 }

        return if (currentIndex != null) {
            entries
                .drop(currentIndex + 1)
                .firstOrNull { it.entryStatus == "pending" }
                ?.trackId
        } else {
            entries
                .firstOrNull { it.entryStatus == "pending" && it.trackId != event.nowPlaying.currentTrack?.id }
                ?.trackId
        }
    }

    private fun nextQueuedTrackStreamUrl(event: PlaybackEventDto): String? =
        currentTrackStreamUrl(nextQueuedTrackId(event))

    private fun reportLocalCompletion() {
        if (!isLocalRendererActive() || currentBaseUrl.isBlank() || currentRendererLocation.isBlank()) {
            return
        }
        serviceScope.launch {
            runCatching {
                repository.reportAndroidLocalCompleted(
                    baseUrl = currentBaseUrl,
                    rendererLocation = currentRendererLocation,
                )
            }
        }
    }

    private fun localTransportState(): String =
        when {
            localPlayer.currentMediaItem == null -> "NO_MEDIA_PRESENT"
            localPlayer.playbackState == Player.STATE_BUFFERING -> "TRANSITIONING"
            localPlayer.isPlaying -> "PLAYING"
            localPlayer.playbackState == Player.STATE_ENDED -> "COMPLETED"
            localPlayer.playbackState == Player.STATE_READY && !localPlayer.playWhenReady -> "PAUSED_PLAYBACK"
            localPlayer.playbackState == Player.STATE_IDLE -> "STOPPED"
            else -> "READY"
        }

    private fun currentTrackStreamUrl(trackId: String?): String? =
        trackId
            ?.takeIf { it.isNotBlank() }
            ?.let { value -> "${currentBaseUrl.trimEnd('/')}/stream/track/$value" }

    private fun appLaunchPendingIntent(): PendingIntent {
        val intent = Intent(this, MainActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        return PendingIntent.getActivity(
            this,
            REQUEST_OPEN_APP,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    private fun servicePendingIntent(action: String): PendingIntent {
        val intent = Intent(this, MusicdPlaybackNotificationService::class.java).apply {
            this.action = action
        }
        return PendingIntent.getService(
            this,
            action.hashCode(),
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    private fun ensureNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = getSystemService(NotificationManager::class.java) ?: return
        val channel = NotificationChannel(
            CHANNEL_ID,
            "Playback controls",
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Remote playback controls for musicd"
        }
        manager.createNotificationChannel(channel)
    }

    private fun stopServiceNow() {
        observerJob?.cancel()
        observerJob = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
    }

    private fun isLocalRendererActive(): Boolean =
        isLocalRendererLocation(currentRendererLocation)

    companion object {
        private const val CHANNEL_ID = "musicd_playback"
        private const val NOTIFICATION_ID = 1401
        private const val MEDIA_SESSION_TAG = "musicd-remote-playback"

        private const val ACTION_START = "io.musicd.android.playback.START"
        private const val ACTION_STOP_SERVICE = "io.musicd.android.playback.STOP_SERVICE"
        private const val ACTION_PLAY = "io.musicd.android.playback.PLAY"
        private const val ACTION_PAUSE = "io.musicd.android.playback.PAUSE"
        private const val ACTION_TOGGLE_PLAYBACK = "io.musicd.android.playback.TOGGLE"
        private const val ACTION_NEXT = "io.musicd.android.playback.NEXT"
        private const val ACTION_PREVIOUS = "io.musicd.android.playback.PREVIOUS"
        private const val ACTION_STOP_TRANSPORT = "io.musicd.android.playback.STOP_TRANSPORT"

        private const val EXTRA_BASE_URL = "base_url"
        private const val EXTRA_RENDERER_LOCATION = "renderer_location"
        private const val EXTRA_SERVER_NAME = "server_name"

        private const val REQUEST_OPEN_APP = 2001
        private const val SERVER_SEEK_SYNC_TOLERANCE_MS = 4_000L

        fun start(
            context: Context,
            baseUrl: String,
            rendererLocation: String,
            serverName: String?,
        ) {
            val intent = Intent(context, MusicdPlaybackNotificationService::class.java).apply {
                action = ACTION_START
                putExtra(EXTRA_BASE_URL, baseUrl)
                putExtra(EXTRA_RENDERER_LOCATION, rendererLocation)
                putExtra(EXTRA_SERVER_NAME, serverName)
            }
            ContextCompat.startForegroundService(context, intent)
        }

        fun stop(context: Context) {
            val intent = Intent(context, MusicdPlaybackNotificationService::class.java).apply {
                action = ACTION_STOP_SERVICE
            }
            context.startService(intent)
        }

        private fun hasDisplayablePlayback(event: PlaybackEventDto): Boolean =
            event.nowPlaying.currentTrack != null || event.queue.entries.isNotEmpty()

        private fun isCurrentlyPlaying(event: PlaybackEventDto?): Boolean =
            when (event?.nowPlaying?.session?.transportState) {
                "PLAYING", "TRANSITIONING" -> true
                else -> false
            }

        private fun isLocalRendererLocation(rendererLocation: String): Boolean =
            rendererLocation.startsWith("android-local://")

        private fun resolveUrl(baseUrl: String, path: String?): String? {
            val normalizedPath = path?.trim().orEmpty()
            if (normalizedPath.isBlank()) return null
            return if (
                normalizedPath.startsWith("http://") ||
                normalizedPath.startsWith("https://")
            ) {
                normalizedPath
            } else {
                "${baseUrl.trimEnd('/')}/${
                    normalizedPath.removePrefix("/")
                }"
            }
        }

        private fun humanizeTransportState(rawState: String?): String? =
            when (rawState?.trim()?.uppercase()) {
                null, "" -> null
                "IDLE" -> "Idle"
                "PLAYING" -> "Playing"
                "PAUSED_PLAYBACK" -> "Paused"
                "STOPPED" -> "Stopped"
                "TRANSITIONING" -> "Changing track"
                "NO_MEDIA_PRESENT" -> "No media"
                else -> rawState.lowercase()
                    .split('_')
                    .filter { it.isNotBlank() }
                    .joinToString(" ") { word ->
                        word.replaceFirstChar { char -> char.titlecase() }
                    }
            }
    }
}
