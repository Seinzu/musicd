use std::io;
#[cfg(test)]
use std::sync::Arc;

use musicd_upnp::{RendererPlaylist, StreamResource, TransportSnapshot};

use crate::types::RendererRecord;

mod android_local;
mod health;
mod upnp;

pub(crate) use android_local::{
    AndroidLocalRendererBackend, android_local_renderer_capabilities, local_renderer_capabilities,
};
pub(crate) use health::{
    parse_renderer_actions_json, renderer_actions_json, renderer_is_viable, renderer_needs_refresh,
};
pub(crate) use upnp::UpnpRendererBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RendererKind {
    Upnp,
    Sonos,
    AndroidLocal,
    CliLocal,
    Group,
}

pub(crate) struct RendererBackends {
    pub(crate) upnp: UpnpRendererBackend,
    pub(crate) android_local: AndroidLocalRendererBackend,
    #[cfg(test)]
    pub(crate) test: Option<Arc<dyn RendererBackend>>,
}

impl std::fmt::Debug for RendererBackends {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RendererBackends")
            .field("upnp", &self.upnp)
            .field("android_local", &self.android_local)
            .finish_non_exhaustive()
    }
}

impl Default for RendererBackends {
    fn default() -> Self {
        Self {
            upnp: UpnpRendererBackend,
            android_local: AndroidLocalRendererBackend,
            #[cfg(test)]
            test: None,
        }
    }
}

pub(crate) trait RendererBackend: Send + Sync {
    fn resolve_renderer(
        &self,
        cached: Option<&RendererRecord>,
        renderer_location: &str,
    ) -> io::Result<RendererRecord>;

    fn play_stream(&self, renderer: &RendererRecord, resource: &StreamResource) -> io::Result<()>;

    fn preload_next(&self, renderer: &RendererRecord, resource: &StreamResource) -> io::Result<()>;

    fn clear_next(&self, renderer: &RendererRecord) -> io::Result<()>;

    fn clear_private_queue(&self, _renderer: &RendererRecord) -> io::Result<bool> {
        Ok(false)
    }

    fn private_queue(&self, _renderer: &RendererRecord) -> io::Result<Option<RendererPlaylist>> {
        Ok(None)
    }

    fn sync_private_queue_after_current(
        &self,
        _renderer: &RendererRecord,
        _current: &StreamResource,
        _successors: &[StreamResource],
    ) -> io::Result<bool> {
        Ok(false)
    }

    fn play(&self, renderer: &RendererRecord) -> io::Result<()>;

    fn pause(&self, renderer: &RendererRecord) -> io::Result<()>;

    fn stop(&self, renderer: &RendererRecord) -> io::Result<()>;

    fn next(&self, renderer: &RendererRecord) -> io::Result<()>;

    fn previous(&self, renderer: &RendererRecord) -> io::Result<()>;

    fn seek(&self, renderer: &RendererRecord, position_seconds: u64) -> io::Result<()>;

    fn transport_snapshot(&self, renderer: &RendererRecord) -> io::Result<TransportSnapshot>;
}

impl RendererBackends {
    pub(crate) fn backend_for_location(
        &self,
        renderer_location: &str,
    ) -> io::Result<&dyn RendererBackend> {
        #[cfg(test)]
        if let Some(test) = &self.test {
            return Ok(test.as_ref());
        }

        match renderer_kind_for_location(renderer_location) {
            RendererKind::Upnp => Ok(&self.upnp),
            RendererKind::Sonos => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Sonos renderer support has not been implemented yet",
            )),
            RendererKind::AndroidLocal | RendererKind::CliLocal => Ok(&self.android_local),
            RendererKind::Group => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "group playback fan-out has not been implemented yet",
            )),
        }
    }
}

pub(crate) fn renderer_group_queue_key(group_id: &str) -> String {
    format!("group:{}", group_id.trim())
}

pub(crate) fn renderer_group_id_from_location(renderer_location: &str) -> Option<&str> {
    renderer_location
        .strip_prefix("group:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn renderer_kind_for_location(renderer_location: &str) -> RendererKind {
    if renderer_location.starts_with("android-local://") {
        RendererKind::AndroidLocal
    } else if renderer_location.starts_with("cli-local://") {
        RendererKind::CliLocal
    } else if renderer_location.starts_with("sonos:") {
        RendererKind::Sonos
    } else if renderer_group_id_from_location(renderer_location).is_some() {
        RendererKind::Group
    } else {
        RendererKind::Upnp
    }
}

pub(crate) fn renderer_kind_name(kind: RendererKind) -> &'static str {
    match kind {
        RendererKind::Upnp => "upnp",
        RendererKind::Sonos => "sonos",
        RendererKind::AndroidLocal => "android_local",
        RendererKind::CliLocal => "cli_local",
        RendererKind::Group => "group",
    }
}
