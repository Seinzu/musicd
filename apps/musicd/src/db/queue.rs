use std::io;

use rusqlite::{Connection, OptionalExtension, params};

use crate::types::{PlaybackQueue, QueueEntry, QueueMutationEntry};
use crate::util::now_unix_timestamp;

use super::{Database, db_error};

impl Database {
    pub(crate) fn load_queue(&self, renderer_location: &str) -> io::Result<Option<PlaybackQueue>> {
        let connection = self.connection()?;
        Self::load_queue_with(&connection, renderer_location)
    }

    pub(super) fn load_queue_with(
        connection: &Connection,
        renderer_location: &str,
    ) -> io::Result<Option<PlaybackQueue>> {
        let queue_row = connection
            .query_row(
                "SELECT renderer_location, name, current_entry_id, status, version, updated_unix
                 FROM playback_queues
                 WHERE renderer_location = ?",
                [renderer_location],
                |row| {
                    Ok(PlaybackQueue {
                        renderer_location: row.get(0)?,
                        name: row.get(1)?,
                        current_entry_id: row.get(2)?,
                        status: row.get(3)?,
                        version: row.get(4)?,
                        updated_unix: row.get(5)?,
                        entries: Vec::new(),
                    })
                },
            )
            .optional()
            .map_err(db_error)?;

        let Some(mut queue) = queue_row else {
            return Ok(None);
        };

        let mut statement = connection
            .prepare(
                "SELECT id, position, track_id, album_id, source_kind, source_ref,
                        entry_status, started_unix, completed_unix
                 FROM queue_entries
                 WHERE renderer_location = ?
                 ORDER BY position ASC, id ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([renderer_location], |row| {
                Ok(QueueEntry {
                    id: row.get(0)?,
                    position: row.get(1)?,
                    track_id: row.get(2)?,
                    album_id: row.get(3)?,
                    source_kind: row.get(4)?,
                    source_ref: row.get(5)?,
                    entry_status: row.get(6)?,
                    started_unix: row.get(7)?,
                    completed_unix: row.get(8)?,
                })
            })
            .map_err(db_error)?;

        for row in rows {
            queue.entries.push(row.map_err(db_error)?);
        }

        Ok(Some(queue))
    }

    pub(crate) fn replace_queue(
        &self,
        renderer_location: &str,
        name: &str,
        entries: &[QueueMutationEntry],
    ) -> io::Result<PlaybackQueue> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_version = transaction
            .query_row(
                "SELECT version FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?
            .unwrap_or(0);
        transaction
            .execute(
                "DELETE FROM queue_entries WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;

        let mut current_entry_id = None;
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO queue_entries
                     (renderer_location, position, track_id, album_id, source_kind, source_ref,
                      entry_status, started_unix, completed_unix)
                     VALUES (?, ?, ?, ?, ?, ?, 'pending', NULL, NULL)",
                )
                .map_err(db_error)?;
            for (index, entry) in entries.iter().enumerate() {
                statement
                    .execute(params![
                        renderer_location,
                        i64::try_from(index + 1).unwrap_or(i64::MAX),
                        entry.track_id,
                        entry.album_id,
                        entry.source_kind,
                        entry.source_ref,
                    ])
                    .map_err(db_error)?;
                if index == 0 {
                    current_entry_id = Some(transaction.last_insert_rowid());
                }
            }
        }

        transaction
            .execute(
                "INSERT INTO playback_queues
                 (renderer_location, name, current_entry_id, status, version, updated_unix)
                 VALUES (?, ?, ?, ?, ?, ?)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    name = excluded.name,
                    current_entry_id = excluded.current_entry_id,
                    status = excluded.status,
                    version = excluded.version,
                    updated_unix = excluded.updated_unix",
                params![
                    renderer_location,
                    name,
                    current_entry_id,
                    if entries.is_empty() { "empty" } else { "ready" },
                    current_version + 1,
                    now_unix_timestamp(),
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM playback_sessions WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;

        Self::load_queue_with(&connection, renderer_location)?
            .ok_or_else(|| io::Error::other("queue disappeared after replace"))
    }

    pub(crate) fn append_queue_entries(
        &self,
        renderer_location: &str,
        name: &str,
        entries: &[QueueMutationEntry],
    ) -> io::Result<PlaybackQueue> {
        if entries.is_empty() {
            return self
                .load_queue(renderer_location)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue does not exist"));
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_version = transaction
            .query_row(
                "SELECT version FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?
            .unwrap_or(0);
        let max_position = transaction
            .query_row(
                "SELECT MAX(position) FROM queue_entries WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(db_error)?
            .unwrap_or(0);
        let mut first_inserted_id = None;
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO queue_entries
                     (renderer_location, position, track_id, album_id, source_kind, source_ref,
                      entry_status, started_unix, completed_unix)
                     VALUES (?, ?, ?, ?, ?, ?, 'pending', NULL, NULL)",
                )
                .map_err(db_error)?;
            for (index, entry) in entries.iter().enumerate() {
                statement
                    .execute(params![
                        renderer_location,
                        max_position + i64::try_from(index + 1).unwrap_or(i64::MAX),
                        entry.track_id,
                        entry.album_id,
                        entry.source_kind,
                        entry.source_ref,
                    ])
                    .map_err(db_error)?;
                if index == 0 {
                    first_inserted_id = Some(transaction.last_insert_rowid());
                }
            }
        }

        let existing_current_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();
        transaction
            .execute(
                "INSERT INTO playback_queues
                 (renderer_location, name, current_entry_id, status, version, updated_unix)
                 VALUES (?, ?, ?, 'ready', ?, ?)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    name = excluded.name,
                    current_entry_id = COALESCE(playback_queues.current_entry_id, excluded.current_entry_id),
                    status = CASE
                        WHEN playback_queues.status = 'playing' THEN playback_queues.status
                        ELSE excluded.status
                    END,
                    version = excluded.version,
                    updated_unix = excluded.updated_unix",
                params![
                    renderer_location,
                    name,
                    existing_current_entry_id.or(first_inserted_id),
                    current_version + 1,
                    now_unix_timestamp(),
                ],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;

        Self::load_queue_with(&connection, renderer_location)?
            .ok_or_else(|| io::Error::other("queue disappeared after append"))
    }

    pub(crate) fn insert_queue_entries_after_current(
        &self,
        renderer_location: &str,
        name: &str,
        entries: &[QueueMutationEntry],
    ) -> io::Result<PlaybackQueue> {
        if entries.is_empty() {
            return self
                .load_queue(renderer_location)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue does not exist"));
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_version = transaction
            .query_row(
                "SELECT version FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?
            .unwrap_or(0);
        let existing_current_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();
        let insert_after_position = if let Some(current_entry_id) = existing_current_entry_id {
            transaction
                .query_row(
                    "SELECT position FROM queue_entries WHERE id = ?",
                    [current_entry_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(db_error)?
                .unwrap_or_else(|| {
                    transaction
                        .query_row(
                            "SELECT MAX(position) FROM queue_entries WHERE renderer_location = ?",
                            [renderer_location],
                            |row| row.get::<_, Option<i64>>(0),
                        )
                        .map_err(db_error)
                        .ok()
                        .flatten()
                        .unwrap_or(0)
                })
        } else {
            transaction
                .query_row(
                    "SELECT MAX(position) FROM queue_entries WHERE renderer_location = ?",
                    [renderer_location],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .map_err(db_error)?
                .unwrap_or(0)
        };

        transaction
            .execute(
                "UPDATE queue_entries
                 SET position = position + ?
                 WHERE renderer_location = ?
                   AND position > ?",
                params![
                    i64::try_from(entries.len()).unwrap_or(i64::MAX),
                    renderer_location,
                    insert_after_position
                ],
            )
            .map_err(db_error)?;

        let mut first_inserted_id = None;
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO queue_entries
                     (renderer_location, position, track_id, album_id, source_kind, source_ref,
                      entry_status, started_unix, completed_unix)
                     VALUES (?, ?, ?, ?, ?, ?, 'pending', NULL, NULL)",
                )
                .map_err(db_error)?;
            for (index, entry) in entries.iter().enumerate() {
                statement
                    .execute(params![
                        renderer_location,
                        insert_after_position + i64::try_from(index + 1).unwrap_or(i64::MAX),
                        entry.track_id,
                        entry.album_id,
                        entry.source_kind,
                        entry.source_ref,
                    ])
                    .map_err(db_error)?;
                if index == 0 {
                    first_inserted_id = Some(transaction.last_insert_rowid());
                }
            }
        }

        transaction
            .execute(
                "INSERT INTO playback_queues
                 (renderer_location, name, current_entry_id, status, version, updated_unix)
                 VALUES (?, ?, ?, 'ready', ?, ?)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    name = excluded.name,
                    current_entry_id = COALESCE(playback_queues.current_entry_id, excluded.current_entry_id),
                    status = CASE
                        WHEN playback_queues.status = 'playing' THEN playback_queues.status
                        WHEN playback_queues.status = 'paused' THEN playback_queues.status
                        ELSE excluded.status
                    END,
                    version = excluded.version,
                    updated_unix = excluded.updated_unix",
                params![
                    renderer_location,
                    name,
                    existing_current_entry_id.or(first_inserted_id),
                    current_version + 1,
                    now_unix_timestamp(),
                ],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;

        Self::load_queue_with(&connection, renderer_location)?
            .ok_or_else(|| io::Error::other("queue disappeared after insert"))
    }

    pub(crate) fn move_queue_entry(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
        direction: i64,
    ) -> io::Result<PlaybackQueue> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();
        if current_entry_id == Some(queue_entry_id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot move the currently playing queue entry",
            ));
        }

        let current_position = transaction
            .query_row(
                "SELECT position FROM queue_entries
                 WHERE renderer_location = ? AND id = ?",
                params![renderer_location, queue_entry_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue entry not found"))?;

        let neighbor = if direction < 0 {
            transaction
                .query_row(
                    "SELECT id, position
                     FROM queue_entries
                     WHERE renderer_location = ?
                       AND position < ?
                     ORDER BY position DESC, id DESC
                     LIMIT 1",
                    params![renderer_location, current_position],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()
                .map_err(db_error)?
        } else {
            transaction
                .query_row(
                    "SELECT id, position
                     FROM queue_entries
                     WHERE renderer_location = ?
                       AND position > ?
                     ORDER BY position ASC, id ASC
                     LIMIT 1",
                    params![renderer_location, current_position],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()
                .map_err(db_error)?
        };

        let Some((neighbor_id, neighbor_position)) = neighbor else {
            transaction.commit().map_err(db_error)?;
            return self
                .load_queue(renderer_location)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue not found"));
        };

        transaction
            .execute(
                "UPDATE queue_entries SET position = ? WHERE id = ?",
                params![neighbor_position, queue_entry_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE queue_entries SET position = ? WHERE id = ?",
                params![current_position, neighbor_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE playback_queues
                 SET updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![now_unix_timestamp(), renderer_location],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;

        Self::load_queue_with(&connection, renderer_location)?
            .ok_or_else(|| io::Error::other("queue disappeared after move"))
    }

    pub(crate) fn remove_queue_entry(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<PlaybackQueue> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();
        if current_entry_id == Some(queue_entry_id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot remove the currently playing queue entry",
            ));
        }

        transaction
            .execute(
                "DELETE FROM queue_entries
                 WHERE renderer_location = ? AND id = ?",
                params![renderer_location, queue_entry_id],
            )
            .map_err(db_error)?;

        let ids = {
            let mut statement = transaction
                .prepare(
                    "SELECT id FROM queue_entries
                     WHERE renderer_location = ?
                     ORDER BY position ASC, id ASC",
                )
                .map_err(db_error)?;
            statement
                .query_map([renderer_location], |row| row.get::<_, i64>(0))
                .map_err(db_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_error)?
        };
        for (index, id) in ids.iter().enumerate() {
            transaction
                .execute(
                    "UPDATE queue_entries SET position = ? WHERE id = ?",
                    params![i64::try_from(index + 1).unwrap_or(i64::MAX), id],
                )
                .map_err(db_error)?;
        }
        transaction
            .execute(
                "UPDATE playback_queues
                 SET updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![now_unix_timestamp(), renderer_location],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;

        Self::load_queue_with(&connection, renderer_location)?
            .ok_or_else(|| io::Error::other("queue disappeared after remove"))
    }

    pub(crate) fn move_queue(
        &self,
        from_location: &str,
        to_location: &str,
        rename_to: Option<&str>,
    ) -> io::Result<()> {
        if from_location == to_location {
            return Ok(());
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let now = now_unix_timestamp();

        // Clear anything already at the destination so the rename UPDATEs don't collide
        // with a unique constraint on renderer_location.
        transaction
            .execute(
                "DELETE FROM queue_entries WHERE renderer_location = ?",
                [to_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM playback_sessions WHERE renderer_location = ?",
                [to_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM playback_queues WHERE renderer_location = ?",
                [to_location],
            )
            .map_err(db_error)?;

        // queue_entries.id is autoincrement; preserving it keeps current_entry_id /
        // session.queue_entry_id valid after the move. Only the owning location changes.
        transaction
            .execute(
                "UPDATE queue_entries SET renderer_location = ? WHERE renderer_location = ?",
                params![to_location, from_location],
            )
            .map_err(db_error)?;

        if let Some(name) = rename_to {
            transaction
                .execute(
                    "UPDATE playback_queues
                     SET renderer_location = ?, name = ?, updated_unix = ?, version = version + 1
                     WHERE renderer_location = ?",
                    params![to_location, name, now, from_location],
                )
                .map_err(db_error)?;
        } else {
            transaction
                .execute(
                    "UPDATE playback_queues
                     SET renderer_location = ?, updated_unix = ?, version = version + 1
                     WHERE renderer_location = ?",
                    params![to_location, now, from_location],
                )
                .map_err(db_error)?;
        }

        transaction
            .execute(
                "UPDATE playback_sessions
                 SET renderer_location = ?, last_observed_unix = ?
                 WHERE renderer_location = ?",
                params![to_location, now, from_location],
            )
            .map_err(db_error)?;

        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn clear_queue(&self, renderer_location: &str) -> io::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM queue_entries WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM playback_sessions WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn sync_queue_status(
        &self,
        renderer_location: &str,
        queue_status: &str,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE playback_queues
                 SET status = ?, updated_unix = ?
                 WHERE renderer_location = ?
                   AND status != ?",
                params![
                    queue_status,
                    now_unix_timestamp(),
                    renderer_location,
                    queue_status
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn set_queue_status(
        &self,
        renderer_location: &str,
        queue_status: &str,
        transport_state: &str,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        let now = now_unix_timestamp();
        connection
            .execute(
                "UPDATE playback_queues
                 SET status = ?, updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![queue_status, now, renderer_location],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (
                    ?,
                    (SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?),
                    (SELECT next_queue_entry_id FROM playback_sessions WHERE renderer_location = ?),
                    ?,
                    (SELECT current_track_uri FROM playback_sessions WHERE renderer_location = ?),
                    (SELECT position_seconds FROM playback_sessions WHERE renderer_location = ?),
                    (SELECT duration_seconds FROM playback_sessions WHERE renderer_location = ?),
                    ?,
                    NULL
                 )
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    transport_state = excluded.transport_state,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    transport_state,
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    now,
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn select_queue_entry(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        let now = now_unix_timestamp();
        connection
            .execute(
                "UPDATE playback_queues
                 SET current_entry_id = ?, status = 'ready', updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![queue_entry_id, now, renderer_location],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "UPDATE queue_entries
                 SET entry_status = CASE
                    WHEN id = ? THEN 'pending'
                    WHEN completed_unix IS NOT NULL THEN 'completed'
                    ELSE 'pending'
                 END
                 WHERE renderer_location = ?",
                params![queue_entry_id, renderer_location],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (?, ?, NULL, 'READY', NULL, NULL, NULL, ?, NULL)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    queue_entry_id = excluded.queue_entry_id,
                    next_queue_entry_id = excluded.next_queue_entry_id,
                    transport_state = excluded.transport_state,
                    current_track_uri = excluded.current_track_uri,
                    position_seconds = excluded.position_seconds,
                    duration_seconds = excluded.duration_seconds,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![renderer_location, queue_entry_id, now],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn advance_queue_after_completion(
        &self,
        renderer_location: &str,
    ) -> io::Result<Option<i64>> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();
        let Some(current_entry_id) = current_entry_id else {
            transaction.commit().map_err(db_error)?;
            return Ok(None);
        };

        let now = now_unix_timestamp();
        transaction
            .execute(
                "UPDATE queue_entries
                 SET entry_status = 'completed',
                     completed_unix = COALESCE(completed_unix, ?)
                 WHERE id = ?",
                params![now, current_entry_id],
            )
            .map_err(db_error)?;

        let next_entry_id = transaction
            .query_row(
                "SELECT id
                 FROM queue_entries
                 WHERE renderer_location = ?
                   AND position > (
                       SELECT position FROM queue_entries WHERE id = ?
                   )
                 ORDER BY position ASC, id ASC
                 LIMIT 1",
                params![renderer_location, current_entry_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?;

        if next_entry_id.is_none() {
            transaction
                .execute(
                    "DELETE FROM queue_entries WHERE renderer_location = ?",
                    [renderer_location],
                )
                .map_err(db_error)?;
            transaction
                .execute(
                    "DELETE FROM playback_queues WHERE renderer_location = ?",
                    [renderer_location],
                )
                .map_err(db_error)?;
            transaction
                .execute(
                    "DELETE FROM playback_sessions WHERE renderer_location = ?",
                    [renderer_location],
                )
                .map_err(db_error)?;
            transaction.commit().map_err(db_error)?;
            return Ok(None);
        }

        transaction
            .execute(
                "UPDATE playback_queues
                 SET current_entry_id = ?, status = 'ready', updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![next_entry_id, now, renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (?, ?, NULL, 'READY', NULL, NULL, NULL, ?, NULL)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    queue_entry_id = excluded.queue_entry_id,
                    next_queue_entry_id = NULL,
                    transport_state = 'READY',
                    current_track_uri = NULL,
                    position_seconds = NULL,
                    duration_seconds = NULL,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = NULL",
                params![renderer_location, next_entry_id, now],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(next_entry_id)
    }

    pub(crate) fn list_playing_queue_renderers(&self) -> io::Result<Vec<String>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT renderer_location
                 FROM playback_queues
                 WHERE status IN ('playing', 'paused')
                 ORDER BY updated_unix ASC, renderer_location ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        let mut locations = Vec::new();
        for row in rows {
            locations.push(row.map_err(db_error)?);
        }
        Ok(locations)
    }
}
