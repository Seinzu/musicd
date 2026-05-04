use std::io;

use musicd_upnp::{StreamResource, TransportSnapshot};

use crate::types::RendererRecord;

mod android_local;
mod health;
mod upnp;

pub(crate) use android_local::{AndroidLocalRendererBackend, android_local_renderer_capabilities};
pub(crate) use health::{
    parse_renderer_actions_json, renderer_actions_json, renderer_is_viable, renderer_needs_refresh,
};
pub(crate) use upnp::UpnpRendererBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RendererKind {
    Upnp,
    Sonos,
    AndroidLocal,
}

#[derive(Debug, Default)]
pub(crate) struct RendererBackends {
    pub(crate) upnp: UpnpRendererBackend,
    pub(crate) android_local: AndroidLocalRendererBackend,
}

pub(crate) trait RendererBackend: Send + Sync {
    fn resolve_renderer(
        &self,
        cached: Option<&RendererRecord>,
        renderer_location: &str,
    ) -> io::Result<RendererRecord>;

    fn play_stream(&self, renderer: &RendererRecord, resource: &StreamResource) -> io::Result<()>;

    fn preload_next(&self, renderer: &RendererRecord, resource: &StreamResource) -> io::Result<()>;

    fn play(&self, renderer: &RendererRecord) -> io::Result<()>;

    fn pause(&self, renderer: &RendererRecord) -> io::Result<()>;

    fn stop(&self, renderer: &RendererRecord) -> io::Result<()>;

    fn next(&self, renderer: &RendererRecord) -> io::Result<()>;

    fn previous(&self, renderer: &RendererRecord) -> io::Result<()>;

    fn transport_snapshot(&self, renderer: &RendererRecord) -> io::Result<TransportSnapshot>;
}

impl RendererBackends {
    pub(crate) fn backend_for_location(
        &self,
        renderer_location: &str,
    ) -> io::Result<&dyn RendererBackend> {
        match renderer_kind_for_location(renderer_location) {
            RendererKind::Upnp => Ok(&self.upnp),
            RendererKind::Sonos => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Sonos renderer support has not been implemented yet",
            )),
            RendererKind::AndroidLocal => Ok(&self.android_local),
        }
    }
}

pub(crate) fn renderer_kind_for_location(renderer_location: &str) -> RendererKind {
    if renderer_location.starts_with("android-local://") {
        RendererKind::AndroidLocal
    } else if renderer_location.starts_with("sonos:") {
        RendererKind::Sonos
    } else {
        RendererKind::Upnp
    }
}

pub(crate) fn renderer_kind_name(kind: RendererKind) -> &'static str {
    match kind {
        RendererKind::Upnp => "upnp",
        RendererKind::Sonos => "sonos",
        RendererKind::AndroidLocal => "android_local",
    }
}
