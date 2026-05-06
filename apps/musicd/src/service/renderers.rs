use std::io;

use musicd_upnp::{RendererCapabilities, inspect_renderer};

use crate::ids::{
    normalized_renderer_name, renderer_location_host, renderer_name_looks_like_location,
};
use crate::renderer::{
    RendererKind, android_local_renderer_capabilities, renderer_group_id_from_location,
    renderer_is_viable, renderer_kind_for_location, renderer_needs_refresh,
};
use crate::types::RendererRecord;
use crate::util::now_unix_timestamp;

use super::ServiceState;

impl ServiceState {
    pub(crate) fn renderer_snapshot(&self) -> Vec<RendererRecord> {
        self.database.list_renderers().unwrap_or_default()
    }

    pub(crate) fn enriched_renderer_snapshot(&self) -> Vec<RendererRecord> {
        self.renderer_snapshot()
            .into_iter()
            .filter_map(|renderer| {
                let renderer = self
                    .enrich_renderer_record_if_needed(&renderer)
                    .unwrap_or(renderer);
                renderer_is_viable(&renderer).then_some(renderer)
            })
            .collect()
    }

    pub(crate) fn enriched_renderer_record(
        &self,
        renderer_location: &str,
    ) -> Option<RendererRecord> {
        if renderer_group_id_from_location(renderer_location).is_some() {
            return None;
        }
        self.database
            .load_renderer(renderer_location)
            .ok()
            .flatten()
            .and_then(|renderer| {
                let renderer = self
                    .enrich_renderer_record_if_needed(&renderer)
                    .unwrap_or(renderer);
                renderer_is_viable(&renderer).then_some(renderer)
            })
    }

    pub(crate) fn enrich_renderer_record_if_needed(
        &self,
        renderer: &RendererRecord,
    ) -> io::Result<RendererRecord> {
        if !renderer_needs_refresh(renderer) {
            return Ok(renderer.clone());
        }
        let resolved = self.resolve_renderer(&renderer.location)?;
        Ok(resolved)
    }

    pub(crate) fn preferred_renderer_location(&self, requested: Option<&str>) -> String {
        requested
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                self.database
                    .last_selected_renderer_location()
                    .ok()
                    .flatten()
            })
            .or_else(|| self.config.default_renderer_location.clone())
            .unwrap_or_default()
    }

    pub(crate) fn remember_renderer_location(&self, location: &str) -> io::Result<()> {
        if renderer_group_id_from_location(location).is_some() {
            if self.database.renderer_group_queue_exists(location)? {
                return self.database.set_last_selected_renderer_location(location);
            }
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "renderer group does not exist",
            ));
        }

        if let Some(existing) = self.database.load_renderer(location)? {
            if !renderer_name_looks_like_location(&existing.name, location) {
                self.database
                    .set_last_selected_renderer_location(location)?;
                return Ok(());
            }
        }

        if matches!(renderer_kind_for_location(location), RendererKind::Upnp) {
            if let Ok(details) = inspect_renderer(location) {
                return self.remember_renderer_details(
                    location,
                    &details.friendly_name,
                    details.manufacturer.as_deref(),
                    details.model_name.as_deref(),
                    Some(&details.av_transport_control_url),
                    Some(&details.capabilities),
                    None,
                );
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "location did not resolve to a playable UPnP media renderer",
            ));
        }

        if matches!(
            renderer_kind_for_location(location),
            RendererKind::AndroidLocal
        ) {
            let renderer = RendererRecord {
                location: location.to_string(),
                name: "This phone".to_string(),
                manufacturer: Some("Android".to_string()),
                model_name: None,
                av_transport_control_url: None,
                capabilities: android_local_renderer_capabilities(),
                visibility: "public".to_string(),
                owner_client_id: None,
                last_checked_unix: now_unix_timestamp(),
                last_reachable_unix: Some(now_unix_timestamp()),
                last_error: None,
                last_seen_unix: now_unix_timestamp(),
            };
            self.database.upsert_renderer(&renderer)?;
            self.database
                .set_last_selected_renderer_location(location)?;
            return Ok(());
        }

        let renderer = RendererRecord {
            location: location.to_string(),
            name: renderer_location_host(location)
                .unwrap_or(location)
                .to_string(),
            manufacturer: None,
            model_name: None,
            av_transport_control_url: None,
            capabilities: RendererCapabilities::default(),
            visibility: "public".to_string(),
            owner_client_id: None,
            last_checked_unix: 0,
            last_reachable_unix: None,
            last_error: None,
            last_seen_unix: 0,
        };
        self.database.upsert_renderer(&renderer)?;
        self.database.set_last_selected_renderer_location(location)
    }

    pub(crate) fn remember_renderer_details(
        &self,
        location: &str,
        name: &str,
        manufacturer: Option<&str>,
        model_name: Option<&str>,
        av_transport_control_url: Option<&str>,
        capabilities: Option<&RendererCapabilities>,
        last_error: Option<&str>,
    ) -> io::Result<()> {
        self.remember_renderer_details_with_visibility(
            location,
            name,
            manufacturer,
            model_name,
            av_transport_control_url,
            capabilities,
            last_error,
            "public",
            None,
        )
    }

    pub(crate) fn remember_renderer_details_with_visibility(
        &self,
        location: &str,
        name: &str,
        manufacturer: Option<&str>,
        model_name: Option<&str>,
        av_transport_control_url: Option<&str>,
        capabilities: Option<&RendererCapabilities>,
        last_error: Option<&str>,
        visibility: &str,
        owner_client_id: Option<&str>,
    ) -> io::Result<()> {
        let existing = self.database.load_renderer(location)?;
        let now = now_unix_timestamp();
        let last_reachable_unix = if last_error.is_none() {
            Some(now)
        } else {
            existing
                .as_ref()
                .and_then(|renderer| renderer.last_reachable_unix)
        };
        let last_seen_unix = last_reachable_unix.unwrap_or(0);
        let renderer = RendererRecord {
            location: location.to_string(),
            name: normalized_renderer_name(location, name, model_name),
            manufacturer: manufacturer.map(ToString::to_string),
            model_name: model_name.map(ToString::to_string),
            av_transport_control_url: av_transport_control_url.map(ToString::to_string),
            capabilities: capabilities
                .cloned()
                .or_else(|| {
                    existing
                        .as_ref()
                        .map(|renderer| renderer.capabilities.clone())
                })
                .unwrap_or_default(),
            visibility: normalized_renderer_visibility(visibility).to_string(),
            owner_client_id: owner_client_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            last_checked_unix: now,
            last_reachable_unix,
            last_error: last_error.map(ToString::to_string),
            last_seen_unix,
        };
        self.database.upsert_renderer(&renderer)?;
        self.database.set_last_selected_renderer_location(location)
    }

    pub(crate) fn remember_renderer_record(&self, renderer: &RendererRecord) -> io::Result<()> {
        self.database.upsert_renderer(renderer)?;
        self.database
            .set_last_selected_renderer_location(&renderer.location)
    }

    pub(crate) fn mark_renderer_reachable(&self, renderer: &RendererRecord) -> io::Result<()> {
        let mut updated = renderer.clone();
        let now = now_unix_timestamp();
        updated.last_checked_unix = now;
        updated.last_reachable_unix = Some(now);
        updated.last_seen_unix = now;
        updated.last_error = None;
        self.database.upsert_renderer(&updated)
    }

    pub(crate) fn mark_renderer_unreachable(
        &self,
        renderer_location: &str,
        error: &io::Error,
    ) -> io::Result<()> {
        let mut renderer =
            self.database
                .load_renderer(renderer_location)?
                .unwrap_or(RendererRecord {
                    location: renderer_location.to_string(),
                    name: renderer_location_host(renderer_location)
                        .unwrap_or(renderer_location)
                        .to_string(),
                    manufacturer: None,
                    model_name: None,
                    av_transport_control_url: None,
                    capabilities: RendererCapabilities::default(),
                    visibility: "public".to_string(),
                    owner_client_id: None,
                    last_checked_unix: 0,
                    last_reachable_unix: None,
                    last_error: None,
                    last_seen_unix: 0,
                });
        renderer.last_checked_unix = now_unix_timestamp();
        renderer.last_error = Some(error.to_string());
        self.database.upsert_renderer(&renderer)
    }

    pub(crate) fn renderer_is_visible_to_client(
        &self,
        renderer: &RendererRecord,
        client_id: Option<&str>,
    ) -> bool {
        renderer.visibility != "private"
            || renderer
                .owner_client_id
                .as_deref()
                .zip(client_id.map(str::trim).filter(|value| !value.is_empty()))
                .is_some_and(|(owner, client)| owner == client)
    }

    pub(crate) fn check_direct_renderer_access(
        &self,
        renderer_location: &str,
        client_id: Option<&str>,
    ) -> io::Result<()> {
        if renderer_group_id_from_location(renderer_location).is_some() {
            return Ok(());
        }
        let Some(renderer) = self.database.load_renderer(renderer_location)? else {
            return Ok(());
        };
        if self.renderer_is_visible_to_client(&renderer, client_id) {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private renderer belongs to another client",
        ))
    }

    pub(crate) fn check_private_renderer_additions_owned(
        &self,
        members: &[String],
        existing_members: &[String],
        client_id: Option<&str>,
    ) -> io::Result<()> {
        for member in members {
            let member = member.trim();
            if member.is_empty() || existing_members.iter().any(|existing| existing == member) {
                continue;
            }
            let Some(renderer) = self.database.load_renderer(member)? else {
                continue;
            };
            if renderer.visibility == "private"
                && !self.renderer_is_visible_to_client(&renderer, client_id)
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "private renderer can only be added to a group by its owning client",
                ));
            }
        }
        Ok(())
    }
}

fn normalized_renderer_visibility(visibility: &str) -> &'static str {
    if visibility.trim().eq_ignore_ascii_case("private") {
        "private"
    } else {
        "public"
    }
}
