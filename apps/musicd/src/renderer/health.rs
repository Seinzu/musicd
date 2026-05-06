use crate::ids::renderer_name_looks_like_location;
use crate::types::RendererRecord;

use super::{RendererKind, renderer_kind_for_location};

pub(crate) fn renderer_needs_refresh(renderer: &RendererRecord) -> bool {
    matches!(
        renderer_kind_for_location(&renderer.location),
        RendererKind::Upnp
    ) && (renderer.av_transport_control_url.is_none()
        || renderer.capabilities.av_transport_actions.is_none()
        || renderer
            .capabilities
            .has_playlist_extension_service
            .is_none()
        || renderer_name_looks_like_location(&renderer.name, &renderer.location))
}

pub(crate) fn renderer_is_viable(renderer: &RendererRecord) -> bool {
    match renderer_kind_for_location(&renderer.location) {
        RendererKind::Upnp => renderer.av_transport_control_url.is_some(),
        RendererKind::Sonos => true,
        RendererKind::AndroidLocal | RendererKind::CliLocal => true,
        RendererKind::Group => true,
    }
}

pub(crate) fn renderer_actions_json(actions: &Option<Vec<String>>) -> Option<String> {
    actions
        .as_ref()
        .and_then(|actions| serde_json::to_string(actions).ok())
}

pub(crate) fn parse_renderer_actions_json(value: Option<String>) -> Option<Vec<String>> {
    value.and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
}
