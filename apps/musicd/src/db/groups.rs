use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use crate::ids::stable_track_id;
use crate::types::{RendererGroup, RendererGroupMember};
use crate::util::now_unix_timestamp;

use super::{Database, db_error};

impl Database {
    pub(crate) fn create_renderer_group(
        &self,
        name: &str,
        members: &[String],
    ) -> io::Result<RendererGroup> {
        let normalized_members = normalized_members(members);
        if normalized_members.len() < 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "renderer groups require at least two members",
            ));
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let now = now_unix_timestamp();
        let id = new_group_id(name, &normalized_members);
        let display_name = normalized_group_name(name, &normalized_members);
        transaction
            .execute(
                "INSERT INTO renderer_groups (id, name, created_unix, updated_unix)
                 VALUES (?, ?, ?, ?)",
                params![id, display_name, now, now],
            )
            .map_err(db_error)?;

        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO renderer_group_members
                     (group_id, renderer_location, position, joined_unix)
                     VALUES (?, ?, ?, ?)",
                )
                .map_err(db_error)?;
            for (index, renderer_location) in normalized_members.iter().enumerate() {
                statement
                    .execute(params![
                        id,
                        renderer_location,
                        i64::try_from(index + 1).unwrap_or(i64::MAX),
                        now
                    ])
                    .map_err(db_error)?;
            }
        }

        transaction.commit().map_err(db_error)?;
        self.load_renderer_group(&id)?
            .ok_or_else(|| io::Error::other("renderer group disappeared after create"))
    }

    pub(crate) fn list_renderer_groups(&self) -> io::Result<Vec<RendererGroup>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, created_unix, updated_unix
                 FROM renderer_groups
                 ORDER BY updated_unix DESC, name ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(RendererGroup {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_unix: row.get(2)?,
                    updated_unix: row.get(3)?,
                    members: Vec::new(),
                })
            })
            .map_err(db_error)?;

        let mut groups = Vec::new();
        for row in rows {
            let mut group = row.map_err(db_error)?;
            group.members = Self::load_renderer_group_members_with(&connection, &group.id)?;
            groups.push(group);
        }
        Ok(groups)
    }

    pub(crate) fn load_renderer_group(&self, id: &str) -> io::Result<Option<RendererGroup>> {
        let connection = self.connection()?;
        let Some(mut group) = connection
            .query_row(
                "SELECT id, name, created_unix, updated_unix
                 FROM renderer_groups
                 WHERE id = ?",
                [id],
                |row| {
                    Ok(RendererGroup {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        created_unix: row.get(2)?,
                        updated_unix: row.get(3)?,
                        members: Vec::new(),
                    })
                },
            )
            .optional()
            .map_err(db_error)?
        else {
            return Ok(None);
        };

        group.members = Self::load_renderer_group_members_with(&connection, &group.id)?;
        Ok(Some(group))
    }

    pub(crate) fn load_renderer_group_by_queue_key(
        &self,
        renderer_location: &str,
    ) -> io::Result<Option<RendererGroup>> {
        let Some(group_id) = renderer_location.strip_prefix("group:") else {
            return Ok(None);
        };
        self.load_renderer_group(group_id)
    }

    pub(crate) fn renderer_group_queue_exists(&self, renderer_location: &str) -> io::Result<bool> {
        Ok(self
            .load_renderer_group_by_queue_key(renderer_location)?
            .is_some())
    }

    fn load_renderer_group_members_with(
        connection: &Connection,
        group_id: &str,
    ) -> io::Result<Vec<RendererGroupMember>> {
        let mut statement = connection
            .prepare(
                "SELECT renderer_location, position, joined_unix
                 FROM renderer_group_members
                 WHERE group_id = ?
                 ORDER BY position ASC, renderer_location ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([group_id], |row| {
                Ok(RendererGroupMember {
                    renderer_location: row.get(0)?,
                    position: row.get(1)?,
                    joined_unix: row.get(2)?,
                })
            })
            .map_err(db_error)?;

        let mut members = Vec::new();
        for row in rows {
            members.push(row.map_err(db_error)?);
        }
        Ok(members)
    }
}

fn normalized_members(members: &[String]) -> Vec<String> {
    let mut normalized = Vec::new();
    for member in members {
        let member = member.trim();
        if member.is_empty() || normalized.iter().any(|existing| existing == member) {
            continue;
        }
        normalized.push(member.to_string());
    }
    normalized
}

fn normalized_group_name(name: &str, members: &[String]) -> String {
    let name = name.trim();
    if !name.is_empty() {
        return name.to_string();
    }
    format!("Group of {} renderers", members.len())
}

fn new_group_id(name: &str, members: &[String]) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    stable_track_id(&format!("renderer-group:{name}:{members:?}:{nanos}"))
}
