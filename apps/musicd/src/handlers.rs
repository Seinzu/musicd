use std::io::{self, Write};
use std::sync::Mutex;
use std::time::Duration;

use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};

use crate::http::{
    HttpRequest, ResponseWriter, api_error, redirect_album, redirect_home, redirect_to_path,
    request_value, respond_json, respond_not_found, respond_text, respond_with_file,
    write_response_owned, write_sse_comment, write_sse_event,
};
use crate::library::ScanProgressEvent;
use crate::metrics;
use crate::renderer::{
    RendererKind, android_local_renderer_capabilities, local_renderer_capabilities,
    renderer_kind_for_location,
};
use crate::service::tidal::{TidalQueuedAlbum, TidalQueuedTrack, TidalStreamSource};
use crate::service::{ServiceState, queue_status_for_transport};
use crate::types::{AlbumSummary, LibraryTrack, PlaybackQueue, RecommendationImportRequest};
use crate::util::json_escape;
use crate::views::json::{
    album_summary_json, render_discovery_json, render_playback_event_json_for_renderer,
    render_queue_json_for_renderer, renderer_group_json, session_payload_json_for_renderer,
};

pub(crate) fn handle_play_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let Some(track_id) = request.query.get("track_id") else {
        return redirect_home(
            writer,
            Some(&renderer_location),
            None,
            Some("Select a track before pressing play."),
        );
    };

    if renderer_location.is_empty() {
        return redirect_home(
            writer,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before pressing play."),
        );
    }

    let _ = state.remember_renderer_location(&renderer_location);

    let Some(track) = state.find_track(track_id) else {
        return redirect_home(
            writer,
            Some(&renderer_location),
            None,
            Some("The selected track is no longer in the scanned library."),
        );
    };

    match state
        .replace_queue_with_track(&renderer_location, &track)
        .and_then(|_| state.start_current_queue_entry(&renderer_location))
    {
        Ok((started_track, _queue_entry_id, renderer_name, _renderer_location)) => redirect_home(
            writer,
            Some(&renderer_location),
            Some(&format!(
                "Now playing '{}' on {}. The queue now contains 1 item.",
                started_track.title, renderer_name
            )),
            None,
        ),
        Err(error) => redirect_home(
            writer,
            Some(&renderer_location),
            None,
            Some(&format!("Playback failed: {error}")),
        ),
    }
}

pub(crate) fn handle_play_album_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let Some(album_id) = request.query.get("album_id").map(String::as_str) else {
        return redirect_home(
            writer,
            Some(&renderer_location),
            None,
            Some("Select an album before pressing play."),
        );
    };

    let Some(album) = state.find_album(album_id) else {
        return redirect_home(
            writer,
            Some(&renderer_location),
            None,
            Some("The selected album is no longer in the scanned library."),
        );
    };

    if renderer_location.is_empty() {
        return redirect_album(
            writer,
            &album.id,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before pressing play."),
        );
    }

    let _ = state.remember_renderer_location(&renderer_location);

    let Some(_track) = state.first_track_for_album(&album.id) else {
        return redirect_album(
            writer,
            &album.id,
            Some(&renderer_location),
            None,
            Some("This album does not have any playable tracks."),
        );
    };

    match state
        .replace_queue_with_album(&renderer_location, &album)
        .and_then(|_| state.start_current_queue_entry(&renderer_location))
    {
        Ok((started_track, _queue_entry_id, renderer_name, _renderer_location)) => redirect_album(
            writer,
            &album.id,
            Some(&renderer_location),
            Some(&format!(
                "Started album '{}' from track '{}' on {}. The queue now contains the album and will advance automatically.",
                album.title, started_track.title, renderer_name
            )),
            None,
        ),
        Err(error) => redirect_album(
            writer,
            &album.id,
            Some(&renderer_location),
            None,
            Some(&format!("Playback failed: {error}")),
        ),
    }
}

pub(crate) fn handle_queue_append_track_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let return_to = request
        .query
        .get("return_to")
        .map(String::as_str)
        .unwrap_or("/");
    let Some(track_id) = request.query.get("track_id").map(String::as_str) else {
        return redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some("Select a track before adding it to the queue."),
        );
    };

    if renderer_location.is_empty() {
        return redirect_to_path(
            writer,
            return_to,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before queuing music."),
        );
    }

    let Some(track) = state.find_track(track_id) else {
        return redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some("The selected track is no longer in the scanned library."),
        );
    };

    match state.append_track_to_queue(&renderer_location, &track) {
        Ok(queue) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            Some(&format!(
                "Queued '{}' for {}. Queue length: {}.",
                track.title,
                renderer_location,
                queue.entries.len()
            )),
            None,
        ),
        Err(error) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some(&format!("Queue update failed: {error}")),
        ),
    }
}

pub(crate) fn handle_queue_play_next_track_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let return_to = request
        .query
        .get("return_to")
        .map(String::as_str)
        .unwrap_or("/");
    let Some(track_id) = request.query.get("track_id").map(String::as_str) else {
        return redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some("Select a track before adding it to play next."),
        );
    };

    if renderer_location.is_empty() {
        return redirect_to_path(
            writer,
            return_to,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before queuing music."),
        );
    }

    let Some(track) = state.find_track(track_id) else {
        return redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some("The selected track is no longer in the scanned library."),
        );
    };

    match state.play_next_track(&renderer_location, &track) {
        Ok(queue) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            Some(&format!(
                "'{}' will play next. Queue length: {}.",
                track.title,
                queue.entries.len()
            )),
            None,
        ),
        Err(error) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some(&format!("Queue update failed: {error}")),
        ),
    }
}

pub(crate) fn handle_transport_play_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let return_to = request
        .query
        .get("return_to")
        .map(String::as_str)
        .unwrap_or("/");

    if renderer_location.is_empty() {
        return redirect_to_path(
            writer,
            return_to,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before pressing play."),
        );
    }

    match state.resume_renderer(&renderer_location) {
        Ok(message) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            Some(&message),
            None,
        ),
        Err(error) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some(&format!("Play failed: {error}")),
        ),
    }
}

pub(crate) fn handle_transport_pause_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let return_to = request
        .query
        .get("return_to")
        .map(String::as_str)
        .unwrap_or("/");

    if renderer_location.is_empty() {
        return redirect_to_path(
            writer,
            return_to,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before pausing playback."),
        );
    }

    match state.pause_renderer(&renderer_location) {
        Ok(message) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            Some(&message),
            None,
        ),
        Err(error) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some(&format!("Pause failed: {error}")),
        ),
    }
}

pub(crate) fn handle_transport_stop_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let return_to = request
        .query
        .get("return_to")
        .map(String::as_str)
        .unwrap_or("/");

    if renderer_location.is_empty() {
        return redirect_to_path(
            writer,
            return_to,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before stopping playback."),
        );
    }

    match state.stop_renderer(&renderer_location) {
        Ok(message) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            Some(&message),
            None,
        ),
        Err(error) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some(&format!("Stop failed: {error}")),
        ),
    }
}

pub(crate) fn handle_transport_next_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let return_to = request
        .query
        .get("return_to")
        .map(String::as_str)
        .unwrap_or("/");

    if renderer_location.is_empty() {
        return redirect_to_path(
            writer,
            return_to,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before skipping to the next track."),
        );
    }

    match state.skip_to_next(&renderer_location) {
        Ok(message) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            Some(&message),
            None,
        ),
        Err(error) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some(&format!("Next failed: {error}")),
        ),
    }
}

pub(crate) fn handle_transport_previous_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let return_to = request
        .query
        .get("return_to")
        .map(String::as_str)
        .unwrap_or("/");

    if renderer_location.is_empty() {
        return redirect_to_path(
            writer,
            return_to,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before going to the previous track."),
        );
    }

    match state.skip_to_previous(&renderer_location) {
        Ok(message) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            Some(&message),
            None,
        ),
        Err(error) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some(&format!("Previous failed: {error}")),
        ),
    }
}

pub(crate) fn handle_queue_append_album_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let return_to = request
        .query
        .get("return_to")
        .map(String::as_str)
        .unwrap_or("/");
    let Some(album_id) = request.query.get("album_id").map(String::as_str) else {
        return redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some("Select an album before adding it to the queue."),
        );
    };

    if renderer_location.is_empty() {
        return redirect_to_path(
            writer,
            return_to,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before queuing music."),
        );
    }

    let Some(album) = state.find_album(album_id) else {
        return redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some("The selected album is no longer in the scanned library."),
        );
    };

    match state.append_album_to_queue(&renderer_location, &album) {
        Ok(queue) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            Some(&format!(
                "Queued album '{}' for {}. Queue length: {}.",
                album.title,
                renderer_location,
                queue.entries.len()
            )),
            None,
        ),
        Err(error) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some(&format!("Queue update failed: {error}")),
        ),
    }
}

pub(crate) fn handle_queue_play_next_album_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let return_to = request
        .query
        .get("return_to")
        .map(String::as_str)
        .unwrap_or("/");
    let Some(album_id) = request.query.get("album_id").map(String::as_str) else {
        return redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some("Select an album before adding it to play next."),
        );
    };

    if renderer_location.is_empty() {
        return redirect_to_path(
            writer,
            return_to,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before queuing music."),
        );
    }

    let Some(album) = state.find_album(album_id) else {
        return redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some("The selected album is no longer in the scanned library."),
        );
    };

    match state.play_next_album(&renderer_location, &album) {
        Ok(queue) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            Some(&format!(
                "Album '{}' will play next. Queue length: {}.",
                album.title,
                queue.entries.len()
            )),
            None,
        ),
        Err(error) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some(&format!("Queue update failed: {error}")),
        ),
    }
}

pub(crate) fn handle_queue_move_up_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_queue_entry_mutation_request(writer, request, state, "move up", |state, renderer, id| {
        state.move_queue_entry_up(renderer, id)
    })
}

pub(crate) fn handle_queue_move_down_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_queue_entry_mutation_request(
        writer,
        request,
        state,
        "move down",
        |state, renderer, id| state.move_queue_entry_down(renderer, id),
    )
}

pub(crate) fn handle_queue_remove_entry_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_queue_entry_mutation_request(writer, request, state, "remove", |state, renderer, id| {
        state.remove_pending_queue_entry(renderer, id)
    })
}

pub(crate) fn handle_queue_entry_mutation_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
    action_label: &str,
    apply: impl Fn(&ServiceState, &str, i64) -> io::Result<PlaybackQueue>,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let return_to = request
        .query
        .get("return_to")
        .map(String::as_str)
        .unwrap_or("/");
    let Some(entry_id) = request
        .query
        .get("entry_id")
        .and_then(|value| value.parse::<i64>().ok())
    else {
        return redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some("Select a queue entry first."),
        );
    };

    if renderer_location.is_empty() {
        return redirect_to_path(
            writer,
            return_to,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before editing the queue."),
        );
    }

    match apply(state, &renderer_location, entry_id) {
        Ok(queue) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            Some(&format!(
                "Queue updated after {}. Queue length: {}.",
                action_label,
                queue.entries.len()
            )),
            None,
        ),
        Err(error) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some(&format!("Queue update failed: {error}")),
        ),
    }
}

pub(crate) fn handle_queue_clear_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request
        .query
        .get("renderer_location")
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    let return_to = request
        .query
        .get("return_to")
        .map(String::as_str)
        .unwrap_or("/");

    if renderer_location.is_empty() {
        return redirect_to_path(
            writer,
            return_to,
            Some(""),
            None,
            Some("Enter a renderer LOCATION URL before clearing a queue."),
        );
    }

    match state.clear_queue(&renderer_location) {
        Ok(()) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            Some("Queue cleared."),
            None,
        ),
        Err(error) => redirect_to_path(
            writer,
            return_to,
            Some(&renderer_location),
            None,
            Some(&format!("Failed to clear queue: {error}")),
        ),
    }
}

pub(crate) fn handle_rescan_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request.query.get("renderer_location").map(String::as_str);
    match state.start_rescan() {
        Ok(track_count) => redirect_home(
            writer,
            renderer_location,
            Some(&format!(
                "Library rescan complete. Indexed {track_count} tracks."
            )),
            None,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => redirect_home(
            writer,
            renderer_location,
            Some("A library rescan is already in progress. Please wait for it to complete."),
            None,
        ),
        Err(error) => redirect_home(
            writer,
            renderer_location,
            None,
            Some(&format!("Library rescan failed: {error}")),
        ),
    }
}

pub(crate) fn handle_rescan_progress_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request.query.get("renderer_location").map(String::as_str);
    respond_sse_scan_progress(writer, state, renderer_location)
}

pub(crate) fn handle_api_renderer_discover_request(
    writer: &mut ResponseWriter,
    _request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let body = format!(
        r#"{{"ok":true,"renderers":{}}}"#,
        render_discovery_json(state)
    );
    respond_json(writer, "200 OK", &body)
}

pub(crate) fn handle_api_renderer_volume_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    match state.renderer_volume(&renderer_location) {
        Ok(volume) => api_renderer_volume_response(writer, &renderer_location, volume),
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            api_error(writer, "501 Not Implemented", &error.to_string())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            api_error(writer, "404 Not Found", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("renderer volume query failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_renderer_volume_set_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let volume = match required_request_value(request, "volume")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
    {
        Some(value) if value <= 100 => value as u8,
        _ => {
            return api_error(
                writer,
                "400 Bad Request",
                "volume must be between 0 and 100",
            );
        }
    };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    match state.set_renderer_volume(&renderer_location, volume) {
        Ok(volume) => api_renderer_volume_response(writer, &renderer_location, volume),
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {
            api_error(writer, "501 Not Implemented", &error.to_string())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            api_error(writer, "404 Not Found", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("renderer volume set failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_renderer_group_create_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let members = request_value(request, "members")
        .or_else(|| request_value(request, "renderer_locations"))
        .map(parse_renderer_group_members)
        .unwrap_or_default();
    if members.len() < 2 {
        return api_error(
            writer,
            "400 Bad Request",
            "renderer groups require at least two members",
        );
    }

    let name = request_value(request, "name").unwrap_or("");
    let source_renderer_location = request_value(request, "source_renderer_location");
    match state.create_renderer_group(
        name,
        &members,
        source_renderer_location,
        request_client_id(request),
    ) {
        Ok(group) => {
            let group_location = crate::renderer::renderer_group_queue_key(&group.id);
            let body = format!(
                r#"{{"ok":true,"message":"Renderer group '{}' created.","renderer_location":"{}","group":{},"queue":{}}}"#,
                json_escape(&group.name),
                json_escape(&group_location),
                renderer_group_json(&group),
                render_queue_json_for_renderer(state, &group_location),
            );
            respond_json(writer, "201 Created", &body)
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            api_error(writer, "400 Bad Request", &error.to_string())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            api_error(writer, "404 Not Found", &error.to_string())
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            api_error(writer, "403 Forbidden", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("renderer group create failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_renderer_group_delete_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let inherit_renderer_location = request_value(request, "inherit_renderer_location");
    match state.delete_renderer_group_by_queue_key(&renderer_location, inherit_renderer_location) {
        Ok(group) => {
            let body = format!(
                r#"{{"ok":true,"message":"Renderer group '{}' deleted.","renderer_location":"{}"}}"#,
                json_escape(&group.name),
                json_escape(&renderer_location),
            );
            respond_json(writer, "200 OK", &body)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            api_error(writer, "404 Not Found", &error.to_string())
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            api_error(writer, "403 Forbidden", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("renderer group delete failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_renderer_group_update_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let members = request_value(request, "members")
        .or_else(|| request_value(request, "renderer_locations"))
        .map(parse_renderer_group_members)
        .unwrap_or_default();
    let name = request_value(request, "name").unwrap_or("");

    match state.update_renderer_group_by_queue_key(
        &renderer_location,
        name,
        &members,
        request_client_id(request),
    ) {
        Ok(group) => {
            let body = format!(
                r#"{{"ok":true,"message":"Renderer group '{}' updated.","renderer_location":"{}","group":{},"queue":{}}}"#,
                json_escape(&group.name),
                json_escape(&renderer_location),
                renderer_group_json(&group),
                render_queue_json_for_renderer(state, &renderer_location),
            );
            respond_json(writer, "200 OK", &body)
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            api_error(writer, "400 Bad Request", &error.to_string())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            api_error(writer, "404 Not Found", &error.to_string())
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            api_error(writer, "403 Forbidden", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("renderer group update failed: {error}"),
        ),
    }
}

fn parse_renderer_group_members(value: &str) -> Vec<String> {
    value
        .split([',', '\n'])
        .map(str::trim)
        .filter(|member| !member.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn handle_api_play_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let track_id = match required_request_value(request, "track_id") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    let Some(track) = state.find_track(&track_id) else {
        return api_error(writer, "404 Not Found", "track not found");
    };
    let _ = state.remember_renderer_location(&renderer_location);
    match state
        .replace_queue_with_track(&renderer_location, &track)
        .and_then(|_| state.start_current_queue_entry(&renderer_location))
    {
        Ok((started_track, _, renderer_name, _)) => api_renderer_state_response(
            writer,
            state,
            &renderer_location,
            &format!(
                "Now playing '{}' on {}.",
                started_track.title, renderer_name
            ),
        ),
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("playback failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_play_album_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let album_id = match required_request_value(request, "album_id") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    let Some(album) = state.find_album(&album_id) else {
        return api_error(writer, "404 Not Found", "album not found");
    };
    let _ = state.remember_renderer_location(&renderer_location);
    match state
        .replace_queue_with_album(&renderer_location, &album)
        .and_then(|_| state.start_current_queue_entry(&renderer_location))
    {
        Ok((started_track, _, renderer_name, _)) => api_renderer_state_response(
            writer,
            state,
            &renderer_location,
            &format!(
                "Started album '{}' from track '{}' on {}.",
                album.title, started_track.title, renderer_name
            ),
        ),
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("playback failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_tidal_search_tracks_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let query = match required_request_value(request, "query") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let limit = request_value(request, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20);

    match state.search_tidal_tracks(&query, limit) {
        Ok(tracks) => {
            let body = serde_json::to_string(&tracks)
                .map_err(|error| io::Error::other(format!("TIDAL JSON failed: {error}")))?;
            respond_json(writer, "200 OK", &body)
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            api_error(writer, "400 Bad Request", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "502 Bad Gateway",
            &format!("TIDAL search failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_tidal_search_albums_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let query = match required_request_value(request, "query") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let limit = request_value(request, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20);

    match state.search_tidal_albums(&query, limit) {
        Ok(albums) => {
            let body = serde_json::to_string(&albums)
                .map_err(|error| io::Error::other(format!("TIDAL JSON failed: {error}")))?;
            respond_json(writer, "200 OK", &body)
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            api_error(writer, "400 Bad Request", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "502 Bad Gateway",
            &format!("TIDAL album search failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_tidal_auth_url_request(
    writer: &mut ResponseWriter,
    _request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    match state.tidal_auth_url() {
        Ok(auth) => {
            let body = serde_json::to_string(&auth)
                .map_err(|error| io::Error::other(format!("TIDAL JSON failed: {error}")))?;
            respond_json(writer, "200 OK", &body)
        }
        Err(error) => api_error(
            writer,
            "502 Bad Gateway",
            &format!("TIDAL auth failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_tidal_complete_auth_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let redirect_url = match required_request_value(request, "redirect_url") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    match state.complete_tidal_auth(&redirect_url) {
        Ok(auth) => {
            let body = serde_json::to_string(&auth)
                .map_err(|error| io::Error::other(format!("TIDAL JSON failed: {error}")))?;
            respond_json(writer, "200 OK", &body)
        }
        Err(error) => api_error(
            writer,
            "502 Bad Gateway",
            &format!("TIDAL auth failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_tidal_play_album_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_tidal_album_action(
        writer,
        request,
        state,
        |state, renderer, album| state.replace_queue_with_tidal_album(renderer, album),
        |album, queue| {
            format!(
                "Queued TIDAL album '{}' for playback. Queue length: {}.",
                tidal_album_display_title(album),
                queue.entries.len()
            )
        },
        true,
    )
}

pub(crate) fn handle_api_tidal_play_track_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_tidal_track_action(
        writer,
        request,
        state,
        |state, renderer, track| state.replace_queue_with_tidal_track(renderer, track),
        |track, queue| {
            format!(
                "Queued TIDAL track '{}' for playback. Queue length: {}.",
                tidal_display_title(track),
                queue.entries.len()
            )
        },
        true,
    )
}

pub(crate) fn handle_api_queue_append_tidal_album_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_tidal_album_action(
        writer,
        request,
        state,
        |state, renderer, album| state.append_tidal_album_to_queue(renderer, album),
        |album, queue| {
            format!(
                "Queued TIDAL album '{}'. Queue length: {}.",
                tidal_album_display_title(album),
                queue.entries.len()
            )
        },
        false,
    )
}

pub(crate) fn handle_api_queue_append_tidal_track_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_tidal_track_action(
        writer,
        request,
        state,
        |state, renderer, track| state.append_tidal_track_to_queue(renderer, track),
        |track, queue| {
            format!(
                "Queued TIDAL track '{}'. Queue length: {}.",
                tidal_display_title(track),
                queue.entries.len()
            )
        },
        false,
    )
}

pub(crate) fn handle_api_queue_play_next_tidal_album_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_tidal_album_action(
        writer,
        request,
        state,
        |state, renderer, album| state.play_next_tidal_album(renderer, album),
        |album, queue| {
            format!(
                "TIDAL album '{}' will play next. Queue length: {}.",
                tidal_album_display_title(album),
                queue.entries.len()
            )
        },
        false,
    )
}

pub(crate) fn handle_api_queue_play_next_tidal_track_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_tidal_track_action(
        writer,
        request,
        state,
        |state, renderer, track| state.play_next_tidal_track(renderer, track),
        |track, queue| {
            format!(
                "TIDAL track '{}' will play next. Queue length: {}.",
                tidal_display_title(track),
                queue.entries.len()
            )
        },
        false,
    )
}

fn handle_api_tidal_album_action(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
    apply: impl Fn(&ServiceState, &str, TidalQueuedAlbum) -> io::Result<PlaybackQueue>,
    message: impl Fn(&TidalQueuedAlbum, &PlaybackQueue) -> String,
    start_playback: bool,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let queued_album = match tidal_album_from_request(request) {
        Ok(album) => album,
        Err(error) => return api_error(writer, "400 Bad Request", &error.to_string()),
    };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    let _ = state.remember_renderer_location(&renderer_location);

    match apply(state, &renderer_location, queued_album.clone()) {
        Ok(_) if start_playback => match state.start_current_queue_entry(&renderer_location) {
            Ok((started_track, _, renderer_name, _)) => api_renderer_state_response(
                writer,
                state,
                &renderer_location,
                &format!(
                    "Now playing '{}' from TIDAL on {}.",
                    started_track.title, renderer_name
                ),
            ),
            Err(error) => api_error(
                writer,
                "502 Bad Gateway",
                &format!("TIDAL playback failed: {error}"),
            ),
        },
        Ok(queue) => api_queue_response(
            writer,
            state,
            &renderer_location,
            &message(&queued_album, &queue),
        ),
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            api_error(writer, "400 Bad Request", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("TIDAL queue update failed: {error}"),
        ),
    }
}

fn handle_api_tidal_track_action(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
    apply: impl Fn(&ServiceState, &str, TidalQueuedTrack) -> io::Result<PlaybackQueue>,
    message: impl Fn(&TidalQueuedTrack, &PlaybackQueue) -> String,
    start_playback: bool,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let queued_track = match tidal_track_from_request(request) {
        Ok(track) => track,
        Err(error) => return api_error(writer, "400 Bad Request", &error.to_string()),
    };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    let _ = state.remember_renderer_location(&renderer_location);

    match apply(state, &renderer_location, queued_track.clone()) {
        Ok(_) if start_playback => match state.start_current_queue_entry(&renderer_location) {
            Ok((started_track, _, renderer_name, _)) => api_renderer_state_response(
                writer,
                state,
                &renderer_location,
                &format!(
                    "Now playing '{}' on {}.",
                    started_track.title, renderer_name
                ),
            ),
            Err(error) => api_error(
                writer,
                "502 Bad Gateway",
                &format!("TIDAL playback failed: {error}"),
            ),
        },
        Ok(queue) => api_queue_response(
            writer,
            state,
            &renderer_location,
            &message(&queued_track, &queue),
        ),
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            api_error(writer, "400 Bad Request", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("TIDAL queue update failed: {error}"),
        ),
    }
}

fn tidal_album_from_request(request: &HttpRequest) -> io::Result<TidalQueuedAlbum> {
    let album_id = required_request_value(request, "tidal_album_id")
        .or_else(|_| required_request_value(request, "album_id"))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    Ok(TidalQueuedAlbum {
        album_id,
        title: optional_request_string(request, "title"),
        artist: optional_request_string(request, "artist"),
        track_count: request_value(request, "track_count")
            .and_then(|value| value.trim().parse::<u64>().ok()),
        duration_seconds: request_value(request, "duration_seconds")
            .and_then(|value| value.trim().parse::<u64>().ok()),
        artwork_url: optional_request_string(request, "artwork_url"),
    })
}

fn tidal_track_from_request(request: &HttpRequest) -> io::Result<TidalQueuedTrack> {
    let track_id = required_request_value(request, "tidal_track_id")
        .or_else(|_| required_request_value(request, "track_id"))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    Ok(TidalQueuedTrack {
        track_id,
        title: optional_request_string(request, "title"),
        artist: optional_request_string(request, "artist"),
        album: optional_request_string(request, "album"),
        duration_seconds: request_value(request, "duration_seconds")
            .and_then(|value| value.trim().parse::<u64>().ok()),
        artwork_url: optional_request_string(request, "artwork_url"),
    })
}

fn optional_request_string(request: &HttpRequest, key: &str) -> Option<String> {
    request_value(request, key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn tidal_album_display_title(album: &TidalQueuedAlbum) -> &str {
    album
        .title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&album.album_id)
}

fn tidal_display_title(track: &TidalQueuedTrack) -> &str {
    track
        .title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&track.track_id)
}

pub(crate) fn handle_api_radio_stations_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let query = request_value(request, "query");
    let country_code =
        request_value(request, "countrycode").or_else(|| request_value(request, "country_code"));
    let tag = request_value(request, "tag");
    let limit = request_value(request, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(20);

    match state.search_radio_stations(query, country_code, tag, limit) {
        Ok(stations) => {
            let body = serde_json::to_string(&stations)
                .map_err(|error| io::Error::other(format!("radio JSON failed: {error}")))?;
            respond_json(writer, "200 OK", &body)
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            api_error(writer, "400 Bad Request", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "502 Bad Gateway",
            &format!("station discovery failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_radio_play_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    let stream_url = match request_value(request, "stream_url")
        .or_else(|| request_value(request, "url"))
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value,
        None => return api_error(writer, "400 Bad Request", "missing stream_url"),
    };
    let station_name = request_value(request, "station_name")
        .or_else(|| request_value(request, "name"))
        .unwrap_or("Internet radio");
    let codec = request_value(request, "codec");
    let artwork_url = request_value(request, "artwork_url")
        .or_else(|| request_value(request, "favicon"))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let station_id = request_value(request, "station_id")
        .or_else(|| request_value(request, "id"))
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let _ = state.remember_renderer_location(&renderer_location);
    match state.play_radio_stream(
        &renderer_location,
        stream_url,
        station_name,
        codec,
        artwork_url,
        station_id,
    ) {
        Ok(renderer_name) => api_renderer_state_response(
            writer,
            state,
            &renderer_location,
            &format!(
                "Now playing radio station '{}' on {}.",
                station_name, renderer_name
            ),
        ),
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            api_error(writer, "400 Bad Request", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("radio playback failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_like_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let item_kind = match required_request_value(request, "item_kind") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let item_id = match required_request_value(request, "item_id") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let client_id = match required_request_value(request, "client_id") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };

    match state.like_item(&item_kind, &item_id, &client_id) {
        Ok(result) => {
            let body = format!(
                r#"{{"ok":true,"item_kind":"{}","item_id":"{}","like_count":{},"liked_by_client":{},"created":{}}}"#,
                json_escape(&result.item_kind),
                json_escape(&result.item_id),
                result.like_count,
                if result.liked_by_client {
                    "true"
                } else {
                    "false"
                },
                if result.created { "true" } else { "false" },
            );
            respond_json(writer, "200 OK", &body)
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            api_error(writer, "400 Bad Request", &error.to_string())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            api_error(writer, "404 Not Found", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("like failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_album_artwork_select_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let album_id = match required_request_value(request, "album_id") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let release_id = match required_request_value(request, "release_id") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };

    match state.apply_album_artwork_candidate(&album_id, &release_id) {
        Ok(album) => {
            let body = format!(
                r#"{{"ok":true,"message":"Artwork saved for '{}'.","album":{}}}"#,
                json_escape(&album.title),
                album_summary_json(&album),
            );
            respond_json(writer, "200 OK", &body)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            api_error(writer, "404 Not Found", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("artwork selection failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_recommendations_import_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let import_request = match parse_recommendation_import_request(request) {
        Ok(request) => request,
        Err(error) => return api_error(writer, "400 Bad Request", &error),
    };
    if import_request.recommendations.is_empty() {
        return api_error(
            writer,
            "400 Bad Request",
            "recommendations must contain at least one item",
        );
    }

    match state.import_album_recommendations(&import_request) {
        Ok(imported) => {
            let body = format!(
                r#"{{"ok":true,"message":"Imported {} recommendation(s).","imported":{}}}"#,
                imported, imported,
            );
            respond_json(writer, "200 OK", &body)
        }
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
            api_error(writer, "400 Bad Request", &error.to_string())
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("recommendation import failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_recommendations_delete_request(
    writer: &mut ResponseWriter,
    _request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    match state.delete_album_recommendations() {
        Ok(deleted) => {
            let body = format!(
                r#"{{"ok":true,"message":"Deleted {} recommendation(s).","deleted":{}}}"#,
                deleted, deleted,
            );
            respond_json(writer, "200 OK", &body)
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("recommendation delete failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_events_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location =
        state.preferred_renderer_location(request_value(request, "renderer_location"));
    if renderer_location.trim().is_empty() {
        return api_error(
            writer,
            "400 Bad Request",
            "renderer_location is required for event streaming",
        );
    }
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }

    respond_sse_stream(writer, state, &renderer_location)
}

fn parse_recommendation_import_request(
    request: &HttpRequest,
) -> Result<RecommendationImportRequest, String> {
    if let Some(payload) = request_value(request, "payload") {
        return serde_json::from_str(payload)
            .map_err(|error| format!("invalid recommendation payload JSON: {error}"));
    }
    serde_json::from_slice(&request.body)
        .map_err(|error| format!("invalid recommendation import JSON: {error}"))
}

pub(crate) fn handle_api_register_android_local_renderer_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !matches!(
        renderer_kind_for_location(&renderer_location),
        RendererKind::AndroidLocal
    ) {
        return api_error(
            writer,
            "400 Bad Request",
            "renderer_location must use the android-local:// scheme",
        );
    }

    let name = match required_request_value(request, "name") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let manufacturer = request_value(request, "manufacturer");
    let model_name = request_value(request, "model_name");
    let client_id = request_client_id(request);
    let visibility = request_value(request, "visibility").unwrap_or(if client_id.is_some() {
        "private"
    } else {
        "public"
    });
    if visibility.eq_ignore_ascii_case("private") && client_id.is_none() {
        return api_error(
            writer,
            "400 Bad Request",
            "client_id is required for private renderers",
        );
    }
    let capabilities = android_local_renderer_capabilities();

    match state.remember_renderer_details_with_visibility(
        &renderer_location,
        &name,
        manufacturer.as_deref(),
        model_name.as_deref(),
        None,
        Some(&capabilities),
        None,
        visibility,
        client_id,
        false,
    ) {
        Ok(()) => api_renderer_state_response(
            writer,
            state,
            &renderer_location,
            &format!("Registered local renderer '{}'.", name),
        ),
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("renderer registration failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_register_cli_local_renderer_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !matches!(
        renderer_kind_for_location(&renderer_location),
        RendererKind::CliLocal
    ) {
        return api_error(
            writer,
            "400 Bad Request",
            "renderer_location must use the cli-local:// scheme",
        );
    }

    let name = request_value(request, "name").unwrap_or("This CLI");
    let client_id = request_client_id(request);
    let visibility = request_value(request, "visibility").unwrap_or(if client_id.is_some() {
        "private"
    } else {
        "public"
    });
    if visibility.eq_ignore_ascii_case("private") && client_id.is_none() {
        return api_error(
            writer,
            "400 Bad Request",
            "client_id is required for private renderers",
        );
    }
    let capabilities = local_renderer_capabilities();

    match state.remember_renderer_details_with_visibility(
        &renderer_location,
        name,
        Some("musicdctl"),
        request_value(request, "model_name"),
        None,
        Some(&capabilities),
        None,
        visibility,
        client_id,
        false,
    ) {
        Ok(()) => api_renderer_state_response(
            writer,
            state,
            &renderer_location,
            &format!("Registered local renderer '{}'.", name),
        ),
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("renderer registration failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_android_local_session_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !matches!(
        renderer_kind_for_location(&renderer_location),
        RendererKind::AndroidLocal
    ) {
        return api_error(writer, "400 Bad Request", "renderer is not android_local");
    }
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }

    let transport_state = match required_request_value(request, "transport_state") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let current_track_uri = request_value(request, "current_track_uri");
    let position_seconds =
        request_value(request, "position_seconds").and_then(|value| value.parse::<u64>().ok());
    let duration_seconds =
        request_value(request, "duration_seconds").and_then(|value| value.parse::<u64>().ok());
    let renderer = match state.resolve_renderer(&renderer_location) {
        Ok(renderer) => renderer,
        Err(error) => {
            return api_error(
                writer,
                "500 Internal Server Error",
                &format!("failed to resolve renderer: {error}"),
            );
        }
    };
    let _ = state.mark_renderer_reachable(&renderer);
    if let Err(error) = state.database.record_transport_snapshot(
        &renderer_location,
        &transport_state,
        current_track_uri.as_deref(),
        position_seconds,
        duration_seconds,
    ) {
        state.debug_log(
            "android-local-session-error",
            format!("renderer={} record_error={}", renderer_location, error),
        );
    }
    if let Err(error) = state.database.sync_queue_status(
        &renderer_location,
        queue_status_for_transport(&transport_state),
    ) {
        state.debug_log(
            "android-local-session-error",
            format!("renderer={} status_error={}", renderer_location, error),
        );
    }
    state.debug_log(
        "android-local-session",
        format!(
            "renderer={} state={} queue_current={:?} uri={:?} position={:?} duration={:?}",
            renderer_location,
            transport_state,
            state
                .queue_snapshot(&renderer_location)
                .and_then(|queue| queue.current_entry_id),
            current_track_uri,
            position_seconds,
            duration_seconds
        ),
    );

    api_renderer_state_response(writer, state, &renderer_location, "Session updated.")
}

pub(crate) fn handle_api_cli_local_session_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !matches!(
        renderer_kind_for_location(&renderer_location),
        RendererKind::CliLocal
    ) {
        return api_error(writer, "400 Bad Request", "renderer is not cli_local");
    }
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }

    let transport_state = match required_request_value(request, "transport_state") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let current_track_uri = request_value(request, "current_track_uri");
    let position_seconds =
        request_value(request, "position_seconds").and_then(|value| value.parse::<u64>().ok());
    let duration_seconds =
        request_value(request, "duration_seconds").and_then(|value| value.parse::<u64>().ok());
    let renderer = match state.resolve_renderer(&renderer_location) {
        Ok(renderer) => renderer,
        Err(error) => {
            return api_error(
                writer,
                "500 Internal Server Error",
                &format!("failed to resolve renderer: {error}"),
            );
        }
    };
    let _ = state.mark_renderer_reachable(&renderer);
    if let Err(error) = state.database.record_transport_snapshot(
        &renderer_location,
        &transport_state,
        current_track_uri,
        position_seconds,
        duration_seconds,
    ) {
        state.debug_log(
            "cli-local-session-error",
            format!("renderer={} record_error={}", renderer_location, error),
        );
    }
    if let Err(error) = state.database.sync_queue_status(
        &renderer_location,
        queue_status_for_transport(&transport_state),
    ) {
        state.debug_log(
            "cli-local-session-error",
            format!("renderer={} status_error={}", renderer_location, error),
        );
    }
    state.debug_log(
        "cli-local-session",
        format!(
            "renderer={} state={} queue_current={:?} uri={:?} position={:?} duration={:?}",
            renderer_location,
            transport_state,
            state
                .queue_snapshot(&renderer_location)
                .and_then(|queue| queue.current_entry_id),
            current_track_uri,
            position_seconds,
            duration_seconds
        ),
    );

    api_renderer_state_response(writer, state, &renderer_location, "Session updated.")
}

pub(crate) fn handle_api_android_local_completed_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !matches!(
        renderer_kind_for_location(&renderer_location),
        RendererKind::AndroidLocal
    ) {
        return api_error(writer, "400 Bad Request", "renderer is not android_local");
    }
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }

    match state
        .database
        .advance_queue_after_completion(&renderer_location)
    {
        Ok(next_entry_id) => {
            state.debug_log(
                "android-local-completed",
                format!(
                    "renderer={} next_entry={:?}",
                    renderer_location, next_entry_id
                ),
            );
            if next_entry_id.is_some() {
                if let Err(error) = state.start_current_queue_entry(&renderer_location) {
                    return api_error(
                        writer,
                        "500 Internal Server Error",
                        &format!("failed to start next queue entry: {error}"),
                    );
                }
                state.events.touch(&renderer_location);
                api_renderer_state_response(
                    writer,
                    state,
                    &renderer_location,
                    "Advanced to the next local queue entry.",
                )
            } else {
                state.events.touch(&renderer_location);
                api_renderer_state_response(
                    writer,
                    state,
                    &renderer_location,
                    "Local queue completed.",
                )
            }
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("completion handling failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_cli_local_completed_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !matches!(
        renderer_kind_for_location(&renderer_location),
        RendererKind::CliLocal
    ) {
        return api_error(writer, "400 Bad Request", "renderer is not cli_local");
    }
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }

    match state
        .database
        .advance_queue_after_completion(&renderer_location)
    {
        Ok(next_entry_id) => {
            state.debug_log(
                "cli-local-completed",
                format!(
                    "renderer={} next_entry={:?}",
                    renderer_location, next_entry_id
                ),
            );
            if next_entry_id.is_some() {
                if let Err(error) = state.start_current_queue_entry(&renderer_location) {
                    return api_error(
                        writer,
                        "500 Internal Server Error",
                        &format!("failed to start next queue entry: {error}"),
                    );
                }
                state.events.touch(&renderer_location);
                api_renderer_state_response(
                    writer,
                    state,
                    &renderer_location,
                    "Advanced to the next CLI queue entry.",
                )
            } else {
                state.events.touch(&renderer_location);
                api_renderer_state_response(
                    writer,
                    state,
                    &renderer_location,
                    "CLI queue completed.",
                )
            }
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("completion handling failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_transport_play_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_transport_action(writer, request, state, |state, renderer| {
        state.resume_renderer(renderer)
    })
}

pub(crate) fn handle_api_transport_pause_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_transport_action(writer, request, state, |state, renderer| {
        state.pause_renderer(renderer)
    })
}

pub(crate) fn handle_api_transport_stop_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_transport_action(writer, request, state, |state, renderer| {
        state.stop_renderer(renderer)
    })
}

pub(crate) fn handle_api_transport_next_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_transport_action(writer, request, state, |state, renderer| {
        state.skip_to_next(renderer)
    })
}

pub(crate) fn handle_api_transport_previous_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_transport_action(writer, request, state, |state, renderer| {
        state.skip_to_previous(renderer)
    })
}

pub(crate) fn handle_api_transport_action(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
    apply: impl Fn(&ServiceState, &str) -> io::Result<String>,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    match apply(state, &renderer_location) {
        Ok(message) => api_renderer_state_response(writer, state, &renderer_location, &message),
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("transport action failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_queue_append_track_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_queue_track_action(
        writer,
        request,
        state,
        |state, renderer, track| state.append_track_to_queue(renderer, track),
        |track, queue| {
            format!(
                "Queued '{}' for renderer. Queue length: {}.",
                track.title,
                queue.entries.len()
            )
        },
    )
}

pub(crate) fn handle_api_queue_play_next_track_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_queue_track_action(
        writer,
        request,
        state,
        |state, renderer, track| state.play_next_track(renderer, track),
        |track, queue| {
            format!(
                "'{}' will play next. Queue length: {}.",
                track.title,
                queue.entries.len()
            )
        },
    )
}

pub(crate) fn handle_api_queue_append_album_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_queue_album_action(
        writer,
        request,
        state,
        |state, renderer, album| state.append_album_to_queue(renderer, album),
        |album, queue| {
            format!(
                "Queued album '{}' for renderer. Queue length: {}.",
                album.title,
                queue.entries.len()
            )
        },
    )
}

pub(crate) fn handle_api_queue_play_next_album_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_queue_album_action(
        writer,
        request,
        state,
        |state, renderer, album| state.play_next_album(renderer, album),
        |album, queue| {
            format!(
                "Album '{}' will play next. Queue length: {}.",
                album.title,
                queue.entries.len()
            )
        },
    )
}

pub(crate) fn handle_api_queue_track_action(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
    apply: impl Fn(&ServiceState, &str, &LibraryTrack) -> io::Result<PlaybackQueue>,
    message: impl Fn(&LibraryTrack, &PlaybackQueue) -> String,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let track_id = match required_request_value(request, "track_id") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    let Some(track) = state.find_track(&track_id) else {
        return api_error(writer, "404 Not Found", "track not found");
    };
    match apply(state, &renderer_location, &track) {
        Ok(queue) => {
            api_queue_response(writer, state, &renderer_location, &message(&track, &queue))
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("queue update failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_queue_album_action(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
    apply: impl Fn(&ServiceState, &str, &AlbumSummary) -> io::Result<PlaybackQueue>,
    message: impl Fn(&AlbumSummary, &PlaybackQueue) -> String,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let album_id = match required_request_value(request, "album_id") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    let Some(album) = state.find_album(&album_id) else {
        return api_error(writer, "404 Not Found", "album not found");
    };
    match apply(state, &renderer_location, &album) {
        Ok(queue) => {
            api_queue_response(writer, state, &renderer_location, &message(&album, &queue))
        }
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("queue update failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_queue_move_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let entry_id =
        match request_value(request, "entry_id").and_then(|value| value.parse::<i64>().ok()) {
            Some(value) => value,
            None => return api_error(writer, "400 Bad Request", "missing or invalid entry_id"),
        };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    let direction = match required_request_value(request, "direction") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let result = match direction.as_str() {
        "up" => state.move_queue_entry_up(&renderer_location, entry_id),
        "down" => state.move_queue_entry_down(&renderer_location, entry_id),
        _ => {
            return api_error(
                writer,
                "400 Bad Request",
                "direction must be 'up' or 'down'",
            );
        }
    };
    match result {
        Ok(queue) => api_queue_response(
            writer,
            state,
            &renderer_location,
            &format!(
                "Queue updated after move {}. Queue length: {}.",
                direction,
                queue.entries.len()
            ),
        ),
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("queue move failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_queue_remove_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let entry_id =
        match request_value(request, "entry_id").and_then(|value| value.parse::<i64>().ok()) {
            Some(value) => value,
            None => return api_error(writer, "400 Bad Request", "missing or invalid entry_id"),
        };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    match state.remove_pending_queue_entry(&renderer_location, entry_id) {
        Ok(queue) => api_queue_response(
            writer,
            state,
            &renderer_location,
            &format!(
                "Queue entry removed. Queue length: {}.",
                queue.entries.len()
            ),
        ),
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("queue remove failed: {error}"),
        ),
    }
}

pub(crate) fn handle_api_queue_clear_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    if !authorize_direct_renderer_access(writer, request, state, &renderer_location)? {
        return Ok(());
    }
    match state.clear_queue(&renderer_location) {
        Ok(()) => api_renderer_state_response(writer, state, &renderer_location, "Queue cleared."),
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("queue clear failed: {error}"),
        ),
    }
}

pub(crate) fn required_request_value(
    request: &HttpRequest,
    key: &str,
) -> Result<String, &'static str> {
    request_value(request, key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or(match key {
            "renderer_location" => "missing renderer_location",
            "track_id" => "missing track_id",
            "album_id" => "missing album_id",
            "direction" => "missing direction",
            _ => "missing required field",
        })
}

pub(crate) fn request_client_id(request: &HttpRequest) -> Option<&str> {
    request_value(request, "client_id")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(crate) fn authorize_direct_renderer_access(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
    renderer_location: &str,
) -> io::Result<bool> {
    match state.check_direct_renderer_access(renderer_location, request_client_id(request)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            api_error(writer, "403 Forbidden", &error.to_string())?;
            Ok(false)
        }
        Err(error) => {
            api_error(
                writer,
                "500 Internal Server Error",
                &format!("renderer access check failed: {error}"),
            )?;
            Ok(false)
        }
    }
}

pub(crate) fn api_queue_response(
    writer: &mut ResponseWriter,
    state: &ServiceState,
    renderer_location: &str,
    message: &str,
) -> io::Result<()> {
    let body = format!(
        r#"{{"ok":true,"message":"{}","renderer_location":"{}","queue":{},"session":{}}}"#,
        json_escape(message),
        json_escape(renderer_location),
        render_queue_json_for_renderer(state, renderer_location),
        session_payload_json_for_renderer(state, renderer_location),
    );
    respond_json(writer, "200 OK", &body)
}

pub(crate) fn api_renderer_state_response(
    writer: &mut ResponseWriter,
    state: &ServiceState,
    renderer_location: &str,
    message: &str,
) -> io::Result<()> {
    let body = format!(
        r#"{{"ok":true,"message":"{}","renderer_location":"{}","queue":{},"session":{}}}"#,
        json_escape(message),
        json_escape(renderer_location),
        render_queue_json_for_renderer(state, renderer_location),
        session_payload_json_for_renderer(state, renderer_location),
    );
    respond_json(writer, "200 OK", &body)
}

pub(crate) fn api_renderer_volume_response(
    writer: &mut ResponseWriter,
    renderer_location: &str,
    volume: u8,
) -> io::Result<()> {
    let body = format!(
        r#"{{"ok":true,"renderer_location":"{}","volume":{}}}"#,
        json_escape(renderer_location),
        volume,
    );
    respond_json(writer, "200 OK", &body)
}

pub(crate) fn handle_track_stream_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let track_id = request.path.trim_start_matches("/stream/track/");
    let Some(track) = state.find_track(track_id) else {
        return respond_not_found(writer, request.method == "HEAD");
    };

    state.debug_log(
        "stream-file-open",
        format!(
            "track_id={} title={} relative_path={:?} path={} uri={} method={} range={:?}",
            track.id,
            track.title,
            track.relative_path,
            track.path.display(),
            state.stream_resource_for_track(&track).stream_url,
            request.method,
            request.range_header
        ),
    );
    debug_log_stream_session_context(state, &track);
    let result = respond_with_file(
        writer,
        &track.path,
        request.method == "HEAD",
        request.range_header.clone(),
    );
    match &result {
        Ok(()) => state.debug_log(
            "stream-file-close",
            format!(
                "track_id={} path={} status=ok method={} range={:?}",
                track.id,
                track.path.display(),
                request.method,
                request.range_header
            ),
        ),
        Err(error) => state.debug_log(
            "stream-file-close",
            format!(
                "track_id={} path={} status=error method={} range={:?} error={}",
                track.id,
                track.path.display(),
                request.method,
                request.range_header,
                error
            ),
        ),
    }
    result
}

pub(crate) fn handle_tidal_stream_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let track_id = request.path.trim_start_matches("/stream/tidal/").trim();
    if track_id.is_empty() {
        return respond_not_found(writer, request.method == "HEAD");
    }
    let source = match state.tidal_stream_source(track_id) {
        Ok(source) => source,
        Err(error) => {
            state.debug_log(
                "tidal-stream-resolve-failed",
                format!("track_id={} error={}", track_id, error),
            );
            return respond_text(
                writer,
                "502 Bad Gateway",
                "text/plain; charset=utf-8",
                format!("TIDAL stream resolve failed: {error}").as_bytes(),
                request.method == "HEAD",
            );
        }
    };

    match source {
        TidalStreamSource::Direct {
            url,
            mime_type,
            stream_format,
        } => handle_tidal_direct_stream(
            writer,
            request,
            state,
            track_id,
            &url,
            &mime_type,
            &stream_format,
        ),
        TidalStreamSource::Segments {
            urls,
            mime_type,
            stream_format,
        } => handle_tidal_segment_stream(
            writer,
            request,
            state,
            track_id,
            &urls,
            &mime_type,
            &stream_format,
        ),
    }
}

fn handle_tidal_direct_stream(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
    track_id: &str,
    stream_url: &str,
    mime_type: &str,
    stream_format: &str,
) -> io::Result<()> {
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(tidal_stream_error)?;
    let mut upstream_request = client.get(stream_url);
    if let Some(range_header) = request.range_header.as_deref() {
        upstream_request = upstream_request.header(RANGE, range_header);
    }
    let mut upstream = upstream_request.send().map_err(tidal_stream_error)?;
    if !upstream.status().is_success() {
        let status = upstream.status();
        state.debug_log(
            "tidal-stream-upstream-failed",
            format!(
                "track_id={} upstream_status={} url={}",
                track_id, status, stream_url
            ),
        );
        return respond_text(
            writer,
            "502 Bad Gateway",
            "text/plain; charset=utf-8",
            format!("TIDAL stream request failed with upstream status {status}").as_bytes(),
            request.method == "HEAD",
        );
    }

    let upstream_status = upstream.status();
    let status = format!(
        "{} {}",
        upstream_status.as_u16(),
        upstream_status.canonical_reason().unwrap_or("OK")
    );
    let headers = tidal_direct_stream_response_headers(mime_type, upstream.headers());
    state.debug_log(
        "tidal-stream-proxy",
        format!(
            "track_id={} status={} mime_type={} format={} range={:?} upstream_url={}",
            track_id, status, mime_type, stream_format, request.range_header, stream_url
        ),
    );
    write_response_owned(writer, &status, &headers, None)?;
    if request.method != "HEAD" {
        io::copy(&mut upstream, writer)?;
        writer.flush()?;
    }
    Ok(())
}

fn handle_tidal_segment_stream(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
    track_id: &str,
    urls: &[String],
    mime_type: &str,
    stream_format: &str,
) -> io::Result<()> {
    state.debug_log(
        "tidal-segment-stream-start",
        format!(
            "track_id={} segments={} mime_type={} format={} range={:?}",
            track_id,
            urls.len(),
            mime_type,
            stream_format,
            request.range_header
        ),
    );
    let headers = vec![
        ("Content-Type".to_string(), mime_type.to_string()),
        ("Cache-Control".to_string(), "no-store".to_string()),
    ];
    write_response_owned(writer, "200 OK", &headers, None)?;
    if request.method == "HEAD" {
        return Ok(());
    }

    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(tidal_stream_error)?;
    for (index, url) in urls.iter().enumerate() {
        let mut upstream = client.get(url).send().map_err(tidal_stream_error)?;
        if !upstream.status().is_success() {
            state.debug_log(
                "tidal-segment-stream-failed",
                format!(
                    "track_id={} segment={} upstream_status={} url={}",
                    track_id,
                    index,
                    upstream.status(),
                    url
                ),
            );
            return Err(io::Error::other(format!(
                "TIDAL segment {index} request failed with upstream status {}",
                upstream.status()
            )));
        }
        io::copy(&mut upstream, writer)?;
    }
    writer.flush()?;
    state.debug_log(
        "tidal-segment-stream-finished",
        format!("track_id={} segments={}", track_id, urls.len()),
    );
    Ok(())
}

fn tidal_direct_stream_response_headers(
    fallback_mime_type: &str,
    upstream_headers: &reqwest::header::HeaderMap,
) -> Vec<(String, String)> {
    let content_type = upstream_headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_mime_type);
    let mut headers = vec![
        ("Content-Type".to_string(), content_type.to_string()),
        ("Accept-Ranges".to_string(), "bytes".to_string()),
        ("Cache-Control".to_string(), "no-store".to_string()),
    ];
    for (name, header_name) in [
        ("Content-Length", CONTENT_LENGTH),
        ("Content-Range", CONTENT_RANGE),
    ] {
        if let Some(value) = upstream_headers
            .get(header_name)
            .and_then(|value| value.to_str().ok())
        {
            headers.push((name.to_string(), value.to_string()));
        }
    }
    headers
}

fn tidal_stream_error(error: reqwest::Error) -> io::Error {
    io::Error::other(format!("TIDAL stream request failed: {error}"))
}

fn debug_log_stream_session_context(state: &ServiceState, requested_track: &LibraryTrack) {
    if !state.debug_enabled() {
        return;
    }
    let requested_uri = state.stream_resource_for_track(requested_track).stream_url;
    let sessions = match state.database.load_playback_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            state.debug_log(
                "stream-session-context-error",
                format!("track_id={} error={}", requested_track.id, error),
            );
            return;
        }
    };

    for session in sessions {
        if !matches!(
            session.transport_state.as_str(),
            "PLAYING" | "TRANSITIONING" | "PAUSED_PLAYBACK"
        ) {
            continue;
        }
        let queue = state.queue_snapshot(&session.renderer_location);
        let queue_entry = queue.as_ref().and_then(|queue| {
            queue
                .current_entry_id
                .and_then(|entry_id| queue.entries.iter().find(|entry| entry.id == entry_id))
        });
        let queue_track = queue_entry.and_then(|entry| state.find_track(&entry.track_id));
        let session_track_id = session
            .current_track_uri
            .as_deref()
            .and_then(stream_track_id_from_uri);
        let session_matches_request = session.current_track_uri.as_deref()
            == Some(requested_uri.as_str())
            || session_track_id == Some(requested_track.id.as_str());
        let queue_matches_request = queue_track
            .as_ref()
            .is_some_and(|track| track.id == requested_track.id);
        let event = if session_matches_request || queue_matches_request {
            "stream-session-match"
        } else {
            "stream-session-mismatch"
        };
        state.debug_log(
            event,
            format!(
                "requested_track={} requested_title={:?} requested_uri={} renderer={} session_state={} session_entry={:?} session_uri={:?} session_uri_track={:?} queue_entry={:?} queue_track={:?} queue_title={:?} position={:?} duration={:?}",
                requested_track.id,
                requested_track.title,
                requested_uri,
                session.renderer_location,
                session.transport_state,
                session.queue_entry_id,
                session.current_track_uri.as_deref(),
                session_track_id,
                queue.as_ref().and_then(|queue| queue.current_entry_id),
                queue_track.as_ref().map(|track| track.id.as_str()),
                queue_track.as_ref().map(|track| track.title.as_str()),
                session.position_seconds,
                session.duration_seconds
            ),
        );
    }
}

fn stream_track_id_from_uri(uri: &str) -> Option<&str> {
    let (_, track_id) = uri.split_once("/stream/track/")?;
    Some(
        track_id
            .split(['?', '#'])
            .next()
            .unwrap_or(track_id)
            .trim_end_matches('/'),
    )
    .filter(|track_id| !track_id.is_empty())
}

pub(crate) fn handle_track_artwork_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let track_id = request.path.trim_start_matches("/artwork/track/");
    let Some(track) = state.find_track(track_id) else {
        return respond_not_found(writer, request.method == "HEAD");
    };
    let Some(artwork_path) = state.track_artwork_path(&track) else {
        return respond_not_found(writer, request.method == "HEAD");
    };

    respond_with_file(
        writer,
        &artwork_path,
        request.method == "HEAD",
        request.range_header.clone(),
    )
}

pub(crate) fn handle_album_artwork_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let album_id = request.path.trim_start_matches("/artwork/album/");
    let Some(artwork_path) = state.album_artwork_path(album_id) else {
        return respond_not_found(writer, request.method == "HEAD");
    };

    respond_with_file(
        writer,
        &artwork_path,
        request.method == "HEAD",
        request.range_header.clone(),
    )
}

pub(crate) fn respond_sse_stream(
    writer: &mut ResponseWriter,
    state: &ServiceState,
    renderer_location: &str,
) -> io::Result<()> {
    metrics::set_response_status(200);
    write!(writer, "HTTP/1.1 200 OK\r\n")?;
    write!(writer, "Connection: keep-alive\r\n")?;
    write!(writer, "Cache-Control: no-cache\r\n")?;
    write!(writer, "Content-Type: text/event-stream; charset=utf-8\r\n")?;
    write!(writer, "X-Accel-Buffering: no\r\n")?;
    write!(writer, "\r\n")?;
    writer.flush()?;

    let _subscriber = state.events.subscribe(renderer_location);
    let mut last_version = state.events.version(renderer_location);
    let mut last_payload = render_playback_event_json_for_renderer(state, renderer_location);
    state.debug_log(
        "sse-connect",
        format!(
            "renderer={} version={} payload_bytes={}",
            renderer_location,
            last_version,
            last_payload.len()
        ),
    );
    if let Err(error) = write_sse_event(writer, "playback", &last_payload) {
        state.debug_log(
            "sse-write-error",
            format!(
                "renderer={} phase=initial version={} error={}",
                renderer_location, last_version, error
            ),
        );
        return Err(error);
    }

    loop {
        let new_version =
            state
                .events
                .wait_for_change(renderer_location, last_version, Duration::from_secs(15));
        let payload = render_playback_event_json_for_renderer(state, renderer_location);
        if payload != last_payload {
            if let Err(error) = write_sse_event(writer, "playback", &payload) {
                state.debug_log(
                    "sse-write-error",
                    format!(
                        "renderer={} phase=playback version={} error={}",
                        renderer_location, new_version, error
                    ),
                );
                return Err(error);
            }
            state.debug_log(
                "sse-playback-sent",
                format!(
                    "renderer={} version={} payload_bytes={}",
                    renderer_location,
                    new_version,
                    payload.len()
                ),
            );
            last_payload = payload;
        } else if new_version == last_version {
            if let Err(error) = write_sse_comment(writer, "ping") {
                state.debug_log(
                    "sse-write-error",
                    format!(
                        "renderer={} phase=ping version={} error={}",
                        renderer_location, last_version, error
                    ),
                );
                return Err(error);
            }
            state.debug_log(
                "sse-ping-sent",
                format!("renderer={} version={}", renderer_location, last_version),
            );
        } else {
            state.debug_log(
                "sse-version-without-payload-change",
                format!(
                    "renderer={} old_version={} new_version={} payload_bytes={}",
                    renderer_location,
                    last_version,
                    new_version,
                    payload.len()
                ),
            );
        }
        last_version = new_version;
    }
}

pub(crate) fn respond_sse_scan_progress(
    writer: &mut ResponseWriter,
    state: &ServiceState,
    _renderer_location: Option<&str>,
) -> io::Result<()> {
    metrics::set_response_status(200);
    write!(writer, "HTTP/1.1 200 OK\r\n")?;
    write!(writer, "Connection: keep-alive\r\n")?;
    write!(writer, "Cache-Control: no-cache\r\n")?;
    write!(writer, "Content-Type: text/event-stream; charset=utf-8\r\n")?;
    write!(writer, "X-Accel-Buffering: no\r\n")?;
    write!(writer, "\r\n")?;
    writer.flush()?;

    if let Err(error) = write_sse_event(writer, "scan_start", r#"{"status":"started"}"#) {
        state.debug_log(
            "sse-scan-start-error",
            format!("error sending scan start: {}", error),
        );
        return Err(error);
    }

    let scan_result = {
        let writer = Mutex::new(&mut *writer);
        state.start_rescan_with_progress(|event| {
            let payload = scan_progress_event_json(&event);
            let mut guard = writer.lock().expect("SSE scan writer lock poisoned");
            write_sse_event(&mut **guard, "scan_progress", &payload)
        })
    };

    match scan_result {
        Ok(track_count) => {
            let payload = format!(
                r#"{{"status":"complete","track_count":{},"percent":100,"message":"{}"}}"#,
                track_count,
                json_escape(&format!(
                    "Library rescan complete. Indexed {} tracks.",
                    track_count
                )),
            );
            if let Err(error) = write_sse_event(writer, "scan_complete", &payload) {
                state.debug_log(
                    "sse-scan-complete-error",
                    format!("error sending scan complete: {}", error),
                );
                return Err(error);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let payload = format!(
                r#"{{"status":"busy","message":"{}"}}"#,
                json_escape(
                    "A library rescan is already in progress. Please wait for it to complete."
                ),
            );
            write_sse_event(writer, "scan_error", &payload)?;
        }
        Err(error) => {
            let payload = format!(
                r#"{{"status":"error","message":"{}"}}"#,
                json_escape(&format!("Library rescan failed: {error}")),
            );
            write_sse_event(writer, "scan_error", &payload)?;
        }
    }

    Ok(())
}

fn scan_progress_event_json(event: &ScanProgressEvent) -> String {
    let total = event
        .total
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string());
    let message = event
        .message
        .as_ref()
        .map(|value| format!(r#""{}""#, json_escape(value)))
        .unwrap_or_else(|| "null".to_string());
    let percent = event
        .percent
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string());
    format!(
        r#"{{"stage":"{}","current":{},"total":{},"percent":{},"message":{}}}"#,
        json_escape(&event.stage),
        event.current,
        total,
        percent,
        message,
    )
}
