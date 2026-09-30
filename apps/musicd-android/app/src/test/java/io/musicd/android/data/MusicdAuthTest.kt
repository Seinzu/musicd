package io.musicd.android.data

import kotlinx.coroutines.test.runTest
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class MusicdAuthTest {
    @get:Rule
    val temp = TemporaryFolder()

    private lateinit var paired: MockWebServer
    private lateinit var other: MockWebServer

    private class MemoryStore : AuthTokenStore {
        val tokens = mutableMapOf<String, String>()
        override fun tokenFor(origin: String) = tokens[origin]
        override fun saveToken(origin: String, token: String) {
            tokens[origin] = token
        }
        override fun clearToken(origin: String) {
            tokens.remove(origin)
        }
    }

    @Before
    fun setUp() {
        paired = MockWebServer().apply { start() }
        other = MockWebServer().apply { start() }
    }

    @After
    fun tearDown() {
        paired.shutdown()
        other.shutdown()
    }

    @Test
    fun originsIgnoreCasePathsAndDefaultPorts() {
        assertEquals("http://nas:80", serverOrigin("http://NAS/api/tracks"))
        assertEquals("http://nas:80", serverOrigin("http://nas:80"))
        assertEquals("http://192.168.1.10:7878", serverOrigin(" http://192.168.1.10:7878/ "))
        assertEquals("https://nas:443", serverOrigin("https://nas"))
        assertNull(serverOrigin("cli-local://abc"))
        assertNull(serverOrigin("not a url"))
    }

    @Test
    fun fileStoreRoundTripsAndClearsTokens() {
        val file = temp.root.resolve("nested/tokens.properties")
        val store = FileAuthTokenStore(file)
        assertNull(store.tokenFor("http://nas:7878"))

        store.saveToken("http://nas:7878", "mdt_a")
        store.saveToken("http://other:7878", "mdt_b")
        assertEquals("mdt_a", FileAuthTokenStore(file).tokenFor("http://nas:7878"))

        store.clearToken("http://nas:7878")
        assertNull(FileAuthTokenStore(file).tokenFor("http://nas:7878"))
        assertEquals("mdt_b", FileAuthTokenStore(file).tokenFor("http://other:7878"))
    }

    @Test
    fun interceptorOnlySendsTheTokenToItsOwnServer() = runTest {
        val store = MemoryStore()
        store.saveToken(serverOrigin(paired.url("/").toString())!!, "mdt_secret")
        val client = OkHttpClient.Builder().addInterceptor(MusicdAuthInterceptor(store)).build()
        paired.enqueue(MockResponse().setBody("{}"))
        paired.enqueue(MockResponse().setBody("{}"))
        other.enqueue(MockResponse().setBody("{}"))

        client.newCall(Request.Builder().url(paired.url("/api/tracks")).build()).execute().close()
        client.newCall(
            Request.Builder().url(paired.url("/api/server")).header("Authorization", "Bearer explicit").build(),
        ).execute().close()
        client.newCall(Request.Builder().url(other.url("/stream")).build()).execute().close()

        assertEquals("Bearer mdt_secret", paired.takeRequest().getHeader("Authorization"))
        assertEquals("Bearer explicit", paired.takeRequest().getHeader("Authorization"))
        assertNull(other.takeRequest().getHeader("Authorization"))
    }

    @Test
    fun playerHeaderMatchesTheSameOriginRules() {
        val store = MemoryStore()
        store.saveToken("http://nas:7878", "mdt_x")
        assertEquals("Bearer mdt_x", authorizationHeaderFor("http://nas:7878/stream/track/a", store))
        assertNull(authorizationHeaderFor("http://nas:8080/stream/track/a", store))
        assertNull(authorizationHeaderFor("https://radio.example/live", store))
    }
}
