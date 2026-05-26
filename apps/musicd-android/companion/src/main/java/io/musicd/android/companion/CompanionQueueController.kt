package io.musicd.android.companion

class CompanionQueueController(
    private val database: CompanionDatabase,
    private val playback: CompanionPlaybackRuntime,
) {
    private val library = database.library()
    private val queue = database.queue()

    suspend fun playTrack(trackId: String): MutationResult {
        val track = library.track(trackId) ?: return MutationResult.error("Track not found.")
        queue.clear()
        val id = queue.insert(QueueEntryEntity(position = 0, trackId = track.id, entryStatus = "playing"))
        playback.startEntry(QueueEntryEntity(id = id, position = 0, trackId = track.id, entryStatus = "playing"))
        return MutationResult.ok("Playing ${track.title}.")
    }

    suspend fun playAlbum(albumId: String): MutationResult {
        val tracks = library.tracksForAlbum(albumId)
        if (tracks.isEmpty()) return MutationResult.error("Album not found.")
        queue.clear()
        val entries = tracks.mapIndexed { index, track ->
            QueueEntryEntity(position = index.toLong(), trackId = track.id, entryStatus = if (index == 0) "playing" else "queued")
        }
        val ids = queue.insert(entries)
        playback.startEntry(entries.first().copy(id = ids.first()))
        return MutationResult.ok("Playing ${tracks.first().album}.")
    }

    suspend fun appendTrack(trackId: String): MutationResult {
        val track = library.track(trackId) ?: return MutationResult.error("Track not found.")
        val position = queue.nextPosition()
        queue.insert(QueueEntryEntity(position = position, trackId = track.id, entryStatus = "queued"))
        return MutationResult.ok("Queued ${track.title}.")
    }

    suspend fun playNextTrack(trackId: String): MutationResult {
        val track = library.track(trackId) ?: return MutationResult.error("Track not found.")
        insertAfterCurrent(listOf(track))
        return MutationResult.ok("Queued ${track.title} next.")
    }

    suspend fun appendAlbum(albumId: String): MutationResult {
        val tracks = library.tracksForAlbum(albumId)
        if (tracks.isEmpty()) return MutationResult.error("Album not found.")
        val start = queue.nextPosition()
        queue.insert(
            tracks.mapIndexed { index, track ->
                QueueEntryEntity(position = start + index, trackId = track.id, entryStatus = "queued")
            },
        )
        return MutationResult.ok("Queued ${tracks.first().album}.")
    }

    suspend fun playNextAlbum(albumId: String): MutationResult {
        val tracks = library.tracksForAlbum(albumId)
        if (tracks.isEmpty()) return MutationResult.error("Album not found.")
        insertAfterCurrent(tracks)
        return MutationResult.ok("Queued ${tracks.first().album} next.")
    }

    suspend fun move(entryId: Long, direction: String): MutationResult {
        val entries = queue.entries()
        val index = entries.indexOfFirst { it.id == entryId }
        if (index < 0) return MutationResult.error("Queue entry not found.")
        val swapIndex = when (direction) {
            "up" -> index - 1
            "down" -> index + 1
            else -> return MutationResult.error("Unsupported move direction.")
        }
        if (swapIndex !in entries.indices) return MutationResult.ok("Queue order unchanged.")
        val left = entries[index]
        val right = entries[swapIndex]
        queue.updatePosition(left.id, right.position)
        queue.updatePosition(right.id, left.position)
        return MutationResult.ok("Queue order updated.")
    }

    suspend fun remove(entryId: Long): MutationResult {
        val wasCurrent = queue.currentEntry()?.id == entryId
        queue.delete(entryId)
        if (wasCurrent) playback.next()
        return MutationResult.ok("Queue entry removed.")
    }

    suspend fun clear(): MutationResult {
        queue.clear()
        playback.stop()
        return MutationResult.ok("Queue cleared.")
    }

    suspend fun transportPlay(): MutationResult {
        playback.playCurrent()
        return MutationResult.ok("Playback started.")
    }

    suspend fun transportPause(): MutationResult {
        playback.pause()
        return MutationResult.ok("Playback paused.")
    }

    suspend fun transportStop(): MutationResult {
        playback.stop()
        return MutationResult.ok("Playback stopped.")
    }

    suspend fun transportNext(): MutationResult {
        playback.next()
        return MutationResult.ok("Skipped to next track.")
    }

    suspend fun transportPrevious(): MutationResult {
        playback.previous()
        return MutationResult.ok("Skipped to previous track.")
    }

    private suspend fun insertAfterCurrent(tracks: List<TrackEntity>) {
        val entries = queue.entries()
        val current = queue.currentEntry()
        val insertAfter = current?.position ?: -1L
        val shifted = entries.filter { it.position > insertAfter }
        shifted.sortedByDescending { it.position }.forEach { entry ->
            queue.updatePosition(entry.id, entry.position + tracks.size)
        }
        queue.insert(
            tracks.mapIndexed { index, track ->
                QueueEntryEntity(position = insertAfter + 1 + index, trackId = track.id, entryStatus = "queued")
            },
        )
    }
}

data class MutationResult(
    val ok: Boolean,
    val message: String?,
    val error: String?,
) {
    companion object {
        fun ok(message: String) = MutationResult(ok = true, message = message, error = null)
        fun error(error: String) = MutationResult(ok = false, message = null, error = error)
    }
}
