use std::io;

use musicd_upnp::RendererCapabilities;
use rusqlite::{Connection, OptionalExtension, params};

use crate::renderer::{parse_renderer_actions_json, renderer_actions_json};
use crate::types::RendererRecord;

use super::{Database, db_error};

impl Database {
    pub(crate) fn list_renderers(&self) -> io::Result<Vec<RendererRecord>> {
        let connection = self.connection()?;
        let selected = Self::last_selected_renderer_location_with(&connection)?;
        let mut statement = connection
            .prepare(
                "SELECT location, name, manufacturer, model_name, av_transport_control_url,
                        av_transport_actions_json, has_playlist_extension_service,
                        visibility, owner_client_id,
                        last_checked_unix, last_reachable_unix, last_error, last_seen_unix
                 FROM renderers
                 ORDER BY last_seen_unix DESC, name ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(RendererRecord {
                    location: row.get(0)?,
                    name: row.get(1)?,
                    manufacturer: row.get(2)?,
                    model_name: row.get(3)?,
                    av_transport_control_url: row.get(4)?,
                    capabilities: RendererCapabilities {
                        av_transport_actions: parse_renderer_actions_json(row.get(5)?),
                        has_playlist_extension_service: row.get(6)?,
                    },
                    visibility: row.get(7)?,
                    owner_client_id: row.get(8)?,
                    last_checked_unix: row.get(9)?,
                    last_reachable_unix: row.get(10)?,
                    last_error: row.get(11)?,
                    last_seen_unix: row.get(12)?,
                })
            })
            .map_err(db_error)?;

        let mut renderers = Vec::new();
        for row in rows {
            renderers.push(row.map_err(db_error)?);
        }
        if let Some(selected) = selected {
            renderers.sort_by_key(|renderer| {
                (
                    renderer.location != selected,
                    -renderer.last_seen_unix,
                    renderer.name.clone(),
                )
            });
        }
        Ok(renderers)
    }

    pub(crate) fn load_renderer(&self, location: &str) -> io::Result<Option<RendererRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT location, name, manufacturer, model_name, av_transport_control_url,
                        av_transport_actions_json, has_playlist_extension_service,
                        visibility, owner_client_id,
                        last_checked_unix, last_reachable_unix, last_error, last_seen_unix
                 FROM renderers
                 WHERE location = ?",
                [location],
                |row| {
                    Ok(RendererRecord {
                        location: row.get(0)?,
                        name: row.get(1)?,
                        manufacturer: row.get(2)?,
                        model_name: row.get(3)?,
                        av_transport_control_url: row.get(4)?,
                        capabilities: RendererCapabilities {
                            av_transport_actions: parse_renderer_actions_json(row.get(5)?),
                            has_playlist_extension_service: row.get(6)?,
                        },
                        visibility: row.get(7)?,
                        owner_client_id: row.get(8)?,
                        last_checked_unix: row.get(9)?,
                        last_reachable_unix: row.get(10)?,
                        last_error: row.get(11)?,
                        last_seen_unix: row.get(12)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    pub(crate) fn upsert_renderer(&self, renderer: &RendererRecord) -> io::Result<()> {
        let connection = self.connection()?;
        let av_transport_actions_json =
            renderer_actions_json(&renderer.capabilities.av_transport_actions);
        connection
            .execute(
                "INSERT INTO renderers
                 (location, name, manufacturer, model_name, av_transport_control_url,
                  av_transport_actions_json, has_playlist_extension_service, visibility,
                  owner_client_id, last_checked_unix, last_reachable_unix, last_error,
                  last_seen_unix)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(location) DO UPDATE SET
                    name = excluded.name,
                    manufacturer = COALESCE(excluded.manufacturer, renderers.manufacturer),
                    model_name = COALESCE(excluded.model_name, renderers.model_name),
                    av_transport_control_url = COALESCE(excluded.av_transport_control_url, renderers.av_transport_control_url),
                    av_transport_actions_json = COALESCE(excluded.av_transport_actions_json, renderers.av_transport_actions_json),
                    has_playlist_extension_service = COALESCE(excluded.has_playlist_extension_service, renderers.has_playlist_extension_service),
                    visibility = excluded.visibility,
                    owner_client_id = excluded.owner_client_id,
                    last_checked_unix = excluded.last_checked_unix,
                    last_reachable_unix = COALESCE(excluded.last_reachable_unix, renderers.last_reachable_unix),
                    last_error = excluded.last_error,
                    last_seen_unix = excluded.last_seen_unix",
                params![
                    renderer.location,
                    renderer.name,
                    renderer.manufacturer,
                    renderer.model_name,
                    renderer.av_transport_control_url,
                    av_transport_actions_json,
                    renderer.capabilities.has_playlist_extension_service,
                    renderer.visibility,
                    renderer.owner_client_id,
                    renderer.last_checked_unix,
                    renderer.last_reachable_unix,
                    renderer.last_error,
                    renderer.last_seen_unix
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn set_last_selected_renderer_location(&self, location: &str) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO app_state (key, value) VALUES ('last_renderer_location', ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [location],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn last_selected_renderer_location(&self) -> io::Result<Option<String>> {
        let connection = self.connection()?;
        Self::last_selected_renderer_location_with(&connection)
    }

    pub(super) fn last_selected_renderer_location_with(
        connection: &Connection,
    ) -> io::Result<Option<String>> {
        connection
            .query_row(
                "SELECT value FROM app_state WHERE key = 'last_renderer_location'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)
    }
}
