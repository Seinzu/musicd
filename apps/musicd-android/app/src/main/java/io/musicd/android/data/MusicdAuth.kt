package io.musicd.android.data

import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.Interceptor
import okhttp3.Response
import java.io.File
import java.io.IOException
import java.util.Properties

/**
 * API tokens obtained by pairing, keyed by server origin (see [serverOrigin]).
 */
interface AuthTokenStore {
    fun tokenFor(origin: String): String?
    fun saveToken(origin: String, token: String)
    fun clearToken(origin: String)
}

/**
 * `scheme://host:port` for an http(s) URL, with the port always explicit, so
 * `http://nas:80/x` and `http://NAS/` match. Null for anything else.
 */
fun serverOrigin(url: String): String? {
    val parsed = url.trim().toHttpUrlOrNull() ?: return null
    return "${parsed.scheme}://${parsed.host}:${parsed.port}"
}

/** The `Authorization` header value for [url], if its server has a token. */
fun authorizationHeaderFor(url: String, store: AuthTokenStore): String? =
    serverOrigin(url)?.let(store::tokenFor)?.let { "Bearer $it" }

/**
 * Adds the paired server's token to requests for that server only, so tokens
 * never leak to Last.fm, radio streams or artwork hosts.
 */
class MusicdAuthInterceptor(private val store: AuthTokenStore) : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        if (request.header(AUTHORIZATION) != null) {
            return chain.proceed(request)
        }
        val header = authorizationHeaderFor(request.url.toString(), store)
            ?: return chain.proceed(request)
        return chain.proceed(request.newBuilder().header(AUTHORIZATION, header).build())
    }

    companion object {
        const val AUTHORIZATION = "Authorization"
    }
}

/**
 * Tokens in a properties file. On Android it lives in `noBackupFilesDir`, so
 * device credentials stay out of cloud backups.
 */
class FileAuthTokenStore(private val file: File) : AuthTokenStore {
    private val lock = Any()

    override fun tokenFor(origin: String): String? = synchronized(lock) {
        load().getProperty(origin)?.takeIf { it.isNotBlank() }
    }

    override fun saveToken(origin: String, token: String) = synchronized(lock) {
        val properties = load()
        properties.setProperty(origin, token)
        store(properties)
    }

    override fun clearToken(origin: String) = synchronized(lock) {
        val properties = load()
        if (properties.remove(origin) != null) {
            store(properties)
        }
    }

    private fun load(): Properties {
        val properties = Properties()
        if (file.exists()) {
            try {
                file.inputStream().use(properties::load)
            } catch (_: IOException) {
                // An unreadable file behaves like "not paired"; pairing again rewrites it.
            }
        }
        return properties
    }

    private fun store(properties: Properties) {
        file.parentFile?.mkdirs()
        val temp = File(file.parentFile, "${file.name}.tmp")
        temp.outputStream().use { properties.store(it, null) }
        if (!temp.renameTo(file)) {
            file.delete()
            if (!temp.renameTo(file)) {
                throw IOException("Couldn't save the pairing token.")
            }
        }
    }
}
