package io.musicd.android.companion

import android.content.Context
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.provider.DocumentsContract
import androidx.documentfile.provider.DocumentFile
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.runInterruptible
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.withContext
import java.io.File
import java.security.MessageDigest
import java.util.Locale

class LocalLibraryScanner(private val context: Context) {
    private val database = CompanionDatabase.get(context)
    private val rootDao = database.storageRoots()
    private val libraryDao = database.library()
    private val artworkDir = File(context.filesDir, "artwork")

    suspend fun scanAllRoots(): ScanSummary = withContext(Dispatchers.IO) {
        val roots = rootDao.roots()
        var indexed = 0
        var ignored = 0
        var reused = 0
        val errors = mutableListOf<String>()
        var canceled = false
        for (root in roots) {
            try {
                val rootSummary = scanRoot(root)
                indexed += rootSummary.indexed
                ignored += rootSummary.ignored
                reused += rootSummary.reused
                errors += rootSummary.errors
                canceled = canceled || rootSummary.canceled
            } catch (error: CancellationException) {
                canceled = true
                break
            }
        }
        if (!canceled) {
            LocalLibrarySummaries.rebuild(libraryDao)
        }
        ScanSummary(indexed = indexed, ignored = ignored, reused = reused, errors = errors, canceled = canceled)
    }

    suspend fun markActiveScansCanceled() = withContext(Dispatchers.IO) {
        rootDao.cancelActiveScans(nowUnix())
    }

    suspend fun scanRoot(root: StorageRootEntity): ScanSummary {
        val started = nowUnix()
        rootDao.updateScanState(root.uri, "scanning", started, null, null, root.trackCount, 0, 0, 0, 0, "Starting scan", started)
        val tracks = mutableListOf<TrackEntity>()
        val progress = ScanProgress()
        val errors = mutableListOf<String>()
        val artworkCache = ScanArtworkCache()
        val existingTracks = libraryDao.tracksForRoot(root.uri).associateBy { it.id }

        try {
            val rootFile = DocumentFile.fromTreeUri(context, Uri.parse(root.uri))
            if (rootFile == null || !safCall(ROOT_CHECK_TIMEOUT_MS) { rootFile.exists() }) {
                throw IllegalStateException("Storage root is not available")
            }

            publishProgress(root, progress)
            walk(rootFile, root, tracks, errors, progress, artworkCache, existingTracks)
            libraryDao.replaceRootTracks(root.uri, tracks)
            rootDao.updateScanState(
                root.uri,
                "complete",
                started,
                nowUnix(),
                null,
                tracks.size,
                progress.foldersVisited,
                progress.filesVisited,
                progress.tracksFound,
                progress.filesIgnored,
                null,
                nowUnix(),
            )
        } catch (error: CancellationException) {
            withContext(NonCancellable) {
                rootDao.updateScanState(
                    root.uri,
                    "canceled",
                    started,
                    nowUnix(),
                    null,
                    root.trackCount,
                    progress.foldersVisited,
                    progress.filesVisited,
                    progress.tracksFound,
                    progress.filesIgnored,
                    null,
                    nowUnix(),
                )
            }
            throw error
        } catch (error: Throwable) {
            val message = error.message ?: error::class.java.simpleName
            errors += "${root.label}: $message"
            rootDao.updateScanState(
                root.uri,
                "error",
                started,
                nowUnix(),
                message,
                tracks.size,
                progress.foldersVisited,
                progress.filesVisited,
                progress.tracksFound,
                progress.filesIgnored,
                null,
                nowUnix(),
            )
        }

        return ScanSummary(indexed = tracks.size, ignored = progress.filesIgnored, reused = progress.tracksReused, errors = errors)
    }

    private suspend fun walk(
        file: DocumentFile,
        root: StorageRootEntity,
        tracks: MutableList<TrackEntity>,
        errors: MutableList<String>,
        progress: ScanProgress,
        artworkCache: ScanArtworkCache,
        existingTracks: Map<String, TrackEntity>,
    ) {
        currentCoroutineContext().ensureActive()
        val document = try {
            snapshot(file)
        } catch (error: TimeoutCancellationException) {
            errors += "Timed out reading document details: ${file.uri}"
            return
        }

        if (document.isDirectory) {
            progress.foldersVisited += 1
            publishProgress(root, progress, "Folder: ${document.name.ifBlank { root.label }}", force = true)
            val children = try {
                safCall(LIST_FOLDER_TIMEOUT_MS) { file.listFiles().toList() }
            } catch (error: TimeoutCancellationException) {
                errors += "Timed out listing folder: ${document.name.ifBlank { document.uri.toString() }}"
                return
            }
            children.forEach { child -> walk(child, root, tracks, errors, progress, artworkCache, existingTracks) }
            return
        }

        progress.filesVisited += 1
        if (!document.isFile) {
            progress.filesIgnored += 1
            publishProgress(root, progress)
            return
        }

        if (!isSupportedAudio(document.name, document.mimeType)) {
            progress.filesIgnored += 1
            publishProgress(root, progress)
            return
        }

        try {
            publishProgress(root, progress, "Reading: ${document.label}", force = true)
            val result = trackFor(root, document, artworkCache, existingTracks)
            tracks += result.track
            progress.tracksFound += 1
            if (result.reused) {
                progress.tracksReused += 1
            }
        } catch (error: TimeoutCancellationException) {
            progress.filesIgnored += 1
            errors += "${document.label}: timed out while reading metadata"
        } catch (error: CancellationException) {
            throw error
        } catch (error: Throwable) {
            errors += "${document.label}: ${error.message ?: error::class.java.simpleName}"
        }
        publishProgress(root, progress, "Indexed: ${document.label}")
    }

    private suspend fun publishProgress(
        root: StorageRootEntity,
        progress: ScanProgress,
        currentItem: String? = progress.currentItem,
        force: Boolean = false,
    ) {
        progress.currentItem = currentItem
        if (!force && progress.filesVisited % PROGRESS_UPDATE_INTERVAL != 0 && !progress.shouldPublishImmediately) {
            return
        }
        progress.shouldPublishImmediately = false
        rootDao.updateScanProgress(
            root.uri,
            progress.foldersVisited,
            progress.filesVisited,
            progress.tracksFound,
            progress.filesIgnored,
            progress.currentItem?.take(MAX_CURRENT_ITEM_LENGTH),
            nowUnix(),
        )
    }

    private suspend fun trackFor(
        root: StorageRootEntity,
        document: ScanDocument,
        artworkCache: ScanArtworkCache,
        existingTracks: Map<String, TrackEntity>,
    ): ScanTrackResult {
        val uri = document.uri
        val id = trackIdFor(root, document)
        existingTracks[id]?.let { existingTrack ->
            return ScanTrackResult(existingTrack, reused = true)
        }

        val metadata = safCall(METADATA_TIMEOUT_MS) { readMetadata(uri) }
        val fallbackTitle = document.name.substringBeforeLast('.').cleanupTitle().ifBlank { "Unknown Track" }
        val title = metadata.title ?: fallbackTitle
        val artist = metadata.artist ?: "Unknown Artist"
        val album = metadata.album ?: "Unknown Album"
        val artistId = stableId("artist", artist.lowercase(Locale.ROOT))
        val albumId = stableId("album", artist.lowercase(Locale.ROOT), album.lowercase(Locale.ROOT))
        val artworkPath = artworkCache.pathByAlbumId[albumId]
            ?: if (albumId !in artworkCache.attemptedAlbumIds) {
                artworkCache.attemptedAlbumIds += albumId
                readEmbeddedPictureSaf(uri)?.let { picture ->
                    saveArtwork(albumId, picture)?.also { savedPath ->
                        artworkCache.pathByAlbumId[albumId] = savedPath
                    }
                }
            } else {
                null
            }

        return ScanTrackResult(
            TrackEntity(
                id = id,
                rootUri = root.uri,
                contentUri = uri.toString(),
                albumId = albumId,
                artistId = artistId,
                title = title,
                artist = artist,
                album = album,
                discNumber = metadata.discNumber,
                trackNumber = metadata.trackNumber,
                durationSeconds = metadata.durationMillis?.takeIf { it > 0 }?.div(1000),
                mimeType = document.mimeType,
                size = document.size,
                lastModified = document.lastModified,
                artworkPath = artworkPath,
            ),
            reused = false,
        )
    }

    private fun trackIdFor(root: StorageRootEntity, document: ScanDocument): String {
        val contentIdentity = documentIdentity(document.uri) ?: document.uri.toString()
        return stableId(
            "track",
            root.uri,
            contentIdentity,
            document.name,
            document.size.toString(),
            document.lastModified.toString(),
        )
    }

    private suspend fun snapshot(file: DocumentFile): ScanDocument =
        safCall(DOCUMENT_DETAILS_TIMEOUT_MS) {
            ScanDocument(
                uri = file.uri,
                name = file.name.orEmpty(),
                mimeType = file.type,
                isDirectory = file.isDirectory,
                isFile = file.isFile,
                size = file.length(),
                lastModified = file.lastModified(),
            )
        }

    private fun readMetadata(uri: Uri): AudioMetadata {
        val retriever = MediaMetadataRetriever()
        return try {
            retriever.setDataSource(context, uri)
            AudioMetadata(
                title = retriever.text(MediaMetadataRetriever.METADATA_KEY_TITLE),
                artist = retriever.text(MediaMetadataRetriever.METADATA_KEY_ARTIST)
                    ?: retriever.text(MediaMetadataRetriever.METADATA_KEY_ALBUMARTIST),
                album = retriever.text(MediaMetadataRetriever.METADATA_KEY_ALBUM),
                durationMillis = retriever.text(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull(),
                discNumber = retriever.text(MediaMetadataRetriever.METADATA_KEY_DISC_NUMBER)?.parseNumberPrefix(),
                trackNumber = retriever.text(MediaMetadataRetriever.METADATA_KEY_CD_TRACK_NUMBER)?.parseNumberPrefix(),
            )
        } finally {
            retriever.release()
        }
    }

    private fun readEmbeddedPicture(uri: Uri): ByteArray? {
        val retriever = MediaMetadataRetriever()
        return try {
            retriever.setDataSource(context, uri)
            retriever.embeddedPicture?.takeIf { it.isNotEmpty() }
        } finally {
            retriever.release()
        }
    }

    private suspend fun readEmbeddedPictureSaf(uri: Uri): ByteArray? =
        try {
            safCall(ARTWORK_TIMEOUT_MS) { readEmbeddedPicture(uri) }
        } catch (error: TimeoutCancellationException) {
            null
        } catch (error: CancellationException) {
            throw error
        } catch (error: Throwable) {
            null
        }

    private suspend fun <T> safCall(timeoutMillis: Long, block: () -> T): T =
        withTimeout(timeoutMillis) {
            runInterruptible(Dispatchers.IO) { block() }
        }

    private fun saveArtwork(albumId: String, bytes: ByteArray): String? =
        runCatching {
            artworkDir.mkdirs()
            val extension = artworkExtension(bytes)
            val fileName = "$albumId.$extension"
            val file = File(artworkDir, fileName)
            if (!file.exists() || file.length() != bytes.size.toLong()) {
                file.writeBytes(bytes)
            }
            fileName
        }.getOrNull()

    private fun artworkExtension(bytes: ByteArray): String =
        when {
            bytes.size >= 8 &&
                bytes[0] == 0x89.toByte() &&
                bytes[1] == 0x50.toByte() &&
                bytes[2] == 0x4E.toByte() &&
                bytes[3] == 0x47.toByte() -> "png"
            bytes.size >= 3 &&
                bytes[0] == 0xFF.toByte() &&
                bytes[1] == 0xD8.toByte() &&
                bytes[2] == 0xFF.toByte() -> "jpg"
            else -> "img"
        }

    private fun documentIdentity(uri: Uri): String? =
        runCatching { DocumentsContract.getDocumentId(uri) }.getOrNull()

    private fun MediaMetadataRetriever.text(keyCode: Int): String? =
        extractMetadata(keyCode)?.trim()?.takeIf { it.isNotBlank() }

    private fun isSupportedAudio(name: String, mimeType: String?): Boolean {
        val lowerName = name.lowercase(Locale.ROOT)
        val extensionSupported = supportedExtensions.any { lowerName.endsWith(it) }
        val mimeSupported = when (mimeType?.lowercase(Locale.ROOT)) {
            "audio/mpeg",
            "audio/mp4",
            "audio/aac",
            "audio/x-aac",
            "audio/flac",
            "audio/x-flac",
            "audio/wav",
            "audio/x-wav",
            "audio/wave",
            -> true
            else -> false
        }
        return extensionSupported || mimeSupported
    }

    companion object {
        private val supportedExtensions = listOf(".mp3", ".m4a", ".aac", ".flac", ".wav")
        private const val PROGRESS_UPDATE_INTERVAL = 50
        private const val MAX_CURRENT_ITEM_LENGTH = 160
        private const val ROOT_CHECK_TIMEOUT_MS = 10_000L
        private const val DOCUMENT_DETAILS_TIMEOUT_MS = 10_000L
        private const val LIST_FOLDER_TIMEOUT_MS = 30_000L
        private const val METADATA_TIMEOUT_MS = 15_000L
        private const val ARTWORK_TIMEOUT_MS = 10_000L
    }
}

private data class ScanDocument(
    val uri: Uri,
    val name: String,
    val mimeType: String?,
    val isDirectory: Boolean,
    val isFile: Boolean,
    val size: Long,
    val lastModified: Long,
) {
    val label: String = name.ifBlank { uri.toString() }
}

private data class ScanProgress(
    var foldersVisited: Int = 0,
    var filesVisited: Int = 0,
    var tracksFound: Int = 0,
    var tracksReused: Int = 0,
    var filesIgnored: Int = 0,
    var shouldPublishImmediately: Boolean = true,
    var currentItem: String? = null,
)

private data class ScanArtworkCache(
    val attemptedAlbumIds: MutableSet<String> = mutableSetOf(),
    val pathByAlbumId: MutableMap<String, String> = mutableMapOf(),
)

private data class ScanTrackResult(
    val track: TrackEntity,
    val reused: Boolean,
)

object LocalLibrarySummaries {
    suspend fun rebuild(libraryDao: LibraryDao) {
        val tracks = libraryDao.tracks()
        val albums = tracks
            .groupBy { it.albumId }
            .mapNotNull { (albumId, albumTracks) ->
                val sortedTracks = albumTracks.sortedWith(trackOrder)
                val first = sortedTracks.firstOrNull() ?: return@mapNotNull null
                AlbumEntity(
                    id = albumId,
                    title = first.album,
                    artist = first.artist,
                    artistId = first.artistId,
                    trackCount = albumTracks.size,
                    firstTrackId = first.id,
                    artworkPath = sortedTracks.firstNotNullOfOrNull { it.artworkPath },
                )
            }
            .sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER, AlbumEntity::artist).thenBy(String.CASE_INSENSITIVE_ORDER, AlbumEntity::title))

        val artists = tracks
            .groupBy { it.artistId }
            .mapNotNull { (artistId, artistTracks) ->
                val firstTrack = artistTracks.minWithOrNull(trackOrder) ?: return@mapNotNull null
                val firstAlbum = albums.firstOrNull { it.artistId == artistId } ?: return@mapNotNull null
                ArtistEntity(
                    id = artistId,
                    name = firstTrack.artist,
                    albumCount = artistTracks.map { it.albumId }.distinct().size,
                    trackCount = artistTracks.size,
                    firstAlbumId = firstAlbum.id,
                    artworkPath = firstAlbum.artworkPath,
                )
            }
            .sortedWith(compareBy(String.CASE_INSENSITIVE_ORDER, ArtistEntity::name))

        libraryDao.replaceSummaries(albums, artists)
    }

    private val trackOrder = compareBy<TrackEntity>(
        { it.artist.lowercase(Locale.ROOT) },
        { it.album.lowercase(Locale.ROOT) },
        { it.discNumber ?: Int.MAX_VALUE },
        { it.trackNumber ?: Int.MAX_VALUE },
        { it.title.lowercase(Locale.ROOT) },
    )
}

data class ScanSummary(
    val indexed: Int,
    val ignored: Int,
    val reused: Int = 0,
    val errors: List<String>,
    val canceled: Boolean = false,
)

private data class AudioMetadata(
    val title: String?,
    val artist: String?,
    val album: String?,
    val durationMillis: Long?,
    val discNumber: Int?,
    val trackNumber: Int?,
)

private fun String.cleanupTitle(): String =
    replace('_', ' ')
        .replace(Regex("^\\d+\\s*[-._ ]\\s*"), "")
        .trim()
        .ifBlank { this }

private fun String.parseNumberPrefix(): Int? =
    substringBefore('/')
        .trim()
        .takeWhile { it.isDigit() }
        .takeIf { it.isNotBlank() }
        ?.toIntOrNull()

fun stableId(vararg parts: String): String {
    val digest = MessageDigest.getInstance("SHA-256")
    parts.forEach { part ->
        digest.update(part.toByteArray(Charsets.UTF_8))
        digest.update(0)
    }
    return digest.digest().joinToString("") { "%02x".format(it) }.take(24)
}

fun nowUnix(): Long = System.currentTimeMillis() / 1000L
