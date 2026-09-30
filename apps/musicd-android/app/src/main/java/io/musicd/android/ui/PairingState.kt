package io.musicd.android.ui

import io.musicd.android.data.PairingPollResult

/** Where the "pair this phone with the server" flow is up to. */
sealed interface PairingUiState {
    data object Idle : PairingUiState

    /** The server answered 401: this phone needs a token. */
    data class Needed(val baseUrl: String) : PairingUiState

    data class Starting(val baseUrl: String) : PairingUiState

    data class WaitingForApproval(
        val baseUrl: String,
        val code: String,
        val expiresAtMillis: Long,
    ) : PairingUiState

    data class Failed(val baseUrl: String, val message: String) : PairingUiState
}

val PairingUiState.baseUrl: String?
    get() = when (this) {
        PairingUiState.Idle -> null
        is PairingUiState.Needed -> baseUrl
        is PairingUiState.Starting -> baseUrl
        is PairingUiState.WaitingForApproval -> baseUrl
        is PairingUiState.Failed -> baseUrl
    }

/** The server page where an admin approves the code. */
fun pairingApprovalUrl(baseUrl: String): String = "${baseUrl.trim().trimEnd('/')}/account"

/** The state to show once polling ends without a token. */
fun pairingFailureState(baseUrl: String, result: PairingPollResult): PairingUiState =
    when (result) {
        PairingPollResult.Denied -> PairingUiState.Failed(baseUrl, "Pairing was denied on the server.")
        PairingPollResult.Gone -> PairingUiState.Failed(
            baseUrl,
            "The pairing code expired before it was approved. Try again.",
        )
        PairingPollResult.Pending, is PairingPollResult.Approved -> PairingUiState.Idle
    }
