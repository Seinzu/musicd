use std::io;

use musicd_upnp::{
    StreamResource, TransportSnapshot, get_transport_snapshot, inspect_renderer, next, pause, play,
    previous, set_av_transport_uri, set_next_av_transport_uri, stop,
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
