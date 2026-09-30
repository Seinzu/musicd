package io.musicd.android.data

import kotlinx.coroutines.test.runTest
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test

class MusicdApiPairingTest {
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
    fun startPairingSendsTheDeviceName() = runTest {
        server.enqueue(
            MockResponse().setResponseCode(201).setBody(
                """{"ok":true,"pairing_id":"abc","code":"FFSY-F5VK","expires_in":600,"poll_interval":2}""",
            ),
        )

        val started = api.startPairing(baseUrl, "feltsloth on Pixel 8")

        val request = server.takeRequest()
        assertEquals("/api/pair/start", request.path)
        assertEquals("name=feltsloth%20on%20Pixel%208", request.body.readUtf8())
        assertEquals("abc", started.pairingId)
        assertEquals("FFSY-F5VK", started.code)
        assertEquals(600L, started.expiresInSeconds)
        assertEquals(2L, started.pollIntervalSeconds)
    }

    @Test
    fun pollResultsMapToOutcomes() = runTest {
        server.enqueue(MockResponse().setBody("""{"ok":true,"status":"pending"}"""))
        server.enqueue(MockResponse().setBody("""{"ok":true,"status":"approved","token":"mdt_x"}"""))

        assertEquals(PairingPollResult.Pending, pairingPollResult(api.pollPairing(baseUrl, "abc")))
        assertEquals(
            PairingPollResult.Approved("mdt_x"),
            pairingPollResult(api.pollPairing(baseUrl, "abc")),
        )
        assertEquals("pairing_id=abc", server.takeRequest().body.readUtf8())
    }

    @Test
    fun pollOutcomesCoverDenialAndMissingTokens() {
        assertEquals(PairingPollResult.Denied, pairingPollResult(PairingPollDto(status = "denied")))
        assertEquals(PairingPollResult.Gone, pairingPollResult(PairingPollDto(status = "approved")))
        assertEquals(PairingPollResult.Gone, pairingPollResult(PairingPollDto(status = "something-new")))
    }

    @Test
    fun unauthorizedResponsesAskForPairing() = runTest {
        server.enqueue(
            MockResponse().setResponseCode(401).setBody("""{"ok":false,"error":"authentication required"}"""),
        )

        try {
            api.getServerInfo(baseUrl)
            fail("Expected getServerInfo to throw")
        } catch (error: MusicdApiException.Http) {
            assertEquals(401, error.statusCode)
            assertEquals("This server needs this phone to be paired.", error.userMessage)
            assertTrue(isAuthRequired(error))
        }
        assertFalse(isAuthRequired(MusicdApiException.Http(403, null, "no")))
    }

    @Test
    fun deviceNamesReadNaturally() {
        assertEquals("feltsloth on Google Pixel 8", defaultPairingDeviceName("google", "Pixel 8"))
        assertEquals("feltsloth on Samsung SM-S911B", defaultPairingDeviceName("samsung", "SM-S911B"))
        assertEquals("feltsloth on OnePlus 12", defaultPairingDeviceName("OnePlus", "OnePlus 12"))
        assertEquals("feltsloth", defaultPairingDeviceName("", ""))
    }
}
