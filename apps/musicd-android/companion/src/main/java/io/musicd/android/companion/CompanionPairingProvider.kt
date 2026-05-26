package io.musicd.android.companion

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Intent
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import androidx.core.content.ContextCompat

class CompanionPairingProvider : ContentProvider() {
    private val pairingStore: PairingTokenStore by lazy {
        PairingTokenStore(requireNotNull(context))
    }

    override fun onCreate(): Boolean = true

    override fun insert(uri: Uri, values: ContentValues?): Uri? {
        if (uri.authority != AUTHORITY || uri.path != "/controller") return null
        val clientId = values?.getAsString("client_id").orEmpty()
        if (clientId.isBlank()) return null
        pairingStore.authorizeController(clientId)
        startControlSurface()
        return uri
    }

    override fun query(
        uri: Uri,
        projection: Array<out String>?,
        selection: String?,
        selectionArgs: Array<out String>?,
        sortOrder: String?,
    ): Cursor? {
        if (uri.authority != AUTHORITY || uri.path != "/controller") return null
        startControlSurface()
        val authorized = pairingStore.authorizedControllerId()
        return MatrixCursor(arrayOf("paired", "client_id_suffix")).apply {
            addRow(arrayOf<Any?>(if (authorized == null) 0 else 1, authorized?.takeLast(8)))
        }
    }

    override fun getType(uri: Uri): String? = null

    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int {
        if (uri.authority != AUTHORITY || uri.path != "/controller") return 0
        val hadPairing = pairingStore.authorizedControllerId() != null
        pairingStore.clear()
        return if (hadPairing) 1 else 0
    }

    override fun update(
        uri: Uri,
        values: ContentValues?,
        selection: String?,
        selectionArgs: Array<out String>?,
    ): Int = 0

    private fun startControlSurface() {
        val appContext = context ?: return
        runCatching {
            ContextCompat.startForegroundService(
                appContext,
                Intent(appContext, MusicdCompanionService::class.java)
                    .setAction(MusicdCompanionService.ACTION_START_CONTROL_SURFACE),
            )
        }
    }

    companion object {
        const val AUTHORITY = "io.musicd.android.companion.pairing"
        const val PERMISSION = "io.musicd.android.companion.permission.PAIR"
    }
}
