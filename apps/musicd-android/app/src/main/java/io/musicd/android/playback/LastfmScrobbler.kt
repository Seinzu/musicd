package io.musicd.android.playback

import io.musicd.android.data.LastfmRepository
import io.musicd.android.data.LastfmTrackPayload
import io.musicd.android.data.LastfmActivePlay
import io.musicd.android.data.PlaybackEventDto
import io.musicd.android.data.TrackSummaryDto
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

private data class PendingScrobble(
    val signature: String,
    val trackId: String,
    val payload: LastfmTrackPayload,
    val durationSeconds: Long?,
    val startedAtUnix: Long,
    val nowPlayingSent: Boolean = false,
    val scrobbled: Boolean = false,
    val lastPositionSeconds: Long = 0L,
    val lastSeenUnix: Long,
)

class LastfmScrobbler(
    private val repository: LastfmRepository,
    private val scope: CoroutineScope,
    private val clockUnixSeconds: () -> Long = { System.currentTimeMillis() / 1000L },
) {
    private var pending: PendingScrobble? = null

    fun handlePlaybackEvent(event: PlaybackEventDto) {
        if (!repository.loadSettings().isConnected) {
            pending = null
            repository.clearActivePlay()
            return
        }

        val track = event.nowPlaying.currentTrack
        if (track == null || !track.isScrobbleable()) {
            flushPendingIfReady()
            pending = null
            repository.clearActivePlay()
            return
        }

        val session = event.nowPlaying.session
        val nowUnix = clockUnixSeconds()
        val transportState = session?.transportState.orEmpty()
        val positionSeconds = session?.positionSeconds?.coerceAtLeast(0L) ?: 0L
        val durationSeconds = session?.durationSeconds ?: track.durationSeconds
        val signature = listOf(
            event.rendererLocation,
            event.queue.currentEntryId ?: session?.queueEntryId ?: event.queue.session?.queueEntryId,
            track.id,
        ).joinToString("|")

        if (pending?.signature != signature) {
            if (pending?.trackId != track.id) {
                flushPendingIfReady()
            }
            val activePlay = repository.loadActivePlay()
                ?.takeIf { it.isContinuationOf(track.id, positionSeconds, nowUnix) }
            val observedAtUnix = session?.lastObservedUnix
                ?.takeIf { it > 0L }
                ?: session?.serverUnix
                ?: nowUnix
            val startedAtUnix = activePlay?.startedAtUnix ?: (observedAtUnix - positionSeconds)
                .coerceAtLeast(0L)
            pending = PendingScrobble(
                signature = signature,
                trackId = track.id,
                payload = track.toLastfmPayload(durationSeconds, startedAtUnix),
                durationSeconds = durationSeconds,
                startedAtUnix = startedAtUnix,
                scrobbled = activePlay?.scrobbled ?: false,
                lastPositionSeconds = positionSeconds,
                lastSeenUnix = nowUnix,
            )
        }

        val current = pending ?: return
        val updated = current.copy(
            lastPositionSeconds = maxOf(current.lastPositionSeconds, positionSeconds),
            lastSeenUnix = nowUnix,
        )
        pending = updated
        repository.saveActivePlay(updated.toActivePlay())

        if (transportState == "PLAYING" || transportState == "TRANSITIONING") {
            sendNowPlayingIfNeeded(updated)
        }
        scrobbleIfReady(updated)
    }

    private fun sendNowPlayingIfNeeded(item: PendingScrobble) {
        if (item.nowPlayingSent) return
        pending = item.copy(nowPlayingSent = true)
        scope.launch {
            runCatching { repository.updateNowPlaying(item.payload) }
        }
    }

    private fun scrobbleIfReady(item: PendingScrobble) {
        if (item.scrobbled || !item.hasReachedScrobbleThreshold()) return
        if (repository.hasRecentScrobble(item.trackId, item.startedAtUnix)) {
            pending = item.copy(scrobbled = true)
            repository.saveActivePlay(item.copy(scrobbled = true).toActivePlay())
            return
        }
        repository.rememberScrobble(item.trackId, item.startedAtUnix)
        pending = item.copy(scrobbled = true)
        repository.saveActivePlay(item.copy(scrobbled = true).toActivePlay())
        scope.launch {
            runCatching { repository.scrobble(item.payload.copy(timestampUnix = item.startedAtUnix)) }
        }
    }

    private fun flushPendingIfReady() {
        pending?.let { scrobbleIfReady(it) }
    }

    private fun PendingScrobble.hasReachedScrobbleThreshold(): Boolean {
        val threshold = durationSeconds
            ?.let { duration -> minOf(duration / 2L, SCROBBLE_MAX_THRESHOLD_SECONDS) }
            ?: SCROBBLE_MAX_THRESHOLD_SECONDS
        return lastPositionSeconds >= threshold
    }

    private fun PendingScrobble.toActivePlay(): LastfmActivePlay =
        LastfmActivePlay(
            trackId = trackId,
            startedAtUnix = startedAtUnix,
            lastPositionSeconds = lastPositionSeconds,
            scrobbled = scrobbled,
            lastSeenUnix = lastSeenUnix,
        )

    private fun LastfmActivePlay.isContinuationOf(
        trackId: String,
        positionSeconds: Long,
        nowUnix: Long,
    ): Boolean =
        this.trackId == trackId &&
            nowUnix - lastSeenUnix <= ACTIVE_PLAY_MAX_GAP_SECONDS &&
            positionSeconds + ACTIVE_PLAY_POSITION_RESET_TOLERANCE_SECONDS >= lastPositionSeconds

    private fun TrackSummaryDto.isScrobbleable(): Boolean =
        title.isNotBlank() &&
            artist.isNotBlank() &&
            durationSeconds?.let { it >= LASTFM_MIN_TRACK_DURATION_SECONDS } != false

    private fun TrackSummaryDto.toLastfmPayload(
        durationSeconds: Long?,
        startedAtUnix: Long,
    ): LastfmTrackPayload =
        LastfmTrackPayload(
            artist = artist,
            track = title,
            album = album.takeIf { it.isNotBlank() },
            durationSeconds = durationSeconds,
            timestampUnix = startedAtUnix,
        )

    companion object {
        private const val LASTFM_MIN_TRACK_DURATION_SECONDS = 30L
        private const val SCROBBLE_MAX_THRESHOLD_SECONDS = 240L
        private const val ACTIVE_PLAY_MAX_GAP_SECONDS = 12 * 60 * 60L
        private const val ACTIVE_PLAY_POSITION_RESET_TOLERANCE_SECONDS = 15L
    }
}
