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

    companion object {
        private const val KEY_API_KEY = "lastfm_api_key"
        private const val KEY_SHARED_SECRET = "lastfm_shared_secret"
        private const val KEY_SESSION_KEY = "lastfm_session_key"
        private const val KEY_USERNAME = "lastfm_username"
        private const val KEY_PENDING_TOKEN = "lastfm_pending_token"

        fun authUrl(apiKey: String, token: String): String =
            "https://www.last.fm/api/auth/?api_key=${apiKey.encodeForLastfmUrl()}&token=${token.encodeForLastfmUrl()}"
    }
}
