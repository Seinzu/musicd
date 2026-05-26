package io.musicd.android.data

import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri

enum class MusicSourceKind {
    RemoteServer,
    LocalCompanion,
}

interface MusicSourceRepository {
    val sourceKind: MusicSourceKind
}

class LocalCompanionRepository(private val context: Context) : MusicSourceRepository {
    override val sourceKind: MusicSourceKind = MusicSourceKind.LocalCompanion

    fun launchCompanion(): Boolean {
        val launchIntent = context.packageManager.getLaunchIntentForPackage(COMPANION_PACKAGE)
            ?: return false
        launchIntent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(launchIntent)
        return true
    }

    fun isInstalled(): Boolean =
        context.packageManager.getLaunchIntentForPackage(COMPANION_PACKAGE) != null

    fun pairController(clientId: String): Boolean {
        if (clientId.isBlank()) return false
        val values = ContentValues().apply { put("client_id", clientId) }
        return runCatching {
            context.contentResolver.insert(COMPANION_PAIRING_URI, values) != null
        }.getOrDefault(false)
    }

    companion object {
        const val COMPANION_PACKAGE = "io.musicd.android.companion"
        const val COMPANION_PAIRING_AUTHORITY = "io.musicd.android.companion.pairing"
        const val LOCAL_COMPANION_BASE_URL = "http://127.0.0.1:8788"
        const val LOCAL_COMPANION_RENDERER = "android-local://this-device"
        val COMPANION_PAIRING_URI: Uri = Uri.parse("content://$COMPANION_PAIRING_AUTHORITY/controller")
    }
}
