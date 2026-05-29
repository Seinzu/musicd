package io.musicd.android.data

import android.content.Context
import androidx.core.content.edit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

data class LastfmSettings(
    val apiKey: String = "",
    val sharedSecret: String = "",
    val sessionKey: String = "",
    val username: String = "",
    val pendingToken: String = "",
) {
    val hasAppCredentials: Boolean
        get() = apiKey.isNotBlank() && sharedSecret.isNotBlank()

    val isConnected: Boolean
        get() = hasAppCredentials && sessionKey.isNotBlank()
}

data class LastfmActivePlay(
    val trackId: String,
    val startedAtUnix: Long,
    val lastPositionSeconds: Long,
    val scrobbled: Boolean,
    val lastSeenUnix: Long,
)

private fun String.encodeForLastfmUrl(): String =
    java.net.URLEncoder.encode(this, Charsets.UTF_8.name())

class LastfmRepository(
    context: Context,
    private val api: LastfmApi = LastfmApi(),
) {
    private val prefs = context.getSharedPreferences("musicd_android", Context.MODE_PRIVATE)

    fun loadSettings(): LastfmSettings =
        LastfmSettings(
            apiKey = prefs.getString(KEY_API_KEY, "").orEmpty(),
            sharedSecret = prefs.getString(KEY_SHARED_SECRET, "").orEmpty(),
            sessionKey = prefs.getString(KEY_SESSION_KEY, "").orEmpty(),
            username = prefs.getString(KEY_USERNAME, "").orEmpty(),
            pendingToken = prefs.getString(KEY_PENDING_TOKEN, "").orEmpty(),
        )

    fun saveAppCredentials(apiKey: String, sharedSecret: String) {
        prefs.edit {
            putString(KEY_API_KEY, apiKey.trim())
            putString(KEY_SHARED_SECRET, sharedSecret.trim())
        }
    }

    fun disconnect() {
        prefs.edit {
            remove(KEY_SESSION_KEY)
            remove(KEY_USERNAME)
            remove(KEY_PENDING_TOKEN)
        }
    }

    suspend fun beginAuthentication(): String = withContext(Dispatchers.IO) {
        val settings = loadSettings()
        if (!settings.hasAppCredentials) {
            throw LastfmApiException("Enter a Last.fm API key and shared secret first.")
        }
        val token = api.requestToken(settings.apiKey, settings.sharedSecret)
        prefs.edit { putString(KEY_PENDING_TOKEN, token) }
        token
    }

    suspend fun completeAuthentication(): LastfmSessionDto = withContext(Dispatchers.IO) {
        val settings = loadSettings()
        if (!settings.hasAppCredentials) {
            throw LastfmApiException("Enter a Last.fm API key and shared secret first.")
        }
        val token = settings.pendingToken.takeIf { it.isNotBlank() }
            ?: throw LastfmApiException("Start Last.fm sign-in first.")
        val session = api.getSession(settings.apiKey, settings.sharedSecret, token)
        prefs.edit {
            putString(KEY_SESSION_KEY, session.key)
            putString(KEY_USERNAME, session.name)
            remove(KEY_PENDING_TOKEN)
        }
        session
    }

    suspend fun updateNowPlaying(track: LastfmTrackPayload) = withContext(Dispatchers.IO) {
        val settings = loadSettings()
        if (!settings.isConnected) return@withContext
        api.updateNowPlaying(
            apiKey = settings.apiKey,
            sharedSecret = settings.sharedSecret,
            sessionKey = settings.sessionKey,
            track = track,
        )
    }

    suspend fun scrobble(track: LastfmTrackPayload) = withContext(Dispatchers.IO) {
        val settings = loadSettings()
        if (!settings.isConnected) return@withContext
        api.scrobble(
            apiKey = settings.apiKey,
            sharedSecret = settings.sharedSecret,
            sessionKey = settings.sessionKey,
            track = track,
        )
    }

    fun hasRecentScrobble(trackId: String, startedAtUnix: Long): Boolean =
        synchronized(LastfmRepository::class.java) {
            loadRecentScrobbles().any { recent ->
                recent.trackId == trackId &&
                    kotlin.math.abs(recent.startedAtUnix - startedAtUnix) <= RECENT_SCROBBLE_TOLERANCE_SECONDS
            }
        }

    fun rememberScrobble(trackId: String, startedAtUnix: Long) {
        synchronized(LastfmRepository::class.java) {
            val cutoff = (System.currentTimeMillis() / 1000L) - RECENT_SCROBBLE_MAX_AGE_SECONDS
            val updated = (loadRecentScrobbles()
                .filter { it.startedAtUnix >= cutoff } + RecentScrobble(trackId, startedAtUnix))
                .takeLast(RECENT_SCROBBLE_LIMIT)
            prefs.edit {
                putString(
                    KEY_RECENT_SCROBBLES,
                    updated.joinToString("\n") { "${it.startedAtUnix}\t${it.trackId}" },
                )
            }
        }
    }

    fun loadActivePlay(): LastfmActivePlay? =
        synchronized(LastfmRepository::class.java) {
            val trackId = prefs.getString(KEY_ACTIVE_TRACK_ID, "").orEmpty()
                .takeIf { it.isNotBlank() }
                ?: return@synchronized null
            val startedAtUnix = prefs.getLong(KEY_ACTIVE_STARTED_AT_UNIX, 0L)
                .takeIf { it > 0L }
                ?: return@synchronized null
            LastfmActivePlay(
                trackId = trackId,
                startedAtUnix = startedAtUnix,
                lastPositionSeconds = prefs.getLong(KEY_ACTIVE_LAST_POSITION_SECONDS, 0L).coerceAtLeast(0L),
                scrobbled = prefs.getBoolean(KEY_ACTIVE_SCROBBLED, false),
                lastSeenUnix = prefs.getLong(KEY_ACTIVE_LAST_SEEN_UNIX, 0L).coerceAtLeast(0L),
            )
        }

    fun saveActivePlay(activePlay: LastfmActivePlay) {
        synchronized(LastfmRepository::class.java) {
            prefs.edit {
                putString(KEY_ACTIVE_TRACK_ID, activePlay.trackId)
                putLong(KEY_ACTIVE_STARTED_AT_UNIX, activePlay.startedAtUnix)
                putLong(KEY_ACTIVE_LAST_POSITION_SECONDS, activePlay.lastPositionSeconds.coerceAtLeast(0L))
                putBoolean(KEY_ACTIVE_SCROBBLED, activePlay.scrobbled)
                putLong(KEY_ACTIVE_LAST_SEEN_UNIX, activePlay.lastSeenUnix)
            }
        }
    }

    fun clearActivePlay() {
        synchronized(LastfmRepository::class.java) {
            prefs.edit {
                remove(KEY_ACTIVE_TRACK_ID)
                remove(KEY_ACTIVE_STARTED_AT_UNIX)
                remove(KEY_ACTIVE_LAST_POSITION_SECONDS)
                remove(KEY_ACTIVE_SCROBBLED)
                remove(KEY_ACTIVE_LAST_SEEN_UNIX)
            }
        }
    }

    private fun loadRecentScrobbles(): List<RecentScrobble> =
        prefs.getString(KEY_RECENT_SCROBBLES, "").orEmpty()
            .lineSequence()
            .mapNotNull { line ->
                val separatorIndex = line.indexOf('\t')
                if (separatorIndex <= 0 || separatorIndex == line.lastIndex) {
                    null
                } else {
                    val startedAtUnix = line.substring(0, separatorIndex).toLongOrNull()
                    val trackId = line.substring(separatorIndex + 1).takeIf { it.isNotBlank() }
                    if (startedAtUnix != null && trackId != null) {
                        RecentScrobble(trackId, startedAtUnix)
                    } else {
                        null
                    }
                }
            }
            .toList()

    companion object {
        private const val KEY_API_KEY = "lastfm_api_key"
        private const val KEY_SHARED_SECRET = "lastfm_shared_secret"
        private const val KEY_SESSION_KEY = "lastfm_session_key"
        private const val KEY_USERNAME = "lastfm_username"
        private const val KEY_PENDING_TOKEN = "lastfm_pending_token"
        private const val KEY_RECENT_SCROBBLES = "lastfm_recent_scrobbles"
        private const val KEY_ACTIVE_TRACK_ID = "lastfm_active_track_id"
        private const val KEY_ACTIVE_STARTED_AT_UNIX = "lastfm_active_started_at_unix"
        private const val KEY_ACTIVE_LAST_POSITION_SECONDS = "lastfm_active_last_position_seconds"
        private const val KEY_ACTIVE_SCROBBLED = "lastfm_active_scrobbled"
        private const val KEY_ACTIVE_LAST_SEEN_UNIX = "lastfm_active_last_seen_unix"
        private const val RECENT_SCROBBLE_LIMIT = 20
        private const val RECENT_SCROBBLE_TOLERANCE_SECONDS = 180L
        private const val RECENT_SCROBBLE_MAX_AGE_SECONDS = 24 * 60 * 60L

        fun authUrl(apiKey: String, token: String): String =
            "https://www.last.fm/api/auth/?api_key=${apiKey.encodeForLastfmUrl()}&token=${token.encodeForLastfmUrl()}"
    }
}

private data class RecentScrobble(
    val trackId: String,
    val startedAtUnix: Long,
)
