package io.musicd.android.data

import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.FormBody
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.IOException
import java.security.MessageDigest

@Serializable
data class LastfmTokenResponseDto(
    val token: String,
)

@Serializable
data class LastfmSessionEnvelopeDto(
    val session: LastfmSessionDto,
)

@Serializable
data class LastfmSessionDto(
    val name: String,
    val key: String,
)

data class LastfmTrackPayload(
    val artist: String,
    val track: String,
    val album: String?,
    val durationSeconds: Long?,
    val timestampUnix: Long? = null,
)

class LastfmApiException(
    val userMessage: String,
    cause: Throwable? = null,
) : IOException(userMessage, cause)

class LastfmApi(
    private val client: OkHttpClient = OkHttpClient(),
    private val json: Json = Json {
        ignoreUnknownKeys = true
        coerceInputValues = true
    },
) {
    fun requestToken(apiKey: String, sharedSecret: String): String {
        val response = post(
            signedParams(
                sharedSecret = sharedSecret,
                "method" to "auth.getToken",
                "api_key" to apiKey,
            ),
        )
        return decode<LastfmTokenResponseDto>(response).token
    }

    fun getSession(apiKey: String, sharedSecret: String, token: String): LastfmSessionDto {
        val response = post(
            signedParams(
                sharedSecret = sharedSecret,
                "method" to "auth.getSession",
                "api_key" to apiKey,
                "token" to token,
            ),
        )
        return decode<LastfmSessionEnvelopeDto>(response).session
    }

    fun updateNowPlaying(
        apiKey: String,
        sharedSecret: String,
        sessionKey: String,
        track: LastfmTrackPayload,
    ) {
        postTrack(
            sharedSecret = sharedSecret,
            params = buildTrackParams(
                method = "track.updateNowPlaying",
                apiKey = apiKey,
                sessionKey = sessionKey,
                track = track,
                includeTimestamp = false,
            ),
        )
    }

    fun scrobble(
        apiKey: String,
        sharedSecret: String,
        sessionKey: String,
        track: LastfmTrackPayload,
    ) {
        postTrack(
            sharedSecret = sharedSecret,
            params = buildTrackParams(
                method = "track.scrobble",
                apiKey = apiKey,
                sessionKey = sessionKey,
                track = track,
                includeTimestamp = true,
            ),
        )
    }

    private fun postTrack(sharedSecret: String, params: Map<String, String>) {
        post(signedParams(sharedSecret, *params.entries.map { it.key to it.value }.toTypedArray()))
    }

    private fun buildTrackParams(
        method: String,
        apiKey: String,
        sessionKey: String,
        track: LastfmTrackPayload,
        includeTimestamp: Boolean,
    ): Map<String, String> = buildMap {
        put("method", method)
        put("api_key", apiKey)
        put("sk", sessionKey)
        put("artist", track.artist)
        put("track", track.track)
        track.album?.takeIf { it.isNotBlank() }?.let { put("album", it) }
        track.durationSeconds?.takeIf { it > 0L }?.let { put("duration", it.toString()) }
        if (includeTimestamp) {
            put("timestamp", (track.timestampUnix ?: (System.currentTimeMillis() / 1000L)).toString())
        }
    }

    private fun signedParams(
        sharedSecret: String,
        vararg params: Pair<String, String>,
    ): Map<String, String> {
        val baseParams = params.toMap()
        return baseParams + mapOf(
            "api_sig" to apiSignature(baseParams, sharedSecret),
            "format" to "json",
        )
    }

    private fun apiSignature(params: Map<String, String>, sharedSecret: String): String {
        val source = buildString {
            params
                .filterKeys { it != "format" && it != "callback" }
                .toSortedMap()
                .forEach { (key, value) ->
                    append(key)
                    append(value)
                }
            append(sharedSecret)
        }
        return md5(source)
    }

    private fun post(params: Map<String, String>): String {
        val bodyBuilder = FormBody.Builder()
        params.forEach { (key, value) -> bodyBuilder.add(key, value) }
        val request = Request.Builder()
            .url(API_ROOT)
            .post(bodyBuilder.build())
            .build()
        val response = runCatching {
            client.newCall(request).execute()
        }.getOrElse { error ->
            throw LastfmApiException("Could not reach Last.fm.", error)
        }
        response.use {
            val body = it.body?.string().orEmpty()
            if (!it.isSuccessful) {
                throw LastfmApiException(lastfmErrorMessage(body) ?: "Last.fm returned HTTP ${it.code}.")
            }
            lastfmErrorMessage(body)?.let { message ->
                throw LastfmApiException(message)
            }
            return body
        }
    }

    private inline fun <reified T> decode(body: String): T =
        try {
            json.decodeFromString<T>(body)
        } catch (error: SerializationException) {
            throw LastfmApiException("Last.fm returned an unexpected response.", error)
        }

    private fun lastfmErrorMessage(body: String): String? =
        runCatching {
            val element = json.parseToJsonElement(body).jsonObject
            if ("error" in element) {
                element["message"]?.jsonPrimitive?.content ?: "Last.fm rejected the request."
            } else {
                null
            }
        }.getOrNull()

    private fun md5(value: String): String {
        val digest = MessageDigest.getInstance("MD5").digest(value.toByteArray(Charsets.UTF_8))
        return digest.joinToString("") { byte -> "%02x".format(byte) }
    }

    companion object {
        private const val API_ROOT = "https://ws.audioscrobbler.com/2.0/"
    }
}
