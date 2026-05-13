use std::io;
use std::sync::Arc;

use crate::discovery::render_musicd_device_description_xml;
use crate::handlers::{
    authorize_direct_renderer_access, handle_album_artwork_request,
    handle_api_album_artwork_select_request, handle_api_android_local_completed_request,
    handle_api_android_local_session_request, handle_api_cli_local_completed_request,
    handle_api_cli_local_session_request, handle_api_events_request, handle_api_like_request,
    handle_api_play_album_request, handle_api_play_request, handle_api_queue_append_album_request,
    handle_api_queue_append_track_request, handle_api_queue_clear_request,
    handle_api_queue_move_request, handle_api_queue_play_next_album_request,
    handle_api_queue_play_next_track_request, handle_api_queue_remove_request,
    handle_api_recommendations_import_request, handle_api_register_android_local_renderer_request,
    handle_api_register_cli_local_renderer_request, handle_api_renderer_discover_request,
    handle_api_renderer_group_create_request, handle_api_renderer_group_delete_request,
    handle_api_renderer_group_update_request, handle_api_transport_next_request,
    handle_api_transport_pause_request, handle_api_transport_play_request,
    handle_api_transport_previous_request, handle_api_transport_stop_request,
    handle_play_album_request, handle_play_request, handle_queue_append_album_request,
    handle_queue_append_track_request, handle_queue_clear_request, handle_queue_move_down_request,
    handle_queue_move_up_request, handle_queue_play_next_album_request,
    handle_queue_play_next_track_request, handle_queue_remove_entry_request,
    handle_rescan_progress_request, handle_rescan_request, handle_track_artwork_request,
    handle_track_stream_request, handle_transport_next_request, handle_transport_pause_request,
    handle_transport_play_request, handle_transport_previous_request,
    handle_transport_stop_request,
};
use crate::service::ServiceState;
use crate::views::json::{
    render_album_artwork_candidates_json, render_album_detail_json,
    render_album_recommendations_json, render_albums_json, render_artist_detail_json,
    render_artists_json, render_discovery_json, render_metrics_text, render_now_playing_json,
    render_play_history_json, render_playback_targets_json, render_queue_json,
    render_recommendation_seeds_json, render_renderer_groups_json, render_renderers_json,
    render_server_json, render_session_json, render_track_detail_json, render_tracks_json,
};
use crate::views::{
    render_album_detail_page, render_library_page, render_library_rows_json, render_queue_page,
    render_queue_panel_html, render_track_detail_page, render_welcome_page,
};

use crate::assets;

use super::ResponseWriter;
use super::request::{HttpRequest, request_value};
use super::response::{respond_asset, respond_method_not_allowed, respond_not_found, respond_text};

pub(crate) fn handle_service_request(
    writer: &mut ResponseWriter,
    request: &HttpRequest,
    state: Arc<ServiceState>,
) -> io::Result<()> {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") | ("HEAD", "/") => {
            let body = render_welcome_page(&state, request);
            respond_text(
                writer,
                "200 OK",
                "text/html; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/library") | ("HEAD", "/library") => {
            let body = render_library_page(&state, request);
            respond_text(
                writer,
                "200 OK",
                "text/html; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/library/rows") | ("HEAD", "/library/rows") => {
            let body = render_library_rows_json(&state, request);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/queue") | ("HEAD", "/queue") => {
            let body = render_queue_page(&state, request);
            respond_text(
                writer,
                "200 OK",
                "text/html; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/health") | ("HEAD", "/health") => respond_text(
            writer,
            "200 OK",
            "text/plain; charset=utf-8",
            b"ok",
            request.method == "HEAD",
        ),
        ("GET", "/assets/home.css") | ("HEAD", "/assets/home.css") => respond_asset(
            writer,
            "text/css; charset=utf-8",
            assets::HOME_CSS.as_bytes(),
            request.method == "HEAD",
        ),
        ("GET", "/assets/home.js") | ("HEAD", "/assets/home.js") => respond_asset(
            writer,
            "text/javascript; charset=utf-8",
            assets::HOME_JS.as_bytes(),
            request.method == "HEAD",
        ),
        ("GET", "/assets/album_detail.css") | ("HEAD", "/assets/album_detail.css") => {
            respond_asset(
                writer,
                "text/css; charset=utf-8",
                assets::ALBUM_DETAIL_CSS.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/assets/track_detail.css") | ("HEAD", "/assets/track_detail.css") => {
            respond_asset(
                writer,
                "text/css; charset=utf-8",
                assets::TRACK_DETAIL_CSS.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/description.xml") | ("HEAD", "/description.xml") => {
            let body = render_musicd_device_description_xml(&state.config);
            respond_text(
                writer,
                "200 OK",
                "application/xml; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/metrics") | ("HEAD", "/metrics") => {
            let body = render_metrics_text(&state);
            respond_text(
                writer,
                "200 OK",
                "text/plain; version=0.0.4; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/tracks") | ("HEAD", "/api/tracks") => {
            let body = render_tracks_json(&state, request);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/albums") | ("HEAD", "/api/albums") => {
            let body = render_albums_json(&state, request);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/recommendation-seeds") | ("HEAD", "/api/recommendation-seeds") => {
            let body = render_recommendation_seeds_json(&state);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/recommendations") | ("HEAD", "/api/recommendations") => {
            let body = render_album_recommendations_json(&state, request);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/renderers") | ("HEAD", "/api/renderers") => {
            let body = render_renderers_json(&state, request);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/playback-targets") | ("HEAD", "/api/playback-targets") => {
            let body = render_playback_targets_json(&state, request);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/renderer-groups") | ("HEAD", "/api/renderer-groups") => {
            let body = render_renderer_groups_json(&state);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/server") | ("HEAD", "/api/server") => {
            let body = render_server_json(&state);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/session") | ("HEAD", "/api/session") => {
            let renderer_location =
                state.preferred_renderer_location(request_value(request, "renderer_location"));
            if !renderer_location.is_empty()
                && !authorize_direct_renderer_access(writer, request, &state, &renderer_location)?
            {
                return Ok(());
            }
            let body = render_session_json(&state, request);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/now-playing") | ("HEAD", "/api/now-playing") => {
            let renderer_location =
                state.preferred_renderer_location(request_value(request, "renderer_location"));
            if !renderer_location.is_empty()
                && !authorize_direct_renderer_access(writer, request, &state, &renderer_location)?
            {
                return Ok(());
            }
            let body = render_now_playing_json(&state, request);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/queue") | ("HEAD", "/api/queue") => {
            let renderer_location =
                state.preferred_renderer_location(request_value(request, "renderer_location"));
            if !renderer_location.is_empty()
                && !authorize_direct_renderer_access(writer, request, &state, &renderer_location)?
            {
                return Ok(());
            }
            let body = render_queue_json(&state, request);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/play-history") | ("HEAD", "/api/play-history") => {
            let body = render_play_history_json(&state);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/events") => handle_api_events_request(writer, request, &state),
        ("GET", "/api/artists") | ("HEAD", "/api/artists") => {
            let body = render_artists_json(&state);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("POST", "/api/renderers/discover") => {
            handle_api_renderer_discover_request(writer, request, &state)
        }
        ("POST", "/api/renderer-groups") => {
            handle_api_renderer_group_create_request(writer, request, &state)
        }
        ("POST", "/api/renderer-groups/delete") => {
            handle_api_renderer_group_delete_request(writer, request, &state)
        }
        ("POST", "/api/renderer-groups/update") => {
            handle_api_renderer_group_update_request(writer, request, &state)
        }
        ("POST", "/api/renderers/register-android-local") => {
            handle_api_register_android_local_renderer_request(writer, request, &state)
        }
        ("POST", "/api/renderers/register-cli-local") => {
            handle_api_register_cli_local_renderer_request(writer, request, &state)
        }
        ("POST", "/api/renderers/android-local/session") => {
            handle_api_android_local_session_request(writer, request, &state)
        }
        ("POST", "/api/renderers/android-local/completed") => {
            handle_api_android_local_completed_request(writer, request, &state)
        }
        ("POST", "/api/renderers/cli-local/session") => {
            handle_api_cli_local_session_request(writer, request, &state)
        }
        ("POST", "/api/renderers/cli-local/completed") => {
            handle_api_cli_local_completed_request(writer, request, &state)
        }
        ("POST", "/api/play") => handle_api_play_request(writer, request, &state),
        ("POST", "/api/play-album") => handle_api_play_album_request(writer, request, &state),
        ("POST", "/api/like") => handle_api_like_request(writer, request, &state),
        ("POST", "/api/albums/artwork/select") => {
            handle_api_album_artwork_select_request(writer, request, &state)
        }
        ("POST", "/api/recommendations/import") => {
            handle_api_recommendations_import_request(writer, request, &state)
        }
        ("POST", "/api/transport/play") => {
            handle_api_transport_play_request(writer, request, &state)
        }
        ("POST", "/api/transport/pause") => {
            handle_api_transport_pause_request(writer, request, &state)
        }
        ("POST", "/api/transport/stop") => {
            handle_api_transport_stop_request(writer, request, &state)
        }
        ("POST", "/api/transport/next") => {
            handle_api_transport_next_request(writer, request, &state)
        }
        ("POST", "/api/transport/previous") => {
            handle_api_transport_previous_request(writer, request, &state)
        }
        ("POST", "/api/queue/append-track") => {
            handle_api_queue_append_track_request(writer, request, &state)
        }
        ("POST", "/api/queue/append-album") => {
            handle_api_queue_append_album_request(writer, request, &state)
        }
        ("POST", "/api/queue/play-next-track") => {
            handle_api_queue_play_next_track_request(writer, request, &state)
        }
        ("POST", "/api/queue/play-next-album") => {
            handle_api_queue_play_next_album_request(writer, request, &state)
        }
        ("POST", "/api/queue/move") => handle_api_queue_move_request(writer, request, &state),
        ("POST", "/api/queue/remove") => handle_api_queue_remove_request(writer, request, &state),
        ("POST", "/api/queue/clear") => handle_api_queue_clear_request(writer, request, &state),
        ("GET", "/queue/panel") | ("HEAD", "/queue/panel") => {
            let body = render_queue_panel_html(&state, request);
            respond_text(
                writer,
                "200 OK",
                "text/html; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        _ if request.path.starts_with("/api/tracks/") => {
            if request.method != "GET" && request.method != "HEAD" {
                return respond_method_not_allowed(writer);
            }
            let body = render_track_detail_json(&state, request);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        _ if request.path.starts_with("/api/albums/") => {
            if request.method != "GET" && request.method != "HEAD" {
                return respond_method_not_allowed(writer);
            }
            let body = if request.path.ends_with("/artwork/candidates") {
                render_album_artwork_candidates_json(&state, request)
            } else {
                render_album_detail_json(&state, request)
            };
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        _ if request.path.starts_with("/api/artists/") => {
            if request.method != "GET" && request.method != "HEAD" {
                return respond_method_not_allowed(writer);
            }
            let body = render_artist_detail_json(&state, request);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/renderers/discover") | ("HEAD", "/api/renderers/discover") => {
            let body = render_discovery_json(&state);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        _ if request.path.starts_with("/track/") => {
            if request.method != "GET" && request.method != "HEAD" {
                return respond_method_not_allowed(writer);
            }
            let body = render_track_detail_page(&state, request);
            respond_text(
                writer,
                "200 OK",
                "text/html; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        _ if request.path.starts_with("/album/") => {
            if request.method != "GET" && request.method != "HEAD" {
                return respond_method_not_allowed(writer);
            }
            let body = render_album_detail_page(&state, request);
            respond_text(
                writer,
                "200 OK",
                "text/html; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/play") => handle_play_request(writer, request, &state),
        ("GET", "/play-album") => handle_play_album_request(writer, request, &state),
        ("GET", "/transport/play") => handle_transport_play_request(writer, request, &state),
        ("GET", "/transport/pause") => handle_transport_pause_request(writer, request, &state),
        ("GET", "/transport/stop") => handle_transport_stop_request(writer, request, &state),
        ("GET", "/transport/next") => handle_transport_next_request(writer, request, &state),
        ("GET", "/transport/previous") => {
            handle_transport_previous_request(writer, request, &state)
        }
        ("GET", "/queue/play-next-track") => {
            handle_queue_play_next_track_request(writer, request, &state)
        }
        ("GET", "/queue/play-next-album") => {
            handle_queue_play_next_album_request(writer, request, &state)
        }
        ("GET", "/queue/append-track") => {
            handle_queue_append_track_request(writer, request, &state)
        }
        ("GET", "/queue/append-album") => {
            handle_queue_append_album_request(writer, request, &state)
        }
        ("GET", "/queue/move-up") => handle_queue_move_up_request(writer, request, &state),
        ("GET", "/queue/move-down") => handle_queue_move_down_request(writer, request, &state),
        ("GET", "/queue/remove-entry") => {
            handle_queue_remove_entry_request(writer, request, &state)
        }
        ("GET", "/queue/clear") => handle_queue_clear_request(writer, request, &state),
        ("GET", "/rescan") => handle_rescan_request(writer, request, &state),
        ("GET", "/rescan-progress") => handle_rescan_progress_request(writer, request, &state),
        ("HEAD", "/play")
        | ("HEAD", "/play-album")
        | ("HEAD", "/api/play")
        | ("HEAD", "/api/play-album")
        | ("HEAD", "/api/like")
        | ("HEAD", "/api/transport/play")
        | ("HEAD", "/api/transport/pause")
        | ("HEAD", "/api/transport/stop")
        | ("HEAD", "/api/transport/next")
        | ("HEAD", "/api/transport/previous")
        | ("HEAD", "/api/queue/append-track")
        | ("HEAD", "/api/queue/append-album")
        | ("HEAD", "/api/queue/play-next-track")
        | ("HEAD", "/api/queue/play-next-album")
        | ("HEAD", "/api/queue/move")
        | ("HEAD", "/api/queue/remove")
        | ("HEAD", "/api/queue/clear")
        | ("HEAD", "/transport/play")
        | ("HEAD", "/transport/pause")
        | ("HEAD", "/transport/stop")
        | ("HEAD", "/transport/next")
        | ("HEAD", "/transport/previous")
        | ("HEAD", "/queue/play-next-track")
        | ("HEAD", "/queue/play-next-album")
        | ("HEAD", "/queue/append-track")
        | ("HEAD", "/queue/append-album")
        | ("HEAD", "/queue/move-up")
        | ("HEAD", "/queue/move-down")
        | ("HEAD", "/queue/remove-entry")
        | ("HEAD", "/queue/clear")
        | ("HEAD", "/rescan") => respond_method_not_allowed(writer),
        _ if request.path.starts_with("/stream/track/") => {
            if request.method != "GET" && request.method != "HEAD" {
                return respond_method_not_allowed(writer);
            }
            handle_track_stream_request(writer, request, &state)
        }
        _ if request.path.starts_with("/artwork/track/") => {
            if request.method != "GET" && request.method != "HEAD" {
                return respond_method_not_allowed(writer);
            }
            handle_track_artwork_request(writer, request, &state)
        }
        _ if request.path.starts_with("/artwork/album/") => {
            if request.method != "GET" && request.method != "HEAD" {
                return respond_method_not_allowed(writer);
            }
            handle_album_artwork_request(writer, request, &state)
        }
        _ => respond_not_found(writer, request.method == "HEAD"),
    }
}
