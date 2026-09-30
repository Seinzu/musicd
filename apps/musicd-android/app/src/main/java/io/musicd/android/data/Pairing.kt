package io.musicd.android.data

sealed interface PairingPollResult {
    data object Pending : PairingPollResult
    data class Approved(val token: String) : PairingPollResult
    data object Denied : PairingPollResult

    /** Expired, unknown, or already collected. */
    data object Gone : PairingPollResult
}

fun pairingPollResult(dto: PairingPollDto): PairingPollResult =
    when (dto.status) {
        "pending" -> PairingPollResult.Pending
        "approved" -> dto.token?.takeIf { it.isNotBlank() }
            ?.let(PairingPollResult::Approved)
            ?: PairingPollResult.Gone
        "denied" -> PairingPollResult.Denied
        else -> PairingPollResult.Gone
    }

/** True when a request failed because the server wants credentials this app doesn't have. */
fun isAuthRequired(error: Throwable): Boolean =
    error is MusicdApiException.Http && error.statusCode == 401

/** How this phone appears on the server's account page and in its token list. */
fun defaultPairingDeviceName(manufacturer: String, model: String): String {
    val cleanModel = model.trim()
    val cleanManufacturer = manufacturer.trim()
    val device = when {
        cleanModel.isEmpty() -> cleanManufacturer
        cleanManufacturer.isEmpty() || cleanModel.startsWith(cleanManufacturer, ignoreCase = true) -> cleanModel
        else -> "${cleanManufacturer.replaceFirstChar { it.uppercase() }} $cleanModel"
    }
    return if (device.isEmpty()) "feltsloth" else "feltsloth on $device"
}
