use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension};

mod groups;
mod library;
mod playback;
mod queue;
mod recommendations;
mod renderers;

pub(crate) type SqlitePool = Pool<SqliteConnectionManager>;
pub(crate) type SqliteConn = PooledConnection<SqliteConnectionManager>;

#[derive(Debug)]
pub(crate) struct Database {
    pub(super) pool: SqlitePool,
}

impl Database {
    pub(crate) fn open(config_path: &Path) -> io::Result<Self> {
        fs::create_dir_all(config_path)?;
        let path = config_path.join("musicd.db");
        let manager = SqliteConnectionManager::file(&path).with_init(|connection| {
            connection.pragma_update(None, "journal_mode", "WAL")?;
            connection.pragma_update(None, "synchronous", "NORMAL")?;
            connection.pragma_update(None, "foreign_keys", true)?;
            connection.busy_timeout(Duration::from_secs(2))?;
            Ok(())
        });
        let pool = Pool::builder()
            .max_size(8)
            .build(manager)
            .map_err(|error| io::Error::other(format!("failed to build sqlite pool: {error}")))?;
        let database = Self { pool };
        database.initialize()?;
        Ok(database)
    }

    fn initialize(&self) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS tracks (
                    id TEXT PRIMARY KEY,
                    album_id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    artist TEXT NOT NULL,
                    album TEXT NOT NULL,
                    disc_number INTEGER,
                    track_number INTEGER,
                    duration_seconds INTEGER,
                    relative_path TEXT NOT NULL,
                    path TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    file_size INTEGER NOT NULL,
                    artwork_cache_key TEXT,
                    artwork_source TEXT,
                    artwork_mime_type TEXT
                );

                CREATE TABLE IF NOT EXISTS albums (
                    id TEXT PRIMARY KEY,
                    artist_id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    artist_name TEXT NOT NULL,
                    track_count INTEGER NOT NULL,
                    artwork_track_id TEXT,
                    artwork_cache_key TEXT,
                    artwork_source TEXT,
                    artwork_mime_type TEXT,
                    first_track_id TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS artists (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    album_count INTEGER NOT NULL,
                    track_count INTEGER NOT NULL,
                    artwork_track_id TEXT,
                    first_album_id TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS renderers (
                    location TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    manufacturer TEXT,
                    model_name TEXT,
                    av_transport_control_url TEXT,
                    av_transport_actions_json TEXT,
                    has_playlist_extension_service INTEGER,
                    visibility TEXT NOT NULL DEFAULT 'public',
                    owner_client_id TEXT,
                    last_checked_unix INTEGER NOT NULL DEFAULT 0,
                    last_reachable_unix INTEGER,
                    last_error TEXT,
                    last_seen_unix INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS renderer_groups (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    created_unix INTEGER NOT NULL,
                    updated_unix INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS renderer_group_members (
                    group_id TEXT NOT NULL,
                    renderer_location TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    joined_unix INTEGER NOT NULL,
                    PRIMARY KEY(group_id, renderer_location),
                    FOREIGN KEY(group_id) REFERENCES renderer_groups(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS app_state (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS playback_queues (
                    renderer_location TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    current_entry_id INTEGER,
                    status TEXT NOT NULL,
                    version INTEGER NOT NULL DEFAULT 1,
                    updated_unix INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS queue_entries (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    renderer_location TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    track_id TEXT NOT NULL,
                    album_id TEXT,
                    source_kind TEXT NOT NULL,
                    source_ref TEXT,
                    entry_status TEXT NOT NULL,
                    started_unix INTEGER,
                    completed_unix INTEGER
                );

                CREATE TABLE IF NOT EXISTS playback_sessions (
                    renderer_location TEXT PRIMARY KEY,
                    queue_entry_id INTEGER,
                    next_queue_entry_id INTEGER,
                    transport_state TEXT NOT NULL,
                    current_track_uri TEXT,
                    position_seconds INTEGER,
                    duration_seconds INTEGER,
                    last_observed_unix INTEGER NOT NULL DEFAULT 0,
                    last_error TEXT
                );

                CREATE TABLE IF NOT EXISTS track_play_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    track_id TEXT NOT NULL,
                    renderer_location TEXT NOT NULL,
                    queue_entry_id INTEGER,
                    played_unix INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS album_artwork_overrides (
                    album_id TEXT PRIMARY KEY,
                    cache_key TEXT NOT NULL,
                    source TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    musicbrainz_release_id TEXT,
                    applied_unix INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS album_recommendations (
                    recommendation_key TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    batch_id TEXT,
                    seed_album_id TEXT NOT NULL,
                    seed_musicbrainz_release_id TEXT,
                    suggested_artist TEXT NOT NULL,
                    suggested_title TEXT NOT NULL,
                    suggested_musicbrainz_release_id TEXT,
                    suggested_musicbrainz_release_group_id TEXT,
                    confidence REAL,
                    rationale TEXT,
                    external_url TEXT,
                    artwork_url TEXT,
                    status TEXT NOT NULL DEFAULT 'suggested',
                    created_unix INTEGER NOT NULL,
                    updated_unix INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_track_play_history_track_id
                ON track_play_history(track_id, played_unix DESC);

                CREATE INDEX IF NOT EXISTS idx_track_play_history_renderer
                ON track_play_history(renderer_location, played_unix DESC);

                CREATE INDEX IF NOT EXISTS idx_renderer_group_members_group
                ON renderer_group_members(group_id, position ASC);

                CREATE INDEX IF NOT EXISTS idx_album_recommendations_seed_album
                ON album_recommendations(seed_album_id, status, updated_unix DESC);
                "#,
            )
            .map_err(db_error)?;
        ensure_column(&connection, "tracks", "album_id", "TEXT")?;
        ensure_column(&connection, "tracks", "disc_number", "INTEGER")?;
        ensure_column(&connection, "tracks", "track_number", "INTEGER")?;
        ensure_column(&connection, "tracks", "duration_seconds", "INTEGER")?;
        ensure_column(&connection, "tracks", "artwork_cache_key", "TEXT")?;
        ensure_column(&connection, "tracks", "artwork_source", "TEXT")?;
        ensure_column(&connection, "tracks", "artwork_mime_type", "TEXT")?;
        ensure_column(&connection, "albums", "artist_id", "TEXT")?;
        ensure_column(&connection, "albums", "title", "TEXT")?;
        ensure_column(&connection, "albums", "artist_name", "TEXT")?;
        ensure_column(&connection, "albums", "track_count", "INTEGER")?;
        ensure_column(&connection, "albums", "artwork_track_id", "TEXT")?;
        ensure_column(&connection, "albums", "artwork_cache_key", "TEXT")?;
        ensure_column(&connection, "albums", "artwork_source", "TEXT")?;
        ensure_column(&connection, "albums", "artwork_mime_type", "TEXT")?;
        ensure_column(&connection, "albums", "first_track_id", "TEXT")?;
        ensure_column(&connection, "artists", "album_count", "INTEGER")?;
        ensure_column(&connection, "artists", "track_count", "INTEGER")?;
        ensure_column(&connection, "artists", "artwork_track_id", "TEXT")?;
        ensure_column(&connection, "artists", "first_album_id", "TEXT")?;
        ensure_column(
            &connection,
            "renderers",
            "av_transport_actions_json",
            "TEXT",
        )?;
        ensure_column(
            &connection,
            "renderers",
            "has_playlist_extension_service",
            "INTEGER",
        )?;
        ensure_column(
            &connection,
            "renderers",
            "visibility",
            "TEXT NOT NULL DEFAULT 'public'",
        )?;
        ensure_column(&connection, "renderers", "owner_client_id", "TEXT")?;
        ensure_column(
            &connection,
            "renderers",
            "last_checked_unix",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(&connection, "renderers", "last_reachable_unix", "INTEGER")?;
        ensure_column(&connection, "renderers", "last_error", "TEXT")?;
        connection
            .execute(
                "UPDATE renderers
                 SET last_reachable_unix = last_seen_unix
                 WHERE last_reachable_unix IS NULL AND last_seen_unix > 0",
                [],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "UPDATE renderers
                 SET last_checked_unix = last_seen_unix
                 WHERE last_checked_unix = 0 AND last_seen_unix > 0",
                [],
            )
            .map_err(db_error)?;
        ensure_column(
            &connection,
            "playback_sessions",
            "next_queue_entry_id",
            "INTEGER",
        )?;
        ensure_column(
            &connection,
            "album_artwork_overrides",
            "musicbrainz_release_id",
            "TEXT",
        )?;
        if table_is_empty(&connection, "albums")? && !table_is_empty(&connection, "tracks")? {
            library::rebuild_normalized_library_tables(&connection)?;
        }
        Ok(())
    }

    pub(super) fn connection(&self) -> io::Result<SqliteConn> {
        self.pool.get().map_err(|error| {
            io::Error::other(format!("failed to acquire sqlite connection: {error}"))
        })
    }
}

pub(super) fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> io::Result<()> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut statement = connection.prepare(&pragma).map_err(db_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(db_error)?;

    for row in rows {
        if row.map_err(db_error)? == column {
            return Ok(());
        }
    }

    let alter = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
    connection.execute(&alter, []).map_err(db_error)?;
    Ok(())
}

pub(super) fn table_is_empty(connection: &Connection, table: &str) -> io::Result<bool> {
    let query = format!("SELECT 1 FROM {table} LIMIT 1");
    connection
        .query_row(&query, [], |_| Ok(()))
        .optional()
        .map(|row| row.is_none())
        .map_err(db_error)
}

pub(super) fn db_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(format!("sqlite error: {error}"))
}
