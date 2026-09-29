package io.musicd.android.data

import kotlinx.coroutines.test.runTest
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test

class MusicdApiRecommendationsTest {
    private lateinit var server: MockWebServer
    private val api = MusicdApi()

    @Before
    fun setUp() {
        server = MockWebServer()
        server.start()
    }

    @After
    fun tearDown() {
        server.shutdown()
    }

    private val baseUrl: String
        get() = server.url("/").toString().trimEnd('/')

    @Test
    fun dismissPostsTheRecommendationKey() = runTest {
        server.enqueue(
            MockResponse().setBody("""{"ok":true,"message":"Dismissed 2 recommendation(s).","dismissed":2}"""),
        )

        val response = api.dismissRecommendation(baseUrl, "llm:seed/eden & co")

        val request = server.takeRequest()
        assertEquals("POST", request.method)
        assertEquals("/api/recommendations/dismiss", request.path)
        assertEquals(
            "recommendation_key=llm%3Aseed%2Feden%20%26%20co",
            request.body.readUtf8(),
        )
        assertTrue(response.ok)
        assertEquals("Dismissed 2 recommendation(s).", response.message)
    }

    @Test
    fun dismissSurfacesServerErrors() = runTest {
        server.enqueue(
            MockResponse().setResponseCode(404).setBody("""{"ok":false,"error":"recommendation not found"}"""),
        )

        try {
            api.dismissRecommendation(baseUrl, "missing")
            fail("Expected dismissRecommendation to throw")
        } catch (error: MusicdApiException.Http) {
            assertEquals(404, error.statusCode)
        }
    }

    @Test
    fun collectionRecommendationsOnlyRequestSuggestedOnes() = runTest {
        server.enqueue(
            MockResponse().setBody(
                """
                {"recommendations":[{
                  "recommendation_key":"k1","source":"llm","seed_album_id":"seed",
                  "suggested_artist":"Talk Talk","suggested_title":"Spirit of Eden",
                  "status":"suggested","dismiss_count":3,"last_dismissed_unix":1790000000,
                  "unknown_field":true
                }]}
                """.trimIndent(),
            ),
        )

        val response = api.getCollectionRecommendations(baseUrl, limit = 6)

        val request = server.takeRequest()
        val url = request.requestUrl!!
        assertEquals("/api/recommendations", url.encodedPath)
        assertEquals("suggested", url.queryParameter("status"))
        assertEquals("true", url.queryParameter("exclude_library"))
        assertEquals("6", url.queryParameter("limit"))
        assertEquals(listOf("k1"), response.recommendations.map { it.recommendationKey })
        assertEquals(3L, response.recommendations.single().dismissCount)
        assertEquals(1790000000L, response.recommendations.single().lastDismissedUnix)
    }
}
