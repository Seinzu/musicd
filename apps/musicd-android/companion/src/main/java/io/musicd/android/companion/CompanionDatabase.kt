package io.musicd.android.companion

import android.content.Context
import androidx.room.Dao
import androidx.room.Database
import androidx.room.Entity
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.PrimaryKey
import androidx.room.Query
import androidx.room.Room
import androidx.room.RoomDatabase
import androidx.room.Transaction
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase

@Entity(tableName = "storage_roots")
data class StorageRootEntity(
    @PrimaryKey val uri: String,
    val label: String,
    val lastScanStatus: String = "never_scanned",
    val lastScanStartedUnix: Long? = null,
    val lastScanFinishedUnix: Long? = null,
    val lastScanError: String? = null,
    val trackCount: Int = 0,
    val scanFoldersVisited: Int = 0,
    val scanFilesVisited: Int = 0,
    val scanTracksFound: Int = 0,
    val scanFilesIgnored: Int = 0,
    val scanCurrentItem: String? = null,
    val scanLastProgressUnix: Long? = null,
)

@Entity(tableName = "tracks")
data class TrackEntity(
    @PrimaryKey val id: String,
    val rootUri: String,
    val contentUri: String,
    val albumId: String,
    val artistId: String,
    val title: String,
    val artist: String,
    val album: String,
    val discNumber: Int?,
    val trackNumber: Int?,
    val durationSeconds: Long?,
    val mimeType: String?,
    val size: Long,
    val lastModified: Long,
    val artworkPath: String?,
)

@Entity(tableName = "albums")
data class AlbumEntity(
    @PrimaryKey val id: String,
    val title: String,
    val artist: String,
    val artistId: String,
    val trackCount: Int,
    val firstTrackId: String,
    val artworkPath: String?,
)

@Entity(tableName = "artists")
data class ArtistEntity(
    @PrimaryKey val id: String,
    val name: String,
    val albumCount: Int,
    val trackCount: Int,
    val firstAlbumId: String,
    val artworkPath: String?,
)

@Entity(tableName = "queue_entries")
data class QueueEntryEntity(
    @PrimaryKey(autoGenerate = true) val id: Long = 0,
    val position: Long,
    val trackId: String,
    val entryStatus: String,
)

@Entity(tableName = "playback_session")
data class PlaybackSessionEntity(
    @PrimaryKey val rendererLocation: String = LOCAL_COMPANION_RENDERER,
    val transportState: String,
    val queueEntryId: Long?,
    val currentTrackUri: String?,
    val positionSeconds: Long?,
    val durationSeconds: Long?,
    val lastObservedUnix: Long,
)

@Dao
interface StorageRootDao {
    @Query("SELECT * FROM storage_roots ORDER BY label COLLATE NOCASE")
    suspend fun roots(): List<StorageRootEntity>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsert(root: StorageRootEntity)

    @Query("DELETE FROM storage_roots WHERE uri = :uri")
    suspend fun delete(uri: String)

    @Query(
        """
        UPDATE storage_roots
        SET lastScanStatus = :status,
            lastScanStartedUnix = :startedUnix,
            lastScanFinishedUnix = :finishedUnix,
            lastScanError = :error,
            trackCount = :trackCount,
            scanFoldersVisited = :foldersVisited,
            scanFilesVisited = :filesVisited,
            scanTracksFound = :tracksFound,
            scanFilesIgnored = :filesIgnored,
            scanCurrentItem = :currentItem,
            scanLastProgressUnix = :progressUnix
        WHERE uri = :uri
        """,
    )
    suspend fun updateScanState(
        uri: String,
        status: String,
        startedUnix: Long?,
        finishedUnix: Long?,
        error: String?,
        trackCount: Int,
        foldersVisited: Int,
        filesVisited: Int,
        tracksFound: Int,
        filesIgnored: Int,
        currentItem: String?,
        progressUnix: Long?,
    )

    @Query(
        """
        UPDATE storage_roots
        SET scanFoldersVisited = :foldersVisited,
            scanFilesVisited = :filesVisited,
            scanTracksFound = :tracksFound,
            scanFilesIgnored = :filesIgnored,
            scanCurrentItem = :currentItem,
            scanLastProgressUnix = :progressUnix
        WHERE uri = :uri
        """,
    )
    suspend fun updateScanProgress(
        uri: String,
        foldersVisited: Int,
        filesVisited: Int,
        tracksFound: Int,
        filesIgnored: Int,
        currentItem: String?,
        progressUnix: Long,
    )

    @Query(
        """
        UPDATE storage_roots
        SET lastScanStatus = 'canceled',
            lastScanFinishedUnix = :finishedUnix,
            lastScanError = null,
            scanCurrentItem = null,
            scanLastProgressUnix = :finishedUnix
        WHERE lastScanStatus = 'scanning'
        """,
    )
    suspend fun cancelActiveScans(finishedUnix: Long)
}

@Dao
interface LibraryDao {
    @Query("SELECT * FROM tracks ORDER BY artist COLLATE NOCASE, album COLLATE NOCASE, discNumber, trackNumber, title COLLATE NOCASE")
    suspend fun tracks(): List<TrackEntity>

    @Query("SELECT * FROM tracks WHERE id = :trackId LIMIT 1")
    suspend fun track(trackId: String): TrackEntity?

    @Query("SELECT * FROM tracks WHERE rootUri = :rootUri")
    suspend fun tracksForRoot(rootUri: String): List<TrackEntity>

    @Query("SELECT * FROM tracks WHERE albumId = :albumId ORDER BY discNumber, trackNumber, title COLLATE NOCASE")
    suspend fun tracksForAlbum(albumId: String): List<TrackEntity>

    @Query("SELECT * FROM albums ORDER BY artist COLLATE NOCASE, title COLLATE NOCASE")
    suspend fun albums(): List<AlbumEntity>

    @Query("SELECT * FROM albums WHERE id = :albumId LIMIT 1")
    suspend fun album(albumId: String): AlbumEntity?

    @Query("SELECT * FROM albums WHERE artistId = :artistId ORDER BY title COLLATE NOCASE")
    suspend fun albumsForArtist(artistId: String): List<AlbumEntity>

    @Query("SELECT * FROM artists ORDER BY name COLLATE NOCASE")
    suspend fun artists(): List<ArtistEntity>

    @Query("SELECT * FROM artists WHERE id = :artistId LIMIT 1")
    suspend fun artist(artistId: String): ArtistEntity?

    @Query("DELETE FROM tracks WHERE rootUri = :rootUri")
    suspend fun deleteTracksForRoot(rootUri: String)

    @Query("DELETE FROM albums")
    suspend fun clearAlbums()

    @Query("DELETE FROM artists")
    suspend fun clearArtists()

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insertTracks(tracks: List<TrackEntity>)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insertAlbums(albums: List<AlbumEntity>)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insertArtists(artists: List<ArtistEntity>)

    @Transaction
    suspend fun replaceRootTracks(rootUri: String, tracks: List<TrackEntity>) {
        deleteTracksForRoot(rootUri)
        tracks.chunked(250).forEach { chunk -> insertTracks(chunk) }
    }

    @Transaction
    suspend fun replaceSummaries(albums: List<AlbumEntity>, artists: List<ArtistEntity>) {
        clearAlbums()
        clearArtists()
        insertAlbums(albums)
        insertArtists(artists)
    }
}

@Dao
interface QueueDao {
    @Query("SELECT * FROM queue_entries ORDER BY position, id")
    suspend fun entries(): List<QueueEntryEntity>

    @Query("SELECT * FROM queue_entries WHERE id = :entryId LIMIT 1")
    suspend fun entry(entryId: Long): QueueEntryEntity?

    @Query("SELECT * FROM queue_entries WHERE entryStatus = 'playing' ORDER BY position, id LIMIT 1")
    suspend fun currentEntry(): QueueEntryEntity?

    @Query("SELECT * FROM queue_entries WHERE entryStatus != 'completed' ORDER BY position, id LIMIT 1")
    suspend fun firstPlayableEntry(): QueueEntryEntity?

    @Query("SELECT * FROM queue_entries WHERE position > :position AND entryStatus != 'completed' ORDER BY position, id LIMIT 1")
    suspend fun nextEntryAfter(position: Long): QueueEntryEntity?

    @Query("SELECT COALESCE(MAX(position), -1) + 1 FROM queue_entries")
    suspend fun nextPosition(): Long

    @Query("DELETE FROM queue_entries")
    suspend fun clear()

    @Query("DELETE FROM queue_entries WHERE id = :entryId")
    suspend fun delete(entryId: Long)

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(entry: QueueEntryEntity): Long

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(entries: List<QueueEntryEntity>): List<Long>

    @Query("UPDATE queue_entries SET entryStatus = :status WHERE id = :entryId")
    suspend fun updateStatus(entryId: Long, status: String)

    @Query("UPDATE queue_entries SET entryStatus = 'queued' WHERE entryStatus = 'playing'")
    suspend fun clearPlayingStatus()

    @Query("UPDATE queue_entries SET position = :position WHERE id = :entryId")
    suspend fun updatePosition(entryId: Long, position: Long)

    @Query("SELECT * FROM playback_session WHERE rendererLocation = :rendererLocation LIMIT 1")
    suspend fun session(rendererLocation: String = LOCAL_COMPANION_RENDERER): PlaybackSessionEntity?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsertSession(session: PlaybackSessionEntity)
}

@Database(
    entities = [
        StorageRootEntity::class,
        TrackEntity::class,
        AlbumEntity::class,
        ArtistEntity::class,
        QueueEntryEntity::class,
        PlaybackSessionEntity::class,
    ],
    version = 5,
    exportSchema = false,
)
abstract class CompanionDatabase : RoomDatabase() {
    abstract fun storageRoots(): StorageRootDao
    abstract fun library(): LibraryDao
    abstract fun queue(): QueueDao

    companion object {
        @Volatile private var instance: CompanionDatabase? = null
        private val migration1To2 = object : Migration(1, 2) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL(
                    """
                    CREATE TABLE IF NOT EXISTS queue_entries (
                        id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                        position INTEGER NOT NULL,
                        trackId TEXT NOT NULL,
                        entryStatus TEXT NOT NULL
                    )
                    """.trimIndent(),
                )
                db.execSQL(
                    """
                    CREATE TABLE IF NOT EXISTS playback_session (
                        rendererLocation TEXT PRIMARY KEY NOT NULL,
                        transportState TEXT NOT NULL,
                        queueEntryId INTEGER,
                        currentTrackUri TEXT,
                        positionSeconds INTEGER,
                        durationSeconds INTEGER,
                        lastObservedUnix INTEGER NOT NULL
                    )
                    """.trimIndent(),
                )
            }
        }
        private val migration2To3 = object : Migration(2, 3) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE tracks ADD COLUMN artworkPath TEXT")
                db.execSQL("ALTER TABLE albums ADD COLUMN artworkPath TEXT")
                db.execSQL("ALTER TABLE artists ADD COLUMN artworkPath TEXT")
            }
        }
        private val migration3To4 = object : Migration(3, 4) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE storage_roots ADD COLUMN scanFoldersVisited INTEGER NOT NULL DEFAULT 0")
                db.execSQL("ALTER TABLE storage_roots ADD COLUMN scanFilesVisited INTEGER NOT NULL DEFAULT 0")
                db.execSQL("ALTER TABLE storage_roots ADD COLUMN scanTracksFound INTEGER NOT NULL DEFAULT 0")
                db.execSQL("ALTER TABLE storage_roots ADD COLUMN scanFilesIgnored INTEGER NOT NULL DEFAULT 0")
            }
        }
        private val migration4To5 = object : Migration(4, 5) {
            override fun migrate(db: SupportSQLiteDatabase) {
                db.execSQL("ALTER TABLE storage_roots ADD COLUMN scanCurrentItem TEXT")
                db.execSQL("ALTER TABLE storage_roots ADD COLUMN scanLastProgressUnix INTEGER")
            }
        }

        fun get(context: Context): CompanionDatabase =
            instance ?: synchronized(this) {
                instance ?: Room.databaseBuilder(
                    context.applicationContext,
                    CompanionDatabase::class.java,
                    "musicd_companion.db",
                )
                    .addMigrations(migration1To2, migration2To3, migration3To4, migration4To5)
                    .build()
                    .also { instance = it }
            }
    }
}
