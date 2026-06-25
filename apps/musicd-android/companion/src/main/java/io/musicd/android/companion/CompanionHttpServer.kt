package io.musicd.android.companion

import android.content.Context
import kotlinx.coroutines.runBlocking
import java.io.BufferedReader
import java.io.BufferedWriter
import java.io.File
import java.io.InputStreamReader
import java.io.OutputStreamWriter
import java.net.InetAddress
import java.net.ServerSocket
import java.net.URLDecoder
import java.nio.charset.StandardCharsets
import kotlin.concurrent.thread

class CompanionHttpServer(
    context: Context,
    playbackRuntime: CompanionPlaybackRuntime,
) {
    private val appContext = context.applicationContext
    private val database = CompanionDatabase.get(context)
    private val pairingStore = PairingTokenStore(context)
    private val queueController = CompanionQueueController(database, playbackRuntime)
    private var serverSocket: ServerSocket? = null
    private var serverThread: Thread? = null

    fun start() {
        if (serverThread?.isAlive == true) return
        serverThread = thread(name = "musicd-companion-http", isDaemon = true) {
            val socket = ServerSocket(LOCAL_COMPANION_PORT, 50, InetAddress.getByName("127.0.0.1"))
            serverSocket = socket
            while (!socket.isClosed) {
                runCatching {
                    val client = socket.accept()
                    thread(name = "musicd-companion-http-client", isDaemon = true) {
                        client.use { handleClient(it.getInputStream().buffered(), it.getOutputStream().buffered()) }
                    }
                }
            }
        }
    }

    fun stop() {
        runCatching { serverSocket?.close() }
        serverSocket = null
        serverThread = null
    }

    private fun handleClient(input: java.io.InputStream, output: java.io.OutputStream) {
        val reader = BufferedReader(InputStreamReader(input, StandardCharsets.UTF_8))
        val writer = BufferedWriter(OutputStreamWriter(output, StandardCharsets.UTF_8))
        runCatching {
            val requestLine = reader.readLine().orEmpty()
            val headers = mutableMapOf<String, String>()
            while (true) {
                val line = reader.readLine() ?: break
                if (line.isEmpty()) break
                val colon = line.indexOf(':')
                if (colon > 0) {
                    headers[line.substring(0, colon).trim().lowercase()] = line.substring(colon + 1).trim()
                }
            }

            val parts = requestLine.split(' ')
            if (parts.size < 2) {
                writer.respond(400, """{"error":"bad request"}""")
                return
            }
            val method = parts[0]
            if (method != "GET" && method != "POST") {
                writer.respond(405, """{"error":"method not allowed"}""")
                return
            }

            val target = parts[1]
            val path = target.substringBefore('?')
            if (method == "GET" && path.startsWith("/api/artwork/local/")) {
                val fileName = decodePathSegment(path.removePrefix("/api/artwork/local/"))
                    .substringBefore('/')
                output.respondFile(artworkFile(fileName))
                return
            }
            val form = if (method == "POST") {
                val contentLength = headers["content-length"]?.toIntOrNull() ?: 0
                val chars = CharArray(contentLength)
                if (contentLength > 0) reader.read(chars, 0, contentLength)
                parseForm(String(chars))
            } else {
                emptyMap()
            }
            if (method == "POST" && !pairingStore.isAuthorized(form["client_id"])) {
                writer.respond(403, """{"error":"local companion pairing is required"}""")
                return
            }
            val body = runBlocking { responseFor(method, path, form) }
            if (body == null) {
                writer.respond(404, """{"error":"not found"}""")
            } else {
                writer.respond(200, body)
            }
        }.onFailure { error ->
            runCatching {
                writer.respond(500, """{"error":${(error.message ?: "internal server error").json()}}""")
            }
        }
    }

    private suspend fun responseFor(method: String, path: String, form: Map<String, String>): String? {
        val library = database.library()
        val queue = database.queue()
        return when {
            method == "GET" && path == "/health" -> """{"ok":true}"""
            method == "GET" && path == "/api/server" -> serverJson()
            method == "GET" && path == "/api/renderers" -> renderersJson()
            method == "GET" && path == "/api/tracks" -> tracksJson(library.tracks())
            method == "GET" && path == "/api/albums" -> albumsJson(library.albums())
            method == "GET" && path == "/api/artists" -> artistsJson(library.artists())
            method == "GET" && path == "/api/queue" -> queueJson(queue, library)
            method == "GET" && path == "/api/now-playing" -> nowPlayingJson(queue, library)
            method == "GET" && path.startsWith("/api/albums/") -> {
                val albumId = decodePathSegment(path.removePrefix("/api/albums/"))
                    .substringBefore('/')
                val album = library.album(albumId) ?: return null
                val tracks = library.tracksForAlbum(albumId)
                albumDetailJson(album, tracks)
            }
            method == "GET" && path.startsWith("/api/artists/") -> {
                val artistId = decodePathSegment(path.removePrefix("/api/artists/"))
                    .substringBefore('/')
                val artist = library.artist(artistId) ?: return null
                val albums = library.albumsForArtist(artistId)
                artistDetailJson(artist, albums)
            }
            method == "POST" -> mutationResponse(path, form, queue, library)
            else -> null
        }
    }

    private suspend fun mutationResponse(
        path: String,
        form: Map<String, String>,
        queue: QueueDao,
        library: LibraryDao,
    ): String? {
        val result = when (path) {
            "/api/play" -> queueController.playTrack(form["track_id"].orEmpty())
            "/api/play-album" -> queueController.playAlbum(form["album_id"].orEmpty())
            "/api/queue/append-track" -> queueController.appendTrack(form["track_id"].orEmpty())
            "/api/queue/play-next-track" -> queueController.playNextTrack(form["track_id"].orEmpty())
            "/api/queue/append-album" -> queueController.appendAlbum(form["album_id"].orEmpty())
            "/api/queue/play-next-album" -> queueController.playNextAlbum(form["album_id"].orEmpty())
            "/api/queue/move" -> queueController.move(form["entry_id"]?.toLongOrNull() ?: -1, form["direction"].orEmpty())
            "/api/queue/remove" -> queueController.remove(form["entry_id"]?.toLongOrNull() ?: -1)
            "/api/queue/clear" -> queueController.clear()
            "/api/transport/play" -> queueController.transportPlay()
            "/api/transport/pause" -> queueController.transportPause()
            "/api/transport/stop" -> queueController.transportStop()
            "/api/transport/next" -> queueController.transportNext()
            "/api/transport/previous" -> queueController.transportPrevious()
            else -> return null
        }
        return mutationJson(result, queue, library)
    }

    private fun artworkFile(fileName: String): File? {
        if (fileName.isBlank() || fileName.contains('/') || fileName.contains("..")) return null
        return File(File(appContext.filesDir, "artwork"), fileName).takeIf { it.isFile }
    }
}

private fun BufferedWriter.respond(status: Int, body: String) {
    val reason = when (status) {
        200 -> "OK"
        403 -> "Forbidden"
        404 -> "Not Found"
        405 -> "Method Not Allowed"
        else -> "Error"
    }
    val bytes = body.toByteArray(StandardCharsets.UTF_8)
    write("HTTP/1.1 $status $reason\r\n")
    write("Content-Type: application/json; charset=utf-8\r\n")
    write("Content-Length: ${bytes.size}\r\n")
    write("Connection: close\r\n")
    write("\r\n")
    write(body)
    flush()
}

private fun java.io.OutputStream.respondFile(file: File?) {
    if (file == null) {
        bufferedWriter(StandardCharsets.UTF_8).respond(404, """{"error":"not found"}""")
        return
    }
    val bytes = file.readBytes()
    write("HTTP/1.1 200 OK\r\n".toByteArray(StandardCharsets.UTF_8))
    write("Content-Type: ${file.artworkContentType()}\r\n".toByteArray(StandardCharsets.UTF_8))
    write("Content-Length: ${bytes.size}\r\n".toByteArray(StandardCharsets.UTF_8))
    write("Cache-Control: private, max-age=86400\r\n".toByteArray(StandardCharsets.UTF_8))
    write("Connection: close\r\n".toByteArray(StandardCharsets.UTF_8))
    write("\r\n".toByteArray(StandardCharsets.UTF_8))
    write(bytes)
    flush()
}

private fun File.artworkContentType(): String =
    when (extension.lowercase()) {
        "png" -> "image/png"
        "jpg", "jpeg" -> "image/jpeg"
        else -> "application/octet-stream"
    }

private fun parseForm(body: String): Map<String, String> =
    body.split('&')
        .filter { it.isNotBlank() }
        .associate { pair ->
            val key = pair.substringBefore('=')
            val value = pair.substringAfter('=', "")
            decodePathSegment(key) to decodePathSegment(value)
        }

private fun serverJson(): String =
    """
    {
      "name": "feltsloth Companion",
      "base_url": "$LOCAL_COMPANION_BASE_URL",
      "bind_address": "127.0.0.1:$LOCAL_COMPANION_PORT"
    }
    """.trimIndent()

private fun renderersJson(): String =
    """
    [
      {
        "location": "$LOCAL_COMPANION_RENDERER",
        "name": "This phone",
        "manufacturer": "Android",
        "model_name": "Local companion",
        "selected": true,
        "kind": "android_local",
        "direct_access": true,
        "visibility": "private"
      }
    ]
    """.trimIndent()

private suspend fun queueJson(queue: QueueDao, library: LibraryDao): String {
    val entries = queue.entries()
    val session = queue.session()
    val tracks = entries.mapNotNull { entry -> library.track(entry.trackId)?.let { entry to it } }
    return """
    {
      "renderer_location": "$LOCAL_COMPANION_RENDERER",
      "name": "This phone",
      "status": ${queueStatus(session).json()},
      "version": ${nowUnix()},
      "updated_unix": ${nowUnix()},
      "current_entry_id": ${session?.queueEntryId.jsonNumber()},
      "entries": ${queueEntriesJson(tracks)},
      "session": ${sessionJson(session, entries)}
    }
    """.trimIndent()
}

private suspend fun nowPlayingJson(queue: QueueDao, library: LibraryDao): String {
    val session = queue.session()
    val currentTrack = session?.queueEntryId
        ?.let { queue.entry(it) }
        ?.let { library.track(it.trackId) }
    val entries = queue.entries()
    return """
    {
      "renderer_location": "$LOCAL_COMPANION_RENDERER",
      "renderer": ${renderersJson().removeSurrounding("[", "]").trim()},
      "current_track": ${currentTrack?.toTrackJson() ?: "null"},
      "session": ${sessionJson(session, entries)},
      "queue_summary": {
        "status": ${queueStatus(session).json()},
        "name": "This phone",
        "entry_count": ${entries.size},
        "current_entry_id": ${session?.queueEntryId.jsonNumber()},
        "updated_unix": ${nowUnix()},
        "version": ${nowUnix()}
      }
    }
    """.trimIndent()
}

private suspend fun mutationJson(result: MutationResult, queue: QueueDao, library: LibraryDao): String =
    """
    {
      "ok": ${result.ok},
      "message": ${result.message.jsonNullable()},
      "error": ${result.error.jsonNullable()},
      "renderer_location": "$LOCAL_COMPANION_RENDERER",
      "queue": ${queueJson(queue, library)},
      "session": ${sessionJson(queue.session(), queue.entries())}
    }
    """.trimIndent()

private fun queueEntriesJson(entries: List<Pair<QueueEntryEntity, TrackEntity>>): String =
    entries.joinToString(prefix = "[", postfix = "]") { (entry, track) ->
        """
        {
          "id": ${entry.id},
          "position": ${entry.position},
          "track_id": ${entry.trackId.json()},
          "title": ${track.title.json()},
          "artist": ${track.artist.json()},
          "album": ${track.album.json()},
          "entry_status": ${entry.entryStatus.json()},
          "duration_seconds": ${track.durationSeconds.jsonNumber()}
        }
        """.trimIndent()
    }

private fun sessionJson(session: PlaybackSessionEntity?, entries: List<QueueEntryEntity> = emptyList()): String {
    if (session == null) return "null"
    val nextQueueEntryId = nextQueueEntryId(session, entries)
    return """
    {
      "transport_state": ${session.transportState.json()},
      "queue_entry_id": ${session.queueEntryId.jsonNumber()},
      "next_queue_entry_id": ${nextQueueEntryId.jsonNumber()},
      "current_track_uri": ${session.currentTrackUri.jsonNullable()},
      "position_seconds": ${session.positionSeconds.jsonNumber()},
      "duration_seconds": ${session.durationSeconds.jsonNumber()},
      "last_observed_unix": ${session.lastObservedUnix},
      "server_unix": ${nowUnix()},
      "last_error": null
    }
    """.trimIndent()
}

private fun nextQueueEntryId(session: PlaybackSessionEntity, entries: List<QueueEntryEntity>): Long? {
    val current = session.queueEntryId?.let { currentId -> entries.firstOrNull { it.id == currentId } }
        ?: return null
    return entries.firstOrNull { entry ->
        entry.position > current.position && entry.entryStatus != "completed"
    }?.id
}

private fun queueStatus(session: PlaybackSessionEntity?): String =
    when (session?.transportState) {
        "PLAYING", "TRANSITIONING" -> "playing"
        "PAUSED_PLAYBACK" -> "paused"
        else -> "idle"
    }

private fun legacyNowPlayingJson(): String =
    """
    {
      "renderer_location": "$LOCAL_COMPANION_RENDERER",
      "renderer": ${renderersJson().removeSurrounding("[", "]").trim()},
      "current_track": null,
      "session": null,
      "queue_summary": {
        "status": "idle",
        "name": "This phone",
        "entry_count": 0,
        "current_entry_id": null,
        "updated_unix": ${nowUnix()},
        "version": 0
      }
    }
    """.trimIndent()

private fun tracksJson(tracks: List<TrackEntity>): String =
    tracks.joinToString(prefix = "[", postfix = "]") { it.toTrackJson() }

private fun albumsJson(albums: List<AlbumEntity>): String =
    albums.joinToString(prefix = "[", postfix = "]") { it.toAlbumJson() }

private fun artistsJson(artists: List<ArtistEntity>): String =
    artists.joinToString(prefix = "[", postfix = "]") { it.toArtistJson() }

private fun albumDetailJson(album: AlbumEntity, tracks: List<TrackEntity>): String =
    """
    {
      "id": ${album.id.json()},
      "title": ${album.title.json()},
      "artist": ${album.artist.json()},
      "metadata": {},
      "track_count": ${album.trackCount},
      "first_track_id": ${album.firstTrackId.json()},
      "artwork_url": ${album.artworkUrl().json()},
      "tracks": ${tracksJson(tracks)}
    }
    """.trimIndent()

private fun artistDetailJson(artist: ArtistEntity, albums: List<AlbumEntity>): String =
    """
    {
      "id": ${artist.id.json()},
      "name": ${artist.name.json()},
      "album_count": ${artist.albumCount},
      "track_count": ${artist.trackCount},
      "artwork_url": ${artist.artworkUrl().jsonNullable()},
      "first_album_id": ${artist.firstAlbumId.json()},
      "albums": ${albumsJson(albums)}
    }
    """.trimIndent()

private fun TrackEntity.toTrackJson(): String =
    """
    {
      "id": ${id.json()},
      "album_id": ${albumId.json()},
      "title": ${title.json()},
      "artist": ${artist.json()},
      "album": ${album.json()},
      "disc_number": ${discNumber.jsonNumber()},
      "track_number": ${trackNumber.jsonNumber()},
      "duration_seconds": ${durationSeconds.jsonNumber()},
      "artwork_url": ${artworkUrl().jsonNullable()},
      "mime_type": ${mimeType.jsonNullable()},
      "like_count": 0,
      "liked_by_client": false
    }
    """.trimIndent()

private fun AlbumEntity.toAlbumJson(): String =
    """
    {
      "id": ${id.json()},
      "title": ${title.json()},
      "artist": ${artist.json()},
      "track_count": $trackCount,
      "first_track_id": ${firstTrackId.json()},
      "artwork_url": ${artworkUrl().json()},
      "metadata": {},
      "like_count": 0,
      "liked_by_client": false
    }
    """.trimIndent()

private fun ArtistEntity.toArtistJson(): String =
    """
    {
      "id": ${id.json()},
      "name": ${name.json()},
      "album_count": $albumCount,
      "track_count": $trackCount,
      "artwork_url": ${artworkUrl().jsonNullable()},
      "first_album_id": ${firstAlbumId.json()}
    }
    """.trimIndent()

private fun TrackEntity.artworkUrl(): String? =
    artworkPath?.let { "/api/artwork/local/${it.encodePathSegment()}" }

private fun AlbumEntity.artworkUrl(): String =
    artworkPath?.let { "/api/artwork/local/${it.encodePathSegment()}" }.orEmpty()

private fun ArtistEntity.artworkUrl(): String? =
    artworkPath?.let { "/api/artwork/local/${it.encodePathSegment()}" }

private fun String.json(): String =
    buildString {
        append('"')
        this@json.forEach { char ->
            when (char) {
                '\\' -> append("\\\\")
                '"' -> append("\\\"")
                '\n' -> append("\\n")
                '\r' -> append("\\r")
                '\t' -> append("\\t")
                else -> append(char)
            }
        }
        append('"')
    }

private fun String?.jsonNullable(): String = this?.json() ?: "null"
private fun Number?.jsonNumber(): String = this?.toString() ?: "null"

private fun decodePathSegment(value: String): String =
    URLDecoder.decode(value, StandardCharsets.UTF_8.name())

private fun String.encodePathSegment(): String =
    java.net.URLEncoder.encode(this, StandardCharsets.UTF_8.name()).replace("+", "%20")

const val LOCAL_COMPANION_PORT = 8788
const val LOCAL_COMPANION_BASE_URL = "http://127.0.0.1:$LOCAL_COMPANION_PORT"
const val LOCAL_COMPANION_RENDERER = "android-local://this-device"
