package io.musicd.android.companion

import android.content.Context
import androidx.core.content.edit

class PairingTokenStore(context: Context) {
    private val prefs = context.applicationContext.getSharedPreferences("musicd_companion_pairing", Context.MODE_PRIVATE)

    fun authorizeController(clientId: String) {
        prefs.edit { putString(KEY_AUTHORIZED_CLIENT_ID, clientId) }
    }

    fun authorizedControllerId(): String? =
        prefs.getString(KEY_AUTHORIZED_CLIENT_ID, "").orEmpty().takeIf { it.isNotBlank() }

    fun isAuthorized(clientId: String?): Boolean {
        val authorized = authorizedControllerId()
        return authorized != null && clientId == authorized
    }

    fun clear() {
        prefs.edit { remove(KEY_AUTHORIZED_CLIENT_ID) }
    }

    companion object {
        private const val KEY_AUTHORIZED_CLIENT_ID = "authorized_client_id"
    }
}
