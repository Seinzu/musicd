package io.musicd.android.ui

import io.musicd.android.data.PairingPollResult
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PairingStateTest {
    @Test
    fun everyActiveStateKnowsItsServer() {
        val baseUrl = "http://nas:7878"
        assertNull(PairingUiState.Idle.baseUrl)
        assertEquals(baseUrl, PairingUiState.Needed(baseUrl).baseUrl)
        assertEquals(baseUrl, PairingUiState.Starting(baseUrl).baseUrl)
        assertEquals(baseUrl, PairingUiState.WaitingForApproval(baseUrl, "ABCD-EFGH", 0L).baseUrl)
        assertEquals(baseUrl, PairingUiState.Failed(baseUrl, "nope").baseUrl)
    }

    @Test
    fun approvalUrlPointsAtTheAccountPage() {
        assertEquals("http://nas:7878/account", pairingApprovalUrl("http://nas:7878/"))
    }

    @Test
    fun failedPollsExplainWhatHappened() {
        val denied = pairingFailureState("http://nas", PairingPollResult.Denied)
        val expired = pairingFailureState("http://nas", PairingPollResult.Gone)
        assertTrue(denied is PairingUiState.Failed && denied.message.contains("denied"))
        assertTrue(expired is PairingUiState.Failed && expired.message.contains("expired"))
    }
}
