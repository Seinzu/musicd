use std::io;

use rusqlite::{OptionalExtension, params};

use crate::types::{DirectStreamMetadata, PlaybackSession, TrackPlayRecord};
use crate::util::now_unix_timestamp;

use super::{Database, db_error};

impl Database {
    pub(crate) fn load_playback_session(
        &self,
        renderer_location: &str,
    ) -> io::Result<Option<PlaybackSession>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                        position_seconds, duration_seconds, last_observed_unix, last_error
                 FROM playback_sessions
                 WHERE renderer_location = ?",
                [renderer_location],
                |row| {
                    Ok(PlaybackSession {
                        renderer_location: row.get(0)?,
                        queue_entry_id: row.get(1)?,
                        next_queue_entry_id: row.get(2)?,
                        transport_state: row.get(3)?,
                        current_track_uri: row.get(4)?,
                        position_seconds: row.get::<_, Option<u64>>(5)?,
                        duration_seconds: row.get::<_, Option<u64>>(6)?,
                        last_observed_unix: row.get(7)?,
                        last_error: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    pub(crate) fn load_direct_stream_metadata(
        &self,
        renderer_location: &str,
    ) -> io::Result<Option<DirectStreamMetadata>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT renderer_location, current_track_uri, title, artwork_url, updated_unix
                 FROM direct_stream_metadata
                 WHERE renderer_location = ?",
                [renderer_location],
                |row| {
                    Ok(DirectStreamMetadata {
                        renderer_location: row.get(0)?,
                        current_track_uri: row.get(1)?,
                        title: row.get(2)?,
                        artwork_url: row.get(3)?,
                        updated_unix: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    #[allow(dead_code)]
    pub(crate) fn count_track_plays(&self, track_id: &str) -> io::Result<u64> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT COUNT(*) FROM track_play_history WHERE track_id = ?",
                [track_id],
                |row| row.get::<_, u64>(0),
            )
            .map_err(db_error)
    }

    #[allow(dead_code)]
    pub(crate) fn load_track_play_history(
        &self,
        track_id: &str,
    ) -> io::Result<Vec<TrackPlayRecord>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, track_id, renderer_location, queue_entry_id, played_unix
                 FROM track_play_history
                 WHERE track_id = ?
                 ORDER BY played_unix DESC, id DESC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([track_id], |row| {
                Ok(TrackPlayRecord {
                    id: row.get(0)?,
                    track_id: row.get(1)?,
                    renderer_location: row.get(2)?,
                    queue_entry_id: row.get(3)?,
                    played_unix: row.get(4)?,
                })
            })
            .map_err(db_error)?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(db_error)?);
        }
        Ok(records)
    }

    pub(crate) fn load_recent_track_play_history(
        &self,
        limit: usize,
    ) -> io::Result<Vec<TrackPlayRecord>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, track_id, renderer_location, queue_entry_id, played_unix
                 FROM track_play_history
                 ORDER BY played_unix DESC, id DESC
                 LIMIT ?",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
                Ok(TrackPlayRecord {
                    id: row.get(0)?,
                    track_id: row.get(1)?,
                    renderer_location: row.get(2)?,
                    queue_entry_id: row.get(3)?,
                    played_unix: row.get(4)?,
                })
            })
            .map_err(db_error)?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(db_error)?);
        }
        Ok(records)
    }

    pub(crate) fn mark_queue_play_started(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
        track_id: &str,
        current_track_uri: &str,
        duration_seconds: Option<u64>,
    ) -> io::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let now = now_unix_timestamp();
        transaction
            .execute(
                "UPDATE queue_entries
                 SET entry_status = 'playing', started_unix = COALESCE(started_unix, ?), completed_unix = NULL
                 WHERE id = ?",
                params![now, queue_entry_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE queue_entries
                 SET entry_status = CASE
                    WHEN id = ? THEN entry_status
                    WHEN completed_unix IS NOT NULL THEN 'completed'
                    ELSE 'pending'
                 END
                 WHERE renderer_location = ?",
                params![queue_entry_id, renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE playback_queues
                 SET current_entry_id = ?, status = 'playing', updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![queue_entry_id, now, renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (?, ?, NULL, 'PLAYING', ?, 0, ?, ?, NULL)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    queue_entry_id = excluded.queue_entry_id,
                    next_queue_entry_id = excluded.next_queue_entry_id,
                    transport_state = excluded.transport_state,
                    current_track_uri = excluded.current_track_uri,
                    position_seconds = excluded.position_seconds,
                    duration_seconds = excluded.duration_seconds,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    queue_entry_id,
                    current_track_uri,
                    duration_seconds,
                    now
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM direct_stream_metadata WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO track_play_history
                 (track_id, renderer_location, queue_entry_id, played_unix)
                 VALUES (?, ?, ?, ?)",
                params![track_id, renderer_location, queue_entry_id, now],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn adopt_next_queue_entry_as_current(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
        track_id: &str,
        current_track_uri: &str,
        duration_seconds: Option<u64>,
    ) -> io::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let now = now_unix_timestamp();
        let previous_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();

        if let Some(previous_entry_id) = previous_entry_id {
            transaction
                .execute(
                    "UPDATE queue_entries
                     SET entry_status = 'completed',
                         completed_unix = COALESCE(completed_unix, ?)
                     WHERE id = ?",
                    params![now, previous_entry_id],
                )
                .map_err(db_error)?;
        }
        transaction
            .execute(
                "UPDATE queue_entries
                 SET entry_status = 'playing', started_unix = COALESCE(started_unix, ?), completed_unix = NULL
                 WHERE id = ?",
                params![now, queue_entry_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE queue_entries
                 SET entry_status = CASE
                    WHEN id = ? THEN entry_status
                    WHEN completed_unix IS NOT NULL THEN 'completed'
                    ELSE 'pending'
                 END
                 WHERE renderer_location = ?",
                params![queue_entry_id, renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE playback_queues
                 SET current_entry_id = ?, status = 'playing', updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![queue_entry_id, now, renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (?, ?, NULL, 'PLAYING', ?, 0, ?, ?, NULL)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    queue_entry_id = excluded.queue_entry_id,
                    next_queue_entry_id = excluded.next_queue_entry_id,
                    transport_state = excluded.transport_state,
                    current_track_uri = excluded.current_track_uri,
                    position_seconds = excluded.position_seconds,
                    duration_seconds = excluded.duration_seconds,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    queue_entry_id,
                    current_track_uri,
                    duration_seconds,
                    now
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM direct_stream_metadata WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO track_play_history
                 (track_id, renderer_location, queue_entry_id, played_unix)
                 VALUES (?, ?, ?, ?)",
                params![track_id, renderer_location, queue_entry_id, now],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn mark_queue_play_error(
        &self,
        renderer_location: &str,
        queue_entry_id: Option<i64>,
        error: &str,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (?, ?, NULL, 'ERROR', NULL, NULL, NULL, ?, ?)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    queue_entry_id = excluded.queue_entry_id,
                    next_queue_entry_id = excluded.next_queue_entry_id,
                    transport_state = excluded.transport_state,
                    current_track_uri = excluded.current_track_uri,
                    position_seconds = excluded.position_seconds,
                    duration_seconds = excluded.duration_seconds,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    queue_entry_id,
                    now_unix_timestamp(),
                    error
                ],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "UPDATE playback_queues
                 SET status = 'error', updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![now_unix_timestamp(), renderer_location],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn mark_direct_stream_play_started(
        &self,
        renderer_location: &str,
        current_track_uri: &str,
        title: &str,
        artwork_url: Option<&str>,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        let now = now_unix_timestamp();
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (?, NULL, NULL, 'PLAYING', ?, 0, NULL, ?, NULL)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    queue_entry_id = NULL,
                    next_queue_entry_id = NULL,
                    transport_state = excluded.transport_state,
                    current_track_uri = excluded.current_track_uri,
                    position_seconds = excluded.position_seconds,
                    duration_seconds = excluded.duration_seconds,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![renderer_location, current_track_uri, now],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "INSERT INTO direct_stream_metadata
                 (renderer_location, current_track_uri, title, artwork_url, updated_unix)
                 VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    current_track_uri = excluded.current_track_uri,
                    title = excluded.title,
                    artwork_url = excluded.artwork_url,
                    updated_unix = excluded.updated_unix",
                params![
                    renderer_location,
                    current_track_uri,
                    title,
                    artwork_url,
                    now
                ],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "UPDATE playback_queues
                 SET status = 'ready', updated_unix = ?, version = version + 1
                 WHERE renderer_location = ? AND status = 'playing'",
                params![now, renderer_location],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn record_transport_snapshot(
        &self,
        renderer_location: &str,
        transport_state: &str,
        current_track_uri: Option<&str>,
        position_seconds: Option<u64>,
        duration_seconds: Option<u64>,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (
                    ?,
                    (SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?),
                    (SELECT next_queue_entry_id FROM playback_sessions WHERE renderer_location = ?),
                    ?, ?, ?, ?, ?, NULL
                 )
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    transport_state = excluded.transport_state,
                    current_track_uri = excluded.current_track_uri,
                    position_seconds = excluded.position_seconds,
                    duration_seconds = excluded.duration_seconds,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    transport_state,
                    current_track_uri,
                    position_seconds,
                    duration_seconds,
                    now_unix_timestamp(),
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn record_transport_poll_error(
        &self,
        renderer_location: &str,
        error: &str,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (
                    ?,
                    (SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?),
                    (SELECT next_queue_entry_id FROM playback_sessions WHERE renderer_location = ?),
                    'ERROR', NULL, NULL, NULL, ?, ?
                 )
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    transport_state = excluded.transport_state,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    now_unix_timestamp(),
                    error
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn record_playback_session_warning(
        &self,
        renderer_location: &str,
        warning: &str,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (
                    ?,
                    (SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?),
                    (SELECT next_queue_entry_id FROM playback_sessions WHERE renderer_location = ?),
                    COALESCE((SELECT transport_state FROM playback_sessions WHERE renderer_location = ?), 'READY'),
                    (SELECT current_track_uri FROM playback_sessions WHERE renderer_location = ?),
                    (SELECT position_seconds FROM playback_sessions WHERE renderer_location = ?),
                    (SELECT duration_seconds FROM playback_sessions WHERE renderer_location = ?),
                    ?,
                    ?
                 )
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    now_unix_timestamp(),
                    warning,
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn mark_next_queue_entry_preloaded(
        &self,
        renderer_location: &str,
        next_queue_entry_id: Option<i64>,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (
                    ?,
                    (SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?),
                    ?,
                    COALESCE((SELECT transport_state FROM playback_sessions WHERE renderer_location = ?), 'READY'),
                    (SELECT current_track_uri FROM playback_sessions WHERE renderer_location = ?),
                    (SELECT position_seconds FROM playback_sessions WHERE renderer_location = ?),
                    (SELECT duration_seconds FROM playback_sessions WHERE renderer_location = ?),
                    ?,
                    (SELECT last_error FROM playback_sessions WHERE renderer_location = ?)
                 )
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    next_queue_entry_id = excluded.next_queue_entry_id,
                    last_observed_unix = excluded.last_observed_unix",
                params![
                    renderer_location,
                    renderer_location,
                    next_queue_entry_id,
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    now_unix_timestamp(),
                    renderer_location,
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }
}
