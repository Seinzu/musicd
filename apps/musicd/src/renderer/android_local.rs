use std::io;

use musicd_upnp::{RendererCapabilities, StreamResource, TransportSnapshot};

use crate::types::RendererRecord;
use crate::util::now_unix_timestamp;

use super::RendererBackend;

#[derive(Debug, Default)]
pub(crate) struct AndroidLocalRendererBackend;

impl RendererBackend for AndroidLocalRendererBackend {
    fn resolve_renderer(
        &self,
        cached: Option<&RendererRecord>,
        renderer_location: &str,
    ) -> io::Result<RendererRecord> {
        if let Some(renderer) = cached {
            return Ok(renderer.clone());
        }

        Ok(RendererRecord {
            location: renderer_location.to_string(),
            name: "This phone".to_string(),
            manufacturer: Some("Android".to_string()),
            model_name: None,
            av_transport_control_url: None,
            capabilities: android_local_renderer_capabilities(),
            last_checked_unix: now_unix_timestamp(),
            last_reachable_unix: Some(now_unix_timestamp()),
            last_error: None,
            last_seen_unix: now_unix_timestamp(),
        })
    }

    fn play_stream(&self, _renderer: &RendererRecord, _resource: &StreamResource) -> io::Result<()> {
        Ok(())
    }

    fn preload_next(&self, _renderer: &RendererRecord, _resource: &StreamResource) -> io::Result<()> {
        Ok(())
    }

    fn play(&self, _renderer: &RendererRecord) -> io::Result<()> {
        Ok(())
    }

    fn pause(&self, _renderer: &RendererRecord) -> io::Result<()> {
        Ok(())
    }

    fn stop(&self, _renderer: &RendererRecord) -> io::Result<()> {
        Ok(())
    }

    fn next(&self, _renderer: &RendererRecord) -> io::Result<()> {
        Ok(())
    }

    fn previous(&self, _renderer: &RendererRecord) -> io::Result<()> {
        Ok(())
    }

    fn transport_snapshot(&self, _renderer: &RendererRecord) -> io::Result<TransportSnapshot> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "android_local renderers report transport state explicitly",
        ))
    }
}

pub(crate) fn android_local_renderer_capabilities() -> RendererCapabilities {
    RendererCapabilities {
        av_transport_actions: Some(vec![
            "Play".to_string(),
            "Pause".to_string(),
            "Stop".to_string(),
            "Next".to_string(),
            "Previous".to_string(),
            "Seek".to_string(),
        ]),
        has_playlist_extension_service: Some(false),
    }
}
