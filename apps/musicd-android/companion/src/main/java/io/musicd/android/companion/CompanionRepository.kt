package io.musicd.android.companion

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import androidx.documentfile.provider.DocumentFile
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

data class SafRoot(
    val uri: String,
    val label: String,
    val lastScanStatus: String = "never_scanned",
    val lastScanError: String? = null,
    val trackCount: Int = 0,
    val scanFoldersVisited: Int = 0,
    val scanFilesVisited: Int = 0,
    val scanTracksFound: Int = 0,
    val scanFilesIgnored: Int = 0,
    val scanCurrentItem: String? = null,
    val scanLastProgressUnix: Long? = null,
)

class CompanionRepository(private val context: Context) {
    private val database = CompanionDatabase.get(context)
    private val rootDao = database.storageRoots()
    private val legacyPrefs = context.getSharedPreferences("musicd_companion", Context.MODE_PRIVATE)

    suspend fun loadRoots(): List<SafRoot> = withContext(Dispatchers.IO) {
        migrateLegacyRoots()
        rootDao.roots().map { it.toSafRoot() }
    }

    suspend fun persistRoot(uri: Uri): Unit = withContext(Dispatchers.IO) {
        val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
        runCatching {
            context.contentResolver.takePersistableUriPermission(uri, flags)
        }.recoverCatching {
            context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        rootDao.upsert(
            StorageRootEntity(
                uri = uri.toString(),
                label = labelFor(uri),
            ),
        )
    }

    suspend fun removeRoot(root: SafRoot): Unit = withContext(Dispatchers.IO) {
        val uri = Uri.parse(root.uri)
        runCatching {
            context.contentResolver.releasePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
            )
        }.recoverCatching {
            context.contentResolver.releasePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        database.library().deleteTracksForRoot(root.uri)
        rootDao.delete(root.uri)
        LocalLibrarySummaries.rebuild(database.library())
    }

    private fun labelFor(uri: Uri): String {
        val documentFile = DocumentFile.fromTreeUri(context, uri)
        val documentName = documentFile?.name?.takeIf { it.isNotBlank() }
        if (documentName != null) return documentName

        return runCatching { DocumentsContract.getTreeDocumentId(uri) }
            .getOrNull()
            ?.substringAfterLast(':')
            ?.ifBlank { null }
            ?: "Music folder"
    }

    private suspend fun migrateLegacyRoots() {
        val legacyRoots = legacyPrefs.getStringSet(LEGACY_KEY_ROOT_URIS, emptySet()).orEmpty()
        if (legacyRoots.isEmpty()) return
        val existing = rootDao.roots().map { it.uri }.toSet()
        legacyRoots
            .filterNot { it in existing }
            .forEach { uri ->
                rootDao.upsert(
                    StorageRootEntity(
                        uri = uri,
                        label = labelFor(Uri.parse(uri)),
                    ),
                )
            }
        legacyPrefs.edit().remove(LEGACY_KEY_ROOT_URIS).apply()
    }

    companion object {
        private const val LEGACY_KEY_ROOT_URIS = "root_uris"
    }
}

private fun StorageRootEntity.toSafRoot(): SafRoot =
    SafRoot(
        uri = uri,
        label = label,
        lastScanStatus = lastScanStatus,
        lastScanError = lastScanError,
        trackCount = trackCount,
        scanFoldersVisited = scanFoldersVisited,
        scanFilesVisited = scanFilesVisited,
        scanTracksFound = scanTracksFound,
        scanFilesIgnored = scanFilesIgnored,
        scanCurrentItem = scanCurrentItem,
        scanLastProgressUnix = scanLastProgressUnix,
    )
