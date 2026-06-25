package io.musicd.android.companion

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Build
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import androidx.core.app.NotificationCompat
import androidx.media.app.NotificationCompat.MediaStyle
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.exoplayer.ExoPlayer
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import kotlin.math.max

class CompanionPlaybackRuntime(private val service: Service) {
    private val database = CompanionDatabase.get(service)
    private val player = ExoPlayer.Builder(service).build()
    private val notificationManager = service.getSystemService(NotificationManager::class.java)
    private var isForeground = false
    private var positionReporterJob: Job? = null
    private var suppressNextIdlePersistence = false
    private var requestedTransportState = "STOPPED"
    private var latestArtworkPath: String? = null
    private var latestArtworkBitmap: Bitmap? = null
    private val coroutineExceptionHandler = CoroutineExceptionHandler { _, _ -> }
    private val mediaSession = MediaSessionCompat(service, "musicd-companion").apply {
        setCallback(
            object : MediaSessionCompat.Callback() {
                override fun onPlay() {
                    scope.launch { playCurrent() }
                }

                override fun onPause() {
                    scope.launch { pause() }
                }

                override fun onStop() {
                    scope.launch { stop() }
                }

                override fun onSkipToNext() {
                    scope.launch { next() }
                }

                override fun onSkipToPrevious() {
                    scope.launch { previous() }
                }
            },
        )
        isActive = true
    }
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate + coroutineExceptionHandler)

    init {
        ensureNotificationChannel()
        player.addListener(
            object : Player.Listener {
                override fun onPlaybackStateChanged(playbackState: Int) {
                    if (playbackState == Player.STATE_ENDED) {
                        scope.launch { completeCurrentAndAdvance() }
                    } else if (playbackState == Player.STATE_IDLE && suppressNextIdlePersistence) {
                        suppressNextIdlePersistence = false
                    } else {
                        persistObservedPlayerState()
                    }
                }

                override fun onIsPlayingChanged(isPlaying: Boolean) {
                    persistObservedPlayerState()
                }

                override fun onPlayerError(error: PlaybackException) {
                    scope.launch { handlePlaybackError(error) }
                }
            },
        )
    }

    fun startControlSurface() {
        scope.launch {
            if (hasActivePlayer()) {
                refreshActiveControlSurface()
            } else {
                restoreControlSurface()
            }
        }
    }

    fun ensureForegroundStarted(title: String = "Ready") {
        promoteOrUpdateNotification(
            buildNotification(
                title = title,
                isPlaying = false,
                ongoing = true,
                artworkBitmap = latestArtworkBitmap,
            ),
        )
    }

    suspend fun playCurrent() {
        val current = database.queue().currentEntry()
            ?: database.queue().firstPlayableEntry()
            ?: return
        val restoredPositionSeconds = database.queue().session()
            ?.takeIf { it.queueEntryId == current.id }
            ?.positionSeconds
            ?.takeIf { it > 0L }
        startEntry(current, restoredPositionSeconds)
    }

    suspend fun pause() {
        withContext(Dispatchers.Main.immediate) {
            requestedTransportState = "PAUSED_PLAYBACK"
            player.pause()
            promoteOrUpdateNotification(
                buildNotification(
                    title = "Paused",
                    isPlaying = false,
                    ongoing = true,
                    artworkBitmap = latestArtworkBitmap,
                ),
            )
        }
        stopPositionReporter()
        persistSession("PAUSED_PLAYBACK")
    }

    suspend fun stop() {
        withContext(Dispatchers.Main.immediate) {
            requestedTransportState = "STOPPED"
            player.stop()
            player.clearMediaItems()
            promoteOrUpdateNotification(buildNotification("Stopped", isPlaying = false, ongoing = true))
        }
        stopPositionReporter()
        database.queue().clearPlayingStatus()
        persistSession("STOPPED")
    }

    suspend fun next() {
        val current = database.queue().currentEntry()
            ?: database.queue().session()?.queueEntryId?.let { database.queue().entry(it) }
        val next = current?.let { database.queue().nextEntryAfter(it.position) }
            ?: database.queue().firstPlayableEntry()
        if (next == null) {
            stop()
        } else {
            startEntry(next)
        }
    }

    suspend fun previous() {
        val entries = database.queue().entries()
        val current = database.queue().currentEntry()
        val previous = if (current == null) {
            entries.firstOrNull()
        } else {
            entries.lastOrNull { it.position < current.position } ?: current
        }
        if (previous != null) startEntry(previous)
    }

    suspend fun startEntry(entry: QueueEntryEntity, positionSeconds: Long? = null) {
        val track = database.library().track(entry.trackId) ?: return
        val artworkBitmap = loadArtworkBitmap(track)
        database.queue().clearPlayingStatus()
        database.queue().updateStatus(entry.id, "playing")
        withContext(Dispatchers.Main.immediate) {
            requestedTransportState = "PLAYING"
            updateMediaSession(track, "PLAYING", positionSeconds ?: 0L, artworkBitmap)
            promoteOrUpdateNotification(
                buildNotification(
                    title = track.title,
                    isPlaying = true,
                    ongoing = true,
                    artworkBitmap = artworkBitmap,
                ),
            )
            player.setMediaItem(
                MediaItem.Builder()
                    .setMediaId(track.id)
                    .setUri(Uri.parse(track.contentUri))
                    .build(),
            )
            player.prepare()
            positionSeconds?.takeIf { it > 0L }?.let { player.seekTo(it * 1000L) }
            player.play()
        }
        persistSession("PLAYING", entry, track)
        startPositionReporter(entry, track)
    }

    fun release() {
        scope.cancel()
        mediaSession.release()
        player.release()
    }

    private suspend fun persistSession(
        transportState: String,
        entry: QueueEntryEntity? = null,
        track: TrackEntity? = null,
    ) {
        val resolvedEntry = entry ?: database.queue().currentEntry()
        val resolvedTrack = track ?: resolvedEntry?.let { database.library().track(it.trackId) }
        val playerSnapshot = withContext(Dispatchers.Main.immediate) {
            player.currentPosition.takeIf { it >= 0 }?.div(1000) to
                player.duration.takeIf { it > 0 }?.div(1000)
        }
        val positionSeconds = playerSnapshot.first
        val durationSeconds = playerSnapshot.second ?: resolvedTrack?.durationSeconds
        database.queue().upsertSession(
            PlaybackSessionEntity(
                transportState = transportState,
                queueEntryId = resolvedEntry?.id,
                currentTrackUri = resolvedTrack?.contentUri,
                positionSeconds = positionSeconds,
                durationSeconds = durationSeconds,
                lastObservedUnix = nowUnix(),
            )
        )
        withContext(Dispatchers.Main.immediate) {
            updatePlaybackState(transportState, positionSeconds ?: 0L)
        }
    }

    private fun persistObservedPlayerState() {
        val transportState = when {
            player.playbackState == Player.STATE_ENDED -> null
            requestedTransportState == "PLAYING" -> "PLAYING"
            requestedTransportState == "TRANSITIONING" -> "TRANSITIONING"
            requestedTransportState == "PAUSED_PLAYBACK" -> "PAUSED_PLAYBACK"
            requestedTransportState == "STOPPED" && player.playbackState == Player.STATE_IDLE -> "STOPPED"
            else -> null
        }
        if (transportState != null) {
            scope.launch { persistSession(transportState) }
        }
    }

    private suspend fun completeCurrentAndAdvance() {
        val current = database.queue().currentEntry()
            ?: database.queue().session()?.queueEntryId?.let { database.queue().entry(it) }
            ?: return
        val track = database.library().track(current.trackId)
        val artworkBitmap = loadArtworkBitmap(track)
        stopPositionReporter()
        requestedTransportState = "COMPLETED"
        persistSession("COMPLETED", current, track)
        database.queue().updateStatus(current.id, "completed")

        val next = database.queue().nextEntryAfter(current.position)
        if (next == null) {
            withContext(Dispatchers.Main.immediate) {
                suppressNextIdlePersistence = true
                requestedTransportState = "STOPPED"
                player.stop()
                player.clearMediaItems()
                updatePlaybackState("COMPLETED", track?.durationSeconds ?: 0L)
                promoteOrUpdateNotification(
                    buildNotification(
                        title = track?.let { "Finished: ${it.title}" } ?: "Playback finished",
                        isPlaying = false,
                        ongoing = true,
                        artworkBitmap = artworkBitmap,
                    ),
                )
            }
            return
        }

        requestedTransportState = "TRANSITIONING"
        persistSession("TRANSITIONING", current, track)
        delay(AUTO_ADVANCE_DELAY_MS)
        startEntry(next)
    }

    private suspend fun hasActivePlayer(): Boolean =
        withContext(Dispatchers.Main.immediate) {
            player.mediaItemCount > 0 ||
                player.playbackState != Player.STATE_IDLE ||
                player.playWhenReady ||
                player.isPlaying
        }

    private suspend fun refreshActiveControlSurface() {
        val session = database.queue().session()
        val entry = session?.queueEntryId?.let { database.queue().entry(it) } ?: database.queue().currentEntry()
        val track = entry?.let { database.library().track(it.trackId) }
        if (track == null) return
        val artworkBitmap = loadArtworkBitmap(track)

        val transportState = withContext(Dispatchers.Main.immediate) {
            when {
                player.isPlaying || player.playWhenReady -> "PLAYING"
                requestedTransportState == "PAUSED_PLAYBACK" -> "PAUSED_PLAYBACK"
                requestedTransportState == "TRANSITIONING" -> "TRANSITIONING"
                else -> session?.transportState ?: requestedTransportState
            }
        }
        withContext(Dispatchers.Main.immediate) {
            requestedTransportState = transportState
            updateMediaSession(track, transportState, session?.positionSeconds ?: 0L, artworkBitmap)
            promoteOrUpdateNotification(
                buildNotification(
                    title = if (transportState == "PAUSED_PLAYBACK") "Paused: ${track.title}" else track.title,
                    isPlaying = transportState == "PLAYING" || transportState == "TRANSITIONING",
                    ongoing = true,
                    artworkBitmap = artworkBitmap,
                ),
            )
        }
        if (session?.transportState != transportState) {
            persistSession(transportState, entry, track)
        }
    }

    private suspend fun restoreControlSurface() {
        val session = database.queue().session()
        val entry = session?.queueEntryId?.let { database.queue().entry(it) } ?: database.queue().currentEntry()
        val track = entry?.let { database.library().track(it.trackId) }
        if (session == null || entry == null || track == null) {
            withContext(Dispatchers.Main.immediate) {
                promoteOrUpdateNotification(buildNotification("Ready", isPlaying = false, ongoing = true))
            }
            return
        }
        val artworkBitmap = loadArtworkBitmap(track)

        val restoredState = when (session.transportState) {
            "PLAYING", "TRANSITIONING" -> "PAUSED_PLAYBACK"
            else -> session.transportState
        }
        if (restoredState != session.transportState) {
            database.queue().upsertSession(
                session.copy(
                    transportState = restoredState,
                    lastObservedUnix = nowUnix(),
                ),
            )
        }
        withContext(Dispatchers.Main.immediate) {
            requestedTransportState = restoredState
            updateMediaSession(track, restoredState, session.positionSeconds ?: 0L, artworkBitmap)
            promoteOrUpdateNotification(
                buildNotification(
                    title = if (restoredState == "PAUSED_PLAYBACK") "Paused: ${track.title}" else "Ready",
                    isPlaying = false,
                    ongoing = true,
                    artworkBitmap = artworkBitmap,
                ),
            )
        }
    }

    private suspend fun handlePlaybackError(error: PlaybackException) {
        withContext(Dispatchers.Main.immediate) {
            requestedTransportState = "STOPPED"
            player.stop()
            player.clearMediaItems()
            promoteOrUpdateNotification(
                buildNotification(
                    title = error.message ?: "Playback error",
                    isPlaying = false,
                    ongoing = true,
                ),
            )
        }
        stopPositionReporter()
        database.queue().clearPlayingStatus()
        persistSession("STOPPED")
    }

    private fun startPositionReporter(entry: QueueEntryEntity, track: TrackEntity) {
        stopPositionReporter()
        positionReporterJob = scope.launch {
            while (isActive) {
                delay(POSITION_REPORT_INTERVAL_MS)
                if (withContext(Dispatchers.Main.immediate) { player.isPlaying }) {
                    persistSession("PLAYING", entry, track)
                }
            }
        }
    }

    private fun stopPositionReporter() {
        positionReporterJob?.cancel()
        positionReporterJob = null
    }

    private fun buildNotification(
        title: String,
        isPlaying: Boolean,
        ongoing: Boolean,
        artworkBitmap: Bitmap? = null,
    ): Notification {
        val playPauseAction = if (isPlaying) {
            NotificationCompat.Action(
                android.R.drawable.ic_media_pause,
                "Pause",
                serviceIntent(MusicdCompanionService.ACTION_PAUSE),
            )
        } else {
            NotificationCompat.Action(
                android.R.drawable.ic_media_play,
                "Play",
                serviceIntent(MusicdCompanionService.ACTION_PLAY),
            )
        }

        return NotificationCompat.Builder(service, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_stat_musicd)
            .setContentTitle("feltsloth Companion")
            .setContentText(title)
            .setContentIntent(activityIntent())
            .setCategory(NotificationCompat.CATEGORY_TRANSPORT)
            .setOnlyAlertOnce(true)
            .setOngoing(ongoing)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
            .setLargeIcon(artworkBitmap)
            .addAction(android.R.drawable.ic_media_previous, "Previous", serviceIntent(MusicdCompanionService.ACTION_PREVIOUS))
            .addAction(playPauseAction)
            .addAction(android.R.drawable.ic_media_next, "Next", serviceIntent(MusicdCompanionService.ACTION_NEXT))
            .addAction(android.R.drawable.ic_menu_close_clear_cancel, "Stop", serviceIntent(MusicdCompanionService.ACTION_STOP))
            .setStyle(
                MediaStyle()
                    .setMediaSession(mediaSession.sessionToken)
                    .setShowActionsInCompactView(0, 1, 2),
            )
            .build()
    }

    private fun activityIntent(): PendingIntent {
        val flags = PendingIntent.FLAG_UPDATE_CURRENT or
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) PendingIntent.FLAG_IMMUTABLE else 0
        return PendingIntent.getActivity(
            service,
            0,
            Intent(service, MainActivity::class.java),
            flags,
        )
    }

    private fun promoteOrUpdateNotification(notification: Notification) {
        if (isForeground) {
            updateNotification(notification)
            return
        }

        runCatching {
            service.startForeground(NOTIFICATION_ID, notification)
            isForeground = true
        }.onFailure { error ->
            if (error is IllegalStateException || error is SecurityException) {
                updateNotification(notification)
            } else {
                throw error
            }
        }
    }

    private fun updateNotification(notification: Notification) {
        runCatching {
            notificationManager.notify(NOTIFICATION_ID, notification)
        }
    }

    private fun serviceIntent(action: String): PendingIntent {
        val flags = PendingIntent.FLAG_UPDATE_CURRENT or
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) PendingIntent.FLAG_IMMUTABLE else 0
        return PendingIntent.getService(
            service,
            action.hashCode(),
            Intent(service, MusicdCompanionService::class.java).setAction(action),
            flags,
        )
    }

    private fun updateMediaSession(
        track: TrackEntity,
        transportState: String,
        positionSeconds: Long = 0L,
        artworkBitmap: Bitmap? = null,
    ) {
        val metadata = MediaMetadataCompat.Builder()
            .putString(MediaMetadataCompat.METADATA_KEY_TITLE, track.title)
            .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, track.artist)
            .putString(MediaMetadataCompat.METADATA_KEY_ALBUM, track.album)
            .putLong(MediaMetadataCompat.METADATA_KEY_DURATION, (track.durationSeconds ?: 0L) * 1000L)
        artworkBitmap?.let { bitmap ->
            metadata.putBitmap(MediaMetadataCompat.METADATA_KEY_ALBUM_ART, bitmap)
            metadata.putBitmap(MediaMetadataCompat.METADATA_KEY_ART, bitmap)
            metadata.putBitmap(MediaMetadataCompat.METADATA_KEY_DISPLAY_ICON, bitmap)
        }
        mediaSession.setMetadata(metadata.build())
        updatePlaybackState(transportState, positionSeconds)
    }

    private suspend fun loadArtworkBitmap(track: TrackEntity?): Bitmap? {
        val artworkPath = track?.artworkPath?.takeIf { it.isNotBlank() }
        if (artworkPath == null) {
            latestArtworkPath = null
            latestArtworkBitmap = null
            return null
        }
        if (artworkPath == latestArtworkPath && latestArtworkBitmap != null) {
            return latestArtworkBitmap
        }

        val bitmap = withContext(Dispatchers.IO) { decodeArtworkBitmap(artworkPath) }
        latestArtworkPath = artworkPath
        latestArtworkBitmap = bitmap
        return bitmap
    }

    private fun decodeArtworkBitmap(artworkPath: String): Bitmap? {
        val file = File(File(service.filesDir, "artwork"), artworkPath).takeIf { it.isFile } ?: return null
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.absolutePath, bounds)
        val sampleSize = bitmapSampleSize(bounds.outWidth, bounds.outHeight, MAX_NOTIFICATION_ARTWORK_SIZE)
        return BitmapFactory.decodeFile(
            file.absolutePath,
            BitmapFactory.Options().apply { inSampleSize = sampleSize },
        )
    }

    private fun bitmapSampleSize(width: Int, height: Int, maxSize: Int): Int {
        if (width <= 0 || height <= 0) return 1
        var sampleSize = 1
        while (max(width, height) / sampleSize > maxSize) {
            sampleSize *= 2
        }
        return sampleSize
    }

    private fun updatePlaybackState(transportState: String, positionSeconds: Long) {
        val state = when (transportState) {
            "PLAYING" -> PlaybackStateCompat.STATE_PLAYING
            "PAUSED_PLAYBACK" -> PlaybackStateCompat.STATE_PAUSED
            "TRANSITIONING" -> PlaybackStateCompat.STATE_BUFFERING
            "STOPPED" -> PlaybackStateCompat.STATE_STOPPED
            "COMPLETED" -> PlaybackStateCompat.STATE_SKIPPING_TO_NEXT
            else -> PlaybackStateCompat.STATE_NONE
        }
        val actions = PlaybackStateCompat.ACTION_PLAY or
            PlaybackStateCompat.ACTION_PAUSE or
            PlaybackStateCompat.ACTION_STOP or
            PlaybackStateCompat.ACTION_SKIP_TO_NEXT or
            PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS
        mediaSession.setPlaybackState(
            PlaybackStateCompat.Builder()
                .setActions(actions)
                .setState(state, positionSeconds * 1000L, if (state == PlaybackStateCompat.STATE_PLAYING) 1f else 0f)
                .build(),
        )
    }

    private fun ensureNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = service.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_ID,
                "feltsloth Companion playback",
                NotificationManager.IMPORTANCE_LOW,
            ),
        )
    }

    companion object {
        private const val CHANNEL_ID = "musicd_companion_playback"
        private const val NOTIFICATION_ID = 2001
        private const val POSITION_REPORT_INTERVAL_MS = 1_000L
        private const val AUTO_ADVANCE_DELAY_MS = 250L
        private const val MAX_NOTIFICATION_ARTWORK_SIZE = 512
    }
}
