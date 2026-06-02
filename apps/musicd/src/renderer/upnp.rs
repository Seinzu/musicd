use std::io;

use musicd_upnp::{
    RendererPlaylist, StreamResource, TransportSnapshot, clear_next_av_transport_uri,
    clear_playlist_extension_queue, get_transport_snapshot, inspect_renderer, next, pause, play,
    previous, query_playlist_extension_queue, seek, set_av_transport_uri,
    set_next_av_transport_uri, stop,
};

use crate::ids::normalized_renderer_name;
use crate::types::RendererRecord;
use crate::util::now_unix_timestamp;

use super::{RendererBackend, renderer_needs_refresh};

#[derive(Debug, Default)]
pub(crate) struct UpnpRendererBackend;

impl RendererBackend for UpnpRendererBackend {
    fn resolve_renderer(
        &self,
        cached: Option<&RendererRecord>,
        renderer_location: &str,
    ) -> io::Result<RendererRecord> {
        if let Some(renderer) = cached.filter(|renderer| !renderer_needs_refresh(renderer)) {
            return Ok(renderer.clone());
        }

        let renderer = inspect_renderer(renderer_location)?;
        Ok(RendererRecord {
            location: renderer_location.to_string(),
            name: normalized_renderer_name(
                renderer_location,
                &renderer.friendly_name,
                renderer.model_name.as_deref(),
            ),
            manufacturer: renderer.manufacturer,
            model_name: renderer.model_name,
            av_transport_control_url: Some(renderer.av_transport_control_url),
            capabilities: renderer.capabilities,
            visibility: cached
                .map(|renderer| renderer.visibility.clone())
                .unwrap_or_else(|| "public".to_string()),
            owner_client_id: cached.and_then(|renderer| renderer.owner_client_id.clone()),
            last_checked_unix: now_unix_timestamp(),
            last_reachable_unix: Some(now_unix_timestamp()),
            last_error: None,
            last_seen_unix: now_unix_timestamp(),
        })
    }

    fn play_stream(&self, renderer: &RendererRecord, resource: &StreamResource) -> io::Result<()> {
        let control_url = upnp_control_url(renderer)?;
        set_av_transport_uri(control_url, resource)?;
        play(control_url)
    }

    fn preload_next(&self, renderer: &RendererRecord, resource: &StreamResource) -> io::Result<()> {
        let control_url = upnp_control_url(renderer)?;
        set_next_av_transport_uri(control_url, resource)
    }

    fn clear_next(&self, renderer: &RendererRecord) -> io::Result<()> {
        clear_next_av_transport_uri(upnp_control_url(renderer)?)
    }

    fn clear_private_queue(&self, renderer: &RendererRecord) -> io::Result<bool> {
        if renderer.capabilities.has_playlist_extension_service == Some(false) {
            return Ok(false);
        }
        clear_playlist_extension_queue(&renderer.location)
    }

    fn private_queue(&self, renderer: &RendererRecord) -> io::Result<Option<RendererPlaylist>> {
        if renderer.capabilities.has_playlist_extension_service != Some(true) {
            return Ok(None);
        }
        let renderer_description = inspect_renderer(&renderer.location)?;
        let Some(service) = renderer_description
            .services
            .iter()
            .find(|service| service.service_type == "urn:UuVol-com:service:PlaylistExtension:1")
        else {
            return Ok(None);
        };
        query_playlist_extension_queue(&service.control_url).map(Some)
    }

    fn play(&self, renderer: &RendererRecord) -> io::Result<()> {
        play(upnp_control_url(renderer)?)
    }

    fn pause(&self, renderer: &RendererRecord) -> io::Result<()> {
        pause(upnp_control_url(renderer)?)
    }

    fn stop(&self, renderer: &RendererRecord) -> io::Result<()> {
        stop(upnp_control_url(renderer)?)
    }

    fn next(&self, renderer: &RendererRecord) -> io::Result<()> {
        next(upnp_control_url(renderer)?)
    }

    fn previous(&self, renderer: &RendererRecord) -> io::Result<()> {
        previous(upnp_control_url(renderer)?)
    }

    fn seek(&self, renderer: &RendererRecord, position_seconds: u64) -> io::Result<()> {
        seek(upnp_control_url(renderer)?, position_seconds)
    }

    fn transport_snapshot(&self, renderer: &RendererRecord) -> io::Result<TransportSnapshot> {
        get_transport_snapshot(upnp_control_url(renderer)?)
    }
}

fn upnp_control_url(renderer: &RendererRecord) -> io::Result<&str> {
    renderer
        .av_transport_control_url
        .as_deref()
        .ok_or_else(|| io::Error::other("renderer is missing an AVTransport control URL"))
}
