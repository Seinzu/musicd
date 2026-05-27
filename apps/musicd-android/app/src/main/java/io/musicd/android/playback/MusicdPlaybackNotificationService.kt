package io.musicd.android.playback

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.os.Build
import android.os.IBinder
import android.os.SystemClock
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
import io.musicd.android.data.LastfmRepository
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

private data class GroupPlaybackClock(
    val trackId: String,
    val positionMs: Long,
    val capturedElapsedMs: Long,
    val durationMs: Long?,
    val transportState: String,
)

class MusicdPlaybackNotificationService : Service() {
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val repository by lazy { MusicdRepository(applicationContext) }
    private val lastfmScrobbler by lazy {
        LastfmScrobbler(
            repository = LastfmRepository(applicationContext),
            scope = serviceScope,
        )
    }

    private lateinit var mediaSession: MediaSessionCompat
    private lateinit var localPlayer: ExoPlayer
    private var observerJob: Job? = null
    private var localPlaybackReportJob: Job? = null
    private var groupPlaybackSyncJob: Job? = null
    private var currentBaseUrl: String = ""
    private var currentRendererLocation: String = ""
    private var currentLocalRendererLocation: String = ""
    private var currentServerName: String? = null
    private var latestPlaybackEvent: PlaybackEventDto? = null
    private var latestArtworkUrl: String? = null
    private var latestArtworkBitmap: Bitmap? = null
    private var suppressLocalPlayerSync = false
    private var lastReportedLocalSessionSignature: String? = null
    private var lastAppliedServerSeekPositionMs: Long? = null
    private var lastGroupHardSeekElapsedMs: Long = 0L
    private var groupPlaybackClock: GroupPlaybackClock? = null
    private var groupDriftDirection: Int = 0
    private var groupDriftSampleCount: Int = 0
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
                            promoteToForeground(buildPlaybackNotification(event, latestArtworkBitmap))
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
                currentLocalRendererLocation = intent.getStringExtra(EXTRA_LOCAL_RENDERER_LOCATION).orEmpty()
                currentServerName = intent.getStringExtra(EXTRA_SERVER_NAME)

                if (currentBaseUrl.isBlank() || currentRendererLocation.isBlank()) {
                    stopServiceNow()
                    return START_NOT_STICKY
                }

                if (!promoteToForeground(buildBootstrapNotification())) {
                    stopServiceNow()
                    return START_NOT_STICKY
                }
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
        lastfmScrobbler.handlePlaybackEvent(event)
        if (!hasDisplayablePlayback(event)) {
            stopServiceNow()
            return
        }

        serviceScope.launch {
            syncLocalPlayback(event)
            val artworkBitmap = loadArtworkBitmap(event)
            updateMediaSession(event, artworkBitmap)
            if (!promoteToForeground(buildPlaybackNotification(event, artworkBitmap))) {
                stopServiceNow()
            }
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
            .ifBlank {
                event.nowPlaying.session?.artist.orEmpty()
            }
        val durationMs = playbackDurationMillis(event)

        val metadata = MediaMetadataCompat.Builder()
            .putString(
                MediaMetadataCompat.METADATA_KEY_TITLE,
                currentTrack?.title ?: event.nowPlaying.session?.title ?: "musicd",
            )
            .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, currentTrack?.artist ?: event.nowPlaying.session?.artist)
            .putString(MediaMetadataCompat.METADATA_KEY_ALBUM, currentTrack?.album ?: event.nowPlaying.session?.album)
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
            ?: event.nowPlaying.session?.title
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

        event.nowPlaying.session?.artist?.takeIf { it.isNotBlank() }?.let {
            return it
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

        val track = event.nowPlaying.currentTrack
        if (track == null) {
            syncDirectLocalPlayback(event)
            return
        }
        val streamUrl = currentTrackStreamUrl(track.id) ?: return
        val nextStreamUrl = nextQueuedTrackStreamUrl(event)
        val currentUri = localPlayer.currentMediaItem?.localConfiguration?.uri?.toString()
        val transportState = event.nowPlaying.session?.transportState.orEmpty()
        val serverPositionMs = serverPlaybackPositionMillis(event)
        val isGroupedLocalPlayback = isGroupedLocalRendererActive()
        updateGroupPlaybackClock(event, track.id, serverPositionMs, isGroupedLocalPlayback)

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
            } else if (
                shouldApplyServerSeek(
                    currentPositionMs = localPlayer.currentPosition,
                    serverPositionMs = serverPositionMs,
                    transportState = transportState,
                    toleranceMs = if (isGroupedLocalPlayback) {
                        GROUP_HARD_SEEK_SYNC_TOLERANCE_MS
                    } else {
                        SERVER_SEEK_SYNC_TOLERANCE_MS
                    },
                    allowPlayingCorrection = false,
                )
            ) {
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

    private fun syncDirectLocalPlayback(event: PlaybackEventDto) {
        val streamUrl = event.nowPlaying.session?.currentTrackUri?.takeIf { it.isNotBlank() } ?: run {
            stopLocalPlayer()
            return
        }
        val transportState = event.nowPlaying.session?.transportState.orEmpty()
        val currentUri = localPlayer.currentMediaItem?.localConfiguration?.uri?.toString()

        suppressLocalPlayerSync = true
        try {
            if (currentUri != streamUrl || localPlayer.mediaItemCount != 1) {
                localPlayer.setMediaItem(
                    MediaItem.Builder()
                        .setMediaId("direct-radio")
                        .setUri(streamUrl)
                        .build(),
                )
                localPlayer.prepare()
                lastAppliedServerSeekPositionMs = null
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

    private fun serverPlaybackPositionMillis(event: PlaybackEventDto): Long? {
        val session = event.nowPlaying.session ?: return null
        val observedPositionMs = session.positionSeconds?.times(1000L) ?: return null
        val durationMs = (session.durationSeconds ?: event.nowPlaying.currentTrack?.durationSeconds)
            ?.times(1000L)
        val elapsedSinceObservationMs = if (
            session.transportState == "PLAYING" &&
            session.lastObservedUnix > 0L &&
            session.serverUnix > 0L
        ) {
            (session.serverUnix - session.lastObservedUnix)
                .coerceIn(0L, MAX_SERVER_PLAYBACK_CLOCK_AGE_SECONDS)
                .times(1000L)
        } else {
            0L
        }
        val targetPositionMs = observedPositionMs + elapsedSinceObservationMs
        return durationMs?.let { targetPositionMs.coerceAtMost(it) } ?: targetPositionMs
    }

    private fun updateGroupPlaybackClock(
        event: PlaybackEventDto,
        trackId: String,
        serverPositionMs: Long?,
        isGroupedLocalPlayback: Boolean,
    ) {
        val session = event.nowPlaying.session
        if (!isGroupedLocalPlayback || session == null || serverPositionMs == null) {
            clearGroupPlaybackSync(resetSpeed = true)
            return
        }
        val nowElapsedMs = SystemClock.elapsedRealtime()
        val durationMs = (session.durationSeconds ?: event.nowPlaying.currentTrack?.durationSeconds)
            ?.times(1000L)
        val smoothedPositionMs = smoothedGroupClockPositionMillis(
            existingClock = groupPlaybackClock,
            trackId = trackId,
            serverPositionMs = serverPositionMs,
            durationMs = durationMs,
            transportState = session.transportState,
            nowElapsedMs = nowElapsedMs,
        )
        groupPlaybackClock = GroupPlaybackClock(
            trackId = trackId,
            positionMs = smoothedPositionMs,
            capturedElapsedMs = nowElapsedMs,
            durationMs = durationMs,
            transportState = session.transportState,
        )
        updateGroupPlaybackSyncLoop()
    }

    private fun updateGroupPlaybackSyncLoop() {
        val clock = groupPlaybackClock
        if (
            !isGroupedLocalRendererActive() ||
            clock == null ||
            clock.transportState != "PLAYING"
        ) {
            clearGroupPlaybackSync(resetSpeed = true)
            return
        }
        if (groupPlaybackSyncJob?.isActive == true) {
            return
        }
        groupPlaybackSyncJob = serviceScope.launch {
            while (isActive && isGroupedLocalRendererActive()) {
                applyGroupPlaybackDriftCorrection()
                delay(GROUP_SYNC_INTERVAL_MS)
            }
        }
    }

    private fun applyGroupPlaybackDriftCorrection() {
        val clock = groupPlaybackClock ?: return
        if (clock.transportState != "PLAYING" || localPlayer.currentMediaItem == null) {
            resetLocalPlaybackSpeed()
            return
        }
        val currentTrackId = localPlayer.currentMediaItem?.mediaId
        if (currentTrackId != clock.trackId) {
            resetLocalPlaybackSpeed()
            resetGroupDriftTracking()
            return
        }
        val nowElapsedMs = SystemClock.elapsedRealtime()
        val clockAgeMs = nowElapsedMs - clock.capturedElapsedMs
        if (clockAgeMs > MAX_SERVER_PLAYBACK_CLOCK_AGE_SECONDS * 1000L) {
            resetLocalPlaybackSpeed()
            resetGroupDriftTracking()
            return
        }

        val targetPositionMs = estimatedGroupPlaybackPositionMillis(clock, nowElapsedMs)
            ?: return
        val currentPositionMs = localPlayer.currentPosition.coerceAtLeast(0L)
        val driftMs = targetPositionMs - currentPositionMs
        val absoluteDriftMs = kotlin.math.abs(driftMs)

        if (!localPlayer.isPlaying) {
            resetLocalPlaybackSpeed()
            resetGroupDriftTracking()
            return
        }

        val driftDirection = when {
            absoluteDriftMs >= GROUP_SPEED_SYNC_TOLERANCE_MS && driftMs > 0L -> 1
            absoluteDriftMs >= GROUP_SPEED_SYNC_TOLERANCE_MS && driftMs < 0L -> -1
            else -> 0
        }
        if (driftDirection == 0) {
            resetGroupDriftTracking()
            if (absoluteDriftMs <= GROUP_SPEED_RESET_TOLERANCE_MS) {
                resetLocalPlaybackSpeed()
            }
            return
        }
        updateGroupDriftTracking(driftDirection)

        if (
            groupDriftSampleCount >= GROUP_HARD_SEEK_MIN_SAMPLES &&
            absoluteDriftMs >= GROUP_HARD_SEEK_SYNC_TOLERANCE_MS &&
            nowElapsedMs - lastGroupHardSeekElapsedMs >= GROUP_HARD_SEEK_COOLDOWN_MS
        ) {
            localPlayer.seekTo(targetPositionMs)
            lastAppliedServerSeekPositionMs = targetPositionMs
            lastGroupHardSeekElapsedMs = nowElapsedMs
            resetLocalPlaybackSpeed()
            resetGroupDriftTracking()
            return
        }

        if (groupDriftSampleCount < GROUP_SPEED_SYNC_MIN_SAMPLES) {
            return
        }

        when (groupDriftDirection) {
            1 -> setLocalPlaybackSpeed(GROUP_SPEED_UP)
            -1 -> setLocalPlaybackSpeed(GROUP_SLOW_DOWN)
        }
    }

    private fun smoothedGroupClockPositionMillis(
        existingClock: GroupPlaybackClock?,
        trackId: String,
        serverPositionMs: Long,
        durationMs: Long?,
        transportState: String,
        nowElapsedMs: Long,
    ): Long {
        if (
            existingClock == null ||
            existingClock.trackId != trackId ||
            existingClock.transportState != transportState ||
            transportState != "PLAYING"
        ) {
            return boundedPlaybackPositionMillis(serverPositionMs, durationMs)
        }
        val estimatedPositionMs = estimatedGroupPlaybackPositionMillis(existingClock, nowElapsedMs)
            ?: return boundedPlaybackPositionMillis(serverPositionMs, durationMs)
        val rebaseDriftMs = serverPositionMs - estimatedPositionMs
        val adjustedPositionMs = when {
            kotlin.math.abs(rebaseDriftMs) <= GROUP_CLOCK_REBASE_IGNORE_MS -> {
                estimatedPositionMs
            }
            kotlin.math.abs(rebaseDriftMs) <= GROUP_CLOCK_REBASE_SMOOTH_MS -> {
                estimatedPositionMs + (rebaseDriftMs / GROUP_CLOCK_REBASE_SMOOTHING_DIVISOR)
            }
            else -> serverPositionMs
        }
        return boundedPlaybackPositionMillis(adjustedPositionMs, durationMs)
    }

    private fun boundedPlaybackPositionMillis(positionMs: Long, durationMs: Long?): Long =
        durationMs
            ?.let { positionMs.coerceIn(0L, it) }
            ?: positionMs.coerceAtLeast(0L)

    private fun updateGroupDriftTracking(driftDirection: Int) {
        if (driftDirection == 0) {
            resetGroupDriftTracking()
            return
        }
        if (driftDirection == groupDriftDirection) {
            groupDriftSampleCount += 1
        } else {
            groupDriftDirection = driftDirection
            groupDriftSampleCount = 1
        }
    }

    private fun resetGroupDriftTracking() {
        groupDriftDirection = 0
        groupDriftSampleCount = 0
    }

    private fun estimatedGroupPlaybackPositionMillis(
        clock: GroupPlaybackClock,
        nowElapsedMs: Long,
    ): Long? {
        val elapsedMs = if (clock.transportState == "PLAYING") {
            (nowElapsedMs - clock.capturedElapsedMs)
                .coerceIn(0L, MAX_SERVER_PLAYBACK_CLOCK_AGE_SECONDS * 1000L)
        } else {
            0L
        }
        val positionMs = clock.positionMs + elapsedMs
        return clock.durationMs?.let { positionMs.coerceAtMost(it) } ?: positionMs
    }

    private fun clearGroupPlaybackSync(resetSpeed: Boolean) {
        groupPlaybackSyncJob?.cancel()
        groupPlaybackSyncJob = null
        groupPlaybackClock = null
        lastGroupHardSeekElapsedMs = 0L
        resetGroupDriftTracking()
        if (resetSpeed) {
            resetLocalPlaybackSpeed()
        }
    }

    private fun setLocalPlaybackSpeed(speed: Float) {
        if (kotlin.math.abs(localPlayer.playbackParameters.speed - speed) > 0.001f) {
            localPlayer.setPlaybackSpeed(speed)
        }
    }

    private fun resetLocalPlaybackSpeed() {
        setLocalPlaybackSpeed(1f)
    }

    private fun stopLocalPlayer() {
        suppressLocalPlayerSync = true
        try {
            clearGroupPlaybackSync(resetSpeed = true)
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
        if (!isLocalRendererActive() || !shouldReportLocalRendererSession() || !localPlayer.isPlaying) {
            localPlaybackReportJob = null
            return
        }
        localPlaybackReportJob = serviceScope.launch {
            while (
                isActive &&
                isLocalRendererActive() &&
                shouldReportLocalRendererSession() &&
                localPlayer.isPlaying
            ) {
                reportLocalSession(force = true)
                delay(5_000)
            }
        }
    }

    private fun reportLocalSession(force: Boolean = false) {
        if (!shouldReportLocalRendererSession() || currentBaseUrl.isBlank() || currentRendererLocation.isBlank()) {
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
            serverPlaybackPositionMillis(event) ?: 0L
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
        toleranceMs: Long = SERVER_SEEK_SYNC_TOLERANCE_MS,
        allowPlayingCorrection: Boolean = false,
    ): Boolean {
        val targetPositionMs = serverPositionMs ?: return false
        if (targetPositionMs <= 0L || localPlayer.currentMediaItem == null) {
            return false
        }
        if (targetPositionMs == lastAppliedServerSeekPositionMs) {
            return false
        }
        if (kotlin.math.abs(currentPositionMs - targetPositionMs) < toleranceMs) {
            return false
        }
        if (allowPlayingCorrection && transportState == "PLAYING") {
            return true
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
        if (!shouldReportLocalRendererSession() || currentBaseUrl.isBlank() || currentRendererLocation.isBlank()) {
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
        runCatching { stopForeground(STOP_FOREGROUND_REMOVE) }
        stopSelf()
    }

    private fun promoteToForeground(notification: Notification): Boolean =
        runCatching {
            startForeground(NOTIFICATION_ID, notification)
        }.isSuccess

    private fun isLocalRendererActive(): Boolean =
        isLocalRendererLocation(currentRendererLocation) ||
            isGroupedLocalRendererActive()

    private fun isGroupedLocalRendererActive(): Boolean =
        !isLocalRendererLocation(currentRendererLocation) &&
            (latestPlaybackEvent
                ?.nowPlaying
                ?.renderer
                ?.group
                ?.members
                ?.any { it.rendererLocation == currentLocalRendererLocation } == true)

    private fun shouldReportLocalRendererSession(): Boolean =
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
        private const val EXTRA_LOCAL_RENDERER_LOCATION = "local_renderer_location"
        private const val EXTRA_SERVER_NAME = "server_name"

        private const val REQUEST_OPEN_APP = 2001
        private const val SERVER_SEEK_SYNC_TOLERANCE_MS = 4_000L
        private const val GROUP_SPEED_SYNC_TOLERANCE_MS = 2_000L
        private const val GROUP_SPEED_RESET_TOLERANCE_MS = 900L
        private const val GROUP_HARD_SEEK_SYNC_TOLERANCE_MS = 12_000L
        private const val GROUP_HARD_SEEK_COOLDOWN_MS = 30_000L
        private const val GROUP_SYNC_INTERVAL_MS = 1_500L
        private const val GROUP_SPEED_UP = 1.015f
        private const val GROUP_SLOW_DOWN = 0.985f
        private const val GROUP_SPEED_SYNC_MIN_SAMPLES = 2
        private const val GROUP_HARD_SEEK_MIN_SAMPLES = 3
        private const val GROUP_CLOCK_REBASE_IGNORE_MS = 500L
        private const val GROUP_CLOCK_REBASE_SMOOTH_MS = 3_000L
        private const val GROUP_CLOCK_REBASE_SMOOTHING_DIVISOR = 4L
        private const val MAX_SERVER_PLAYBACK_CLOCK_AGE_SECONDS = 30L

        fun start(
            context: Context,
            baseUrl: String,
            rendererLocation: String,
            localRendererLocation: String,
            serverName: String?,
        ): Boolean {
            val intent = Intent(context, MusicdPlaybackNotificationService::class.java).apply {
                action = ACTION_START
                putExtra(EXTRA_BASE_URL, baseUrl)
                putExtra(EXTRA_RENDERER_LOCATION, rendererLocation)
                putExtra(EXTRA_LOCAL_RENDERER_LOCATION, localRendererLocation)
                putExtra(EXTRA_SERVER_NAME, serverName)
            }
            return runCatching {
                ContextCompat.startForegroundService(context, intent)
            }.isSuccess
        }

        fun stop(context: Context): Boolean {
            val intent = Intent(context, MusicdPlaybackNotificationService::class.java).apply {
                action = ACTION_STOP_SERVICE
            }
            return runCatching {
                context.startService(intent)
            }.isSuccess
        }

        private fun hasDisplayablePlayback(event: PlaybackEventDto): Boolean =
            event.nowPlaying.currentTrack != null ||
                event.nowPlaying.session?.currentTrackUri?.isNotBlank() == true ||
                event.queue.entries.any { entry ->
                    when (entry.entryStatus.trim().lowercase()) {
                        "", "pending", "queued", "playing" -> true
                        else -> false
                    }
                }

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
