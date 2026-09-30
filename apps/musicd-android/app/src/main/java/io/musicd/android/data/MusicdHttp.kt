package io.musicd.android.data

import android.content.Context
import androidx.annotation.OptIn
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import java.io.File
import okhttp3.OkHttpClient

/**
 * App-wide HTTP plumbing that carries the pairing token: one token store and
 * one OkHttp client shared by the API, the event stream and Coil.
 */
object MusicdHttp {
    private const val TOKEN_FILE = "musicd_auth_tokens.properties"

    @Volatile
    private var tokenStore: AuthTokenStore? = null

    @Volatile
    private var client: OkHttpClient? = null

    fun tokenStore(context: Context): AuthTokenStore =
        tokenStore ?: synchronized(this) {
            tokenStore ?: FileAuthTokenStore(
                File(context.applicationContext.noBackupFilesDir, TOKEN_FILE),
            ).also { tokenStore = it }
        }

    fun client(context: Context): OkHttpClient =
        client ?: synchronized(this) {
            client ?: OkHttpClient.Builder()
                .addInterceptor(MusicdAuthInterceptor(tokenStore(context)))
                .build()
                .also { client = it }
        }

    /** An ExoPlayer builder whose HTTP requests carry the token for a paired server. */
    @OptIn(UnstableApi::class)
    fun playerBuilder(context: Context): ExoPlayer.Builder =
        ExoPlayer.Builder(context)
            .setMediaSourceFactory(DefaultMediaSourceFactory(playerDataSourceFactory(context)))

    @OptIn(UnstableApi::class)
    private fun playerDataSourceFactory(context: Context): DataSource.Factory {
        val store = tokenStore(context)
        return ResolvingDataSource.Factory(DefaultDataSource.Factory(context)) { dataSpec ->
            val header = authorizationHeaderFor(dataSpec.uri.toString(), store)
            if (header == null) {
                dataSpec
            } else {
                dataSpec.withAdditionalHeaders(mapOf(MusicdAuthInterceptor.AUTHORIZATION to header))
            }
        }
    }
}
