use std::collections::{HashMap, HashSet};
use std::time::Duration;

use musicd_upnp::{discover_renderers, inspect_renderer};

use crate::http::{HttpRequest, request_value};
use crate::library::inspect_embedded_metadata;
use crate::renderer::{renderer_group_queue_key, renderer_kind_for_location, renderer_kind_name};
use crate::service::ServiceState;
use crate::service::tidal::TidalQueuedTrack;
use crate::types::{
    AlbumSummary, ArtistSummary, EmbeddedMetadata, LibraryTrack, PlaybackSession, QueueEntry,
    RendererGroup, RendererRecord, TrackPlayRecord,
};
use crate::util::{
    bool_json, json_escape, now_unix_timestamp, option_bool_json, option_i64_json,
    option_json_fragment, option_string_json, option_u32_json, option_u64_json, string_list_json,
};

pub(crate) fn render_track_detail_json(state: &ServiceState, request: &HttpRequest) -> String {
    let track_id = request.path.trim_start_matches("/api/tracks/");
    let Some(track) = state.find_track(track_id) else {
        return r#"{"error":"track not found"}"#.to_string();
    };
    let client_id = request_value(request, "client_id");
    let like_count = state
        .track_like_counts()
        .get(&track.id)
        .copied()
        .unwrap_or(0);
    let liked_by_client = state.client_liked_track_ids(client_id).contains(&track.id);

    let metadata =
        inspect_embedded_metadata(&track.path).unwrap_or_else(|error| EmbeddedMetadata {
            format_name: "Unreadable".to_string(),
            fields: Vec::new(),
            notes: vec![format!("Failed to inspect embedded metadata: {error}")],
        });

    let fields = metadata
        .fields
        .iter()
        .map(|(key, value)| {
            format!(
                r#"{{"key":"{}","value":"{}"}}"#,
                json_escape(key),
                json_escape(value)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let notes = metadata
        .notes
        .iter()
        .map(|note| format!(r#""{}""#, json_escape(note)))
        .collect::<Vec<_>>()
        .join(",");
    let artwork_json = track.artwork.as_ref().map_or_else(
        || "null".to_string(),
        |artwork| {
            format!(
                r#"{{"url":"{}","source":"{}","mime_type":"{}","cache_key":"{}"}}"#,
                json_escape(&format!("/artwork/track/{}", track.id)),
                json_escape(&artwork.source),
                json_escape(&artwork.mime_type),
                json_escape(&artwork.cache_key),
            )
        },
    );

    format!(
        r#"{{"id":"{}","album_id":"{}","title":"{}","artist":"{}","album":"{}","disc_number":{},"track_number":{},"duration_seconds":{},"relative_path":"{}","absolute_path":"{}","mime_type":"{}","size":{},"artwork":{},"metadata":{},"like_count":{},"liked_by_client":{},"embedded_metadata":{{"parser":"{}","fields":[{}],"notes":[{}]}}}}"#,
        json_escape(&track.id),
        json_escape(&track.album_id),
        json_escape(&track.title),
        json_escape(&track.artist),
        json_escape(&track.album),
        option_u32_json(track.disc_number),
        option_u32_json(track.track_number),
        option_u64_json(track.duration_seconds),
        json_escape(&track.relative_path),
        json_escape(&track.path.display().to_string()),
        json_escape(&track.mime_type),
        track.file_size,
        artwork_json,
        track_metadata_json(&track),
        like_count,
        bool_json(liked_by_client),
        json_escape(&metadata.format_name),
        fields,
        notes,
    )
}

pub(crate) fn render_tracks_json(state: &ServiceState, request: &HttpRequest) -> String {
    let tracks = state.tracks_snapshot();
    let client_id = request_value(request, "client_id");
    let like_counts = state.track_like_counts();
    let liked_track_ids = state.client_liked_track_ids(client_id);
    let album_artwork_by_id = state
        .albums_snapshot()
        .iter()
        .filter_map(|album| {
            album
                .artwork_url
                .as_ref()
                .map(|artwork_url| (album.id.clone(), artwork_url.clone()))
        })
        .collect::<HashMap<String, String>>();
    let entries = tracks
        .iter()
        .map(|track| {
            let fallback_artwork_url = album_artwork_by_id.get(&track.album_id).map(String::as_str);
            let summary_json = track_summary_json_with_likes(
                track,
                fallback_artwork_url,
                like_counts.get(&track.id).copied().unwrap_or(0),
                liked_track_ids.contains(&track.id),
            );
            if let Some(stripped) = summary_json.strip_suffix('}') {
                format!(
                    r#"{stripped},"path":"{}","size":{}}}"#,
                    json_escape(&track.relative_path),
                    track.file_size,
                )
            } else {
                summary_json
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{entries}]")
}

pub(crate) fn render_albums_json(state: &ServiceState, request: &HttpRequest) -> String {
    let albums = state.albums_snapshot();
    let mut sorted_albums = albums.iter().collect::<Vec<_>>();
    let client_id = request_value(request, "client_id");
    let like_counts = state.album_like_counts();
    let liked_album_ids = state.client_liked_album_ids(client_id);

    sorted_albums.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });

    let entries = sorted_albums
        .into_iter()
        .map(|album| {
            album_summary_json_with_likes(
                album,
                like_counts.get(&album.id).copied().unwrap_or(0),
                liked_album_ids.contains(&album.id),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{entries}]")
}

pub(crate) fn render_artists_json(state: &ServiceState) -> String {
    let artists = state.artists_snapshot();
    let entries = artists
        .into_iter()
        .map(|artist| artist_summary_json(&artist))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{entries}]")
}

pub(crate) fn render_album_detail_json(state: &ServiceState, request: &HttpRequest) -> String {
    let album_id = request
        .path
        .trim_start_matches("/api/albums/")
        .trim_end_matches("/artwork/candidates");
    let Some(album) = state.find_album(album_id) else {
        return r#"{"error":"album not found"}"#.to_string();
    };
    let client_id = request_value(request, "client_id");
    let album_like_count = state
        .album_like_counts()
        .get(&album.id)
        .copied()
        .unwrap_or(0);
    let liked_album_ids = state.client_liked_album_ids(client_id);
    let track_like_counts = state.track_like_counts();
    let liked_track_ids = state.client_liked_track_ids(client_id);
    let tracks = state.tracks_for_album(&album.id);
    let tracks_json = tracks
        .into_iter()
        .map(|track| {
            track_summary_json_with_likes(
                &track,
                album.artwork_url.as_deref(),
                track_like_counts.get(&track.id).copied().unwrap_or(0),
                liked_track_ids.contains(&track.id),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"id":"{}","title":"{}","artist":"{}","track_count":{},"first_track_id":"{}","artwork_url":"{}","metadata":{},"like_count":{},"liked_by_client":{},"tracks":[{}]}}"#,
        json_escape(&album.id),
        json_escape(&album.title),
        json_escape(&album.artist),
        album.track_count,
        json_escape(&album.first_track_id),
        json_escape(album.artwork_url.as_deref().unwrap_or_default()),
        album_metadata_json(&album),
        album_like_count,
        bool_json(liked_album_ids.contains(&album.id)),
        tracks_json,
    )
}

pub(crate) fn album_summary_json(album: &AlbumSummary) -> String {
    album_summary_json_with_likes(album, 0, false)
}

pub(crate) fn album_summary_json_with_likes(
    album: &AlbumSummary,
    like_count: u64,
    liked_by_client: bool,
) -> String {
    format!(
        r#"{{"id":"{}","title":"{}","artist":"{}","track_count":{},"first_track_id":"{}","artwork_url":"{}","metadata":{},"like_count":{},"liked_by_client":{}}}"#,
        json_escape(&album.id),
        json_escape(&album.title),
        json_escape(&album.artist),
        album.track_count,
        json_escape(&album.first_track_id),
        json_escape(album.artwork_url.as_deref().unwrap_or_default()),
        album_metadata_json(album),
        like_count,
        bool_json(liked_by_client),
    )
}

pub(crate) fn artist_summary_json(artist: &ArtistSummary) -> String {
    format!(
        r#"{{"id":"{}","name":"{}","album_count":{},"track_count":{},"artwork_url":{},"first_album_id":"{}"}}"#,
        json_escape(&artist.id),
        json_escape(&artist.name),
        artist.album_count,
        artist.track_count,
        option_string_json(artist.artwork_url.as_deref()),
        json_escape(&artist.first_album_id),
    )
}

pub(crate) fn render_artist_detail_json(state: &ServiceState, request: &HttpRequest) -> String {
    let artist_id = request.path.trim_start_matches("/api/artists/");
    let Some(artist) = state.find_artist(artist_id) else {
        return r#"{"error":"artist not found"}"#.to_string();
    };
    let albums = state.albums_for_artist(&artist.id);
    let client_id = request_value(request, "client_id");
    let like_counts = state.album_like_counts();
    let liked_album_ids = state.client_liked_album_ids(client_id);
    let albums_json = albums
        .into_iter()
        .map(|album| {
            album_summary_json_with_likes(
                &album,
                like_counts.get(&album.id).copied().unwrap_or(0),
                liked_album_ids.contains(&album.id),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"id":"{}","name":"{}","album_count":{},"track_count":{},"artwork_url":{},"first_album_id":"{}","albums":[{}]}}"#,
        json_escape(&artist.id),
        json_escape(&artist.name),
        artist.album_count,
        artist.track_count,
        option_string_json(artist.artwork_url.as_deref()),
        json_escape(&artist.first_album_id),
        albums_json,
    )
}

pub(crate) fn render_album_artwork_candidates_json(
    state: &ServiceState,
    request: &HttpRequest,
) -> String {
    let album_id = request
        .path
        .trim_start_matches("/api/albums/")
        .trim_end_matches("/artwork/candidates");
    let Some(album) = state.find_album(album_id) else {
        return r#"{"error":"album not found"}"#.to_string();
    };
    match state.search_album_artwork_candidates(&album.id) {
        Ok(candidates) => {
            let candidates_json = candidates
                .into_iter()
                .map(|candidate| {
                    format!(
                        r#"{{"release_id":"{}","release_group_id":{},"title":"{}","artist":"{}","date":{},"country":{},"score":{},"thumbnail_url":"{}","image_url":"{}","source":"{}"}}"#,
                        json_escape(&candidate.release_id),
                        option_string_json(candidate.release_group_id.as_deref()),
                        json_escape(&candidate.title),
                        json_escape(&candidate.artist),
                        option_string_json(candidate.date.as_deref()),
                        option_string_json(candidate.country.as_deref()),
                        candidate.score,
                        json_escape(&candidate.thumbnail_url),
                        json_escape(&candidate.image_url),
                        json_escape(&candidate.source),
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                r#"{{"album":{},"candidates":[{}]}}"#,
                album_summary_json(&album),
                candidates_json,
            )
        }
        Err(error) => format!(
            r#"{{"album":{},"error":"{}","candidates":[]}}"#,
            album_summary_json(&album),
            json_escape(&error.to_string()),
        ),
    }
}

pub(crate) fn render_renderers_json(state: &ServiceState, request: &HttpRequest) -> String {
    let client_id = request_value(request, "client_id");
    let renderers = state.enriched_renderer_snapshot();
    let groups = state.renderer_group_snapshot();
    let group_member_locations = groups
        .iter()
        .flat_map(|group| group.members.iter())
        .map(|member| member.renderer_location.clone())
        .collect::<HashSet<_>>();
    let selected = state
        .database
        .last_selected_renderer_location()
        .ok()
        .flatten();
    let mut entries = renderers
        .into_iter()
        .filter_map(|renderer| {
            let direct_access = state.renderer_is_visible_to_client(&renderer, client_id);
            (direct_access || group_member_locations.contains(&renderer.location))
                .then_some((renderer, direct_access))
        })
        .map(|(renderer, direct_access)| {
            renderer_record_json_with_access(
                &renderer,
                direct_access && selected.as_deref() == Some(renderer.location.as_str()),
                direct_access,
            )
        })
        .collect::<Vec<_>>();
    entries.extend(groups.into_iter().map(|group| {
        let location = renderer_group_queue_key(&group.id);
        renderer_group_as_renderer_json(&group, selected.as_deref() == Some(location.as_str()))
    }));
    let entries = entries.join(",");
    format!("[{entries}]")
}

pub(crate) fn render_renderer_groups_json(state: &ServiceState) -> String {
    let entries = state
        .renderer_group_snapshot()
        .into_iter()
        .map(|group| renderer_group_json(&group))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{entries}]")
}

pub(crate) fn render_playback_targets_json(state: &ServiceState, request: &HttpRequest) -> String {
    let client_id = request_value(request, "client_id");
    let selected = state
        .database
        .last_selected_renderer_location()
        .ok()
        .flatten();
    let mut targets = state
        .enriched_renderer_snapshot()
        .into_iter()
        .filter(|renderer| state.renderer_is_visible_to_client(renderer, client_id))
        .map(|renderer| {
            format!(
                r#"{{"kind":"renderer","id":"{}","location":"{}","name":"{}","selected":{},"queue_summary":{}}}"#,
                json_escape(&renderer.location),
                json_escape(&renderer.location),
                json_escape(&renderer.name),
                bool_json(selected.as_deref() == Some(renderer.location.as_str())),
                queue_summary_json_for_renderer(state, &renderer.location),
            )
        })
        .collect::<Vec<_>>();
    targets.extend(state.renderer_group_snapshot().into_iter().map(|group| {
        let location = renderer_group_queue_key(&group.id);
        format!(
            r#"{{"kind":"group","id":"{}","location":"{}","name":"{}","member_count":{},"members":{},"selected":{},"queue_summary":{}}}"#,
            json_escape(&group.id),
            json_escape(&location),
            json_escape(&group.name),
            group.members.len(),
            renderer_group_members_json(&group),
            bool_json(selected.as_deref() == Some(location.as_str())),
            queue_summary_json_for_renderer(state, &location),
        )
    }));
    format!(r#"{{"targets":[{}]}}"#, targets.join(","))
}

pub(crate) fn render_server_json(state: &ServiceState) -> String {
    format!(
        r#"{{"name":"{}","base_url":"{}","bind_address":"{}"}}"#,
        json_escape(&state.config.instance_name),
        json_escape(&state.config.resolved_base_url()),
        json_escape(&state.config.bind_address),
    )
}

pub(crate) fn render_recommendation_seeds_json(state: &ServiceState) -> String {
    serde_json::json!({
        "seeds": state.recommendation_seeds(),
    })
    .to_string()
}

pub(crate) fn render_album_recommendations_json(
    state: &ServiceState,
    request: &HttpRequest,
) -> String {
    let seed_album_id = request_value(request, "album_id")
        .or_else(|| request_value(request, "seed_album_id"))
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let status = request_value(request, "status")
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let exclude_library = truthy_request_value(request, "exclude_library");
    let randomize = truthy_request_value(request, "random");
    let limit = request_value(request, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .map(|value| value.clamp(1, 50));
    serde_json::json!({
        "recommendations": state.album_recommendations_for_display(
            seed_album_id,
            status,
            exclude_library,
            randomize,
            limit,
        ),
    })
    .to_string()
}

fn truthy_request_value(request: &HttpRequest, key: &str) -> bool {
    matches!(
        request_value(request, key).map(|value| value.trim().to_ascii_lowercase()),
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on")
    )
}

pub(crate) fn render_play_history_json(state: &ServiceState) -> String {
    let records = state
        .database
        .load_recent_track_play_history(20)
        .unwrap_or_default();
    let entries = records
        .iter()
        .map(|record| play_history_record_json(state, record))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"limit":20,"count":{},"entries":[{}]}}"#,
        records.len(),
        entries
    )
}

fn play_history_record_json(state: &ServiceState, record: &TrackPlayRecord) -> String {
    let track_json = state.find_track(&record.track_id).map_or_else(
        || "null".to_string(),
        |track| {
            let fallback_artwork_url = state
                .find_album(&track.album_id)
                .and_then(|album| album.artwork_url);
            track_summary_json(&track, fallback_artwork_url.as_deref())
        },
    );
    format!(
        r#"{{"id":{},"track_id":"{}","renderer_location":"{}","queue_entry_id":{},"played_unix":{},"track":{}}}"#,
        record.id,
        json_escape(&record.track_id),
        json_escape(&record.renderer_location),
        option_i64_json(record.queue_entry_id),
        record.played_unix,
        track_json,
    )
}

pub(crate) fn render_metrics_text(state: &ServiceState) -> String {
    state.metrics().map(|m| m.encode()).unwrap_or_default()
}

pub(crate) fn render_now_playing_json(state: &ServiceState, request: &HttpRequest) -> String {
    let renderer_location =
        state.preferred_renderer_location(request_value(request, "renderer_location"));
    render_now_playing_json_for_renderer(state, &renderer_location)
}

pub(crate) fn render_now_playing_json_for_renderer(
    state: &ServiceState,
    renderer_location: &str,
) -> String {
    let renderer_json = if let Ok(Some(group)) = state
        .database
        .load_renderer_group_by_queue_key(renderer_location)
    {
        renderer_group_as_renderer_json(&group, true)
    } else {
        state
            .enriched_renderer_record(renderer_location)
            .map(|renderer| renderer_record_json(&renderer, true))
            .unwrap_or_else(|| "null".to_string())
    };
    let current_track_json = current_track_json_for_renderer(state, renderer_location);
    let session_json = session_payload_json_for_renderer(state, renderer_location);
    let queue_summary_json = queue_summary_json_for_renderer(state, renderer_location);
    format!(
        r#"{{"renderer_location":"{}","renderer":{},"current_track":{},"session":{},"queue_summary":{}}}"#,
        json_escape(renderer_location),
        renderer_json,
        current_track_json,
        session_json,
        queue_summary_json,
    )
}

pub(crate) fn render_playback_event_json_for_renderer(
    state: &ServiceState,
    renderer_location: &str,
) -> String {
    let now_playing_json = render_now_playing_json_for_renderer(state, renderer_location);
    let queue_json = render_queue_json_for_renderer(state, renderer_location);
    format!(
        r#"{{"renderer_location":"{}","now_playing":{},"queue":{}}}"#,
        json_escape(renderer_location),
        now_playing_json,
        queue_json,
    )
}

pub(crate) fn render_queue_json(state: &ServiceState, request: &HttpRequest) -> String {
    let renderer_location =
        state.preferred_renderer_location(request_value(request, "renderer_location"));
    render_queue_json_for_renderer(state, &renderer_location)
}

pub(crate) fn render_session_json(state: &ServiceState, request: &HttpRequest) -> String {
    let renderer_location =
        state.preferred_renderer_location(request_value(request, "renderer_location"));
    render_session_json_for_renderer(state, &renderer_location)
}

pub(crate) fn render_queue_json_for_renderer(
    state: &ServiceState,
    renderer_location: &str,
) -> String {
    let session = state.playback_session(renderer_location);
    let session_json =
        |session: PlaybackSession| render_session_payload_json(state, renderer_location, session);
    let Some(queue) = state.queue_snapshot(renderer_location) else {
        let session_json = session.map_or_else(|| "null".to_string(), session_json);
        return format!(
            r#"{{"renderer_location":"{}","status":"empty","entries":[],"session":{}}}"#,
            json_escape(renderer_location),
            session_json,
        );
    };

    let entries = queue
        .entries
        .iter()
        .map(|entry| {
            let track = state.find_track(&entry.track_id);
            let tidal_track = (entry.source_kind == "tidal")
                .then(|| {
                    entry
                        .source_ref
                        .as_deref()
                        .and_then(|value| serde_json::from_str::<TidalQueuedTrack>(value).ok())
                })
                .flatten();
            let title = track
                .as_ref()
                .map(|track| track.title.as_str())
                .or_else(|| tidal_track.as_ref().and_then(|track| track.title.as_deref()));
            let artist = track
                .as_ref()
                .map(|track| track.artist.as_str())
                .or_else(|| tidal_track.as_ref().and_then(|track| track.artist.as_deref()));
            let album = track
                .as_ref()
                .map(|track| track.album.as_str())
                .or_else(|| tidal_track.as_ref().and_then(|track| track.album.as_deref()));
            let duration_seconds = track
                .as_ref()
                .and_then(|track| track.duration_seconds)
                .or_else(|| {
                    tidal_track
                        .as_ref()
                        .and_then(|track| normalize_tidal_duration_seconds(track.duration_seconds))
                });
            format!(
                r#"{{"id":{},"position":{},"track_id":"{}","album_id":{},"source_kind":"{}","source_ref":{},"entry_status":"{}","started_unix":{},"completed_unix":{},"title":{},"artist":{},"album":{},"duration_seconds":{}}}"#,
                entry.id,
                entry.position,
                json_escape(&entry.track_id),
                option_string_json(entry.album_id.as_deref()),
                json_escape(&entry.source_kind),
                option_string_json(entry.source_ref.as_deref()),
                json_escape(&entry.entry_status),
                option_i64_json(entry.started_unix),
                option_i64_json(entry.completed_unix),
                option_string_json(title),
                option_string_json(artist),
                option_string_json(album),
                option_u64_json(duration_seconds),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let session_json = session.map_or_else(|| "null".to_string(), session_json);

    format!(
        r#"{{"renderer_location":"{}","name":"{}","status":"{}","version":{},"updated_unix":{},"current_entry_id":{},"entries":[{}],"session":{}}}"#,
        json_escape(&queue.renderer_location),
        json_escape(&queue.name),
        json_escape(&queue.status),
        queue.version,
        queue.updated_unix,
        option_i64_json(queue.current_entry_id),
        entries,
        session_json,
    )
}

pub(crate) fn render_session_json_for_renderer(
    state: &ServiceState,
    renderer_location: &str,
) -> String {
    let session_json = session_payload_json_for_renderer(state, renderer_location);
    format!(
        r#"{{"renderer_location":"{}","session":{}}}"#,
        json_escape(renderer_location),
        session_json,
    )
}

pub(crate) fn session_payload_json_for_renderer(
    state: &ServiceState,
    renderer_location: &str,
) -> String {
    state
        .playback_session(renderer_location)
        .map(|session| render_session_payload_json(state, renderer_location, session))
        .unwrap_or_else(|| "null".to_string())
}

pub(crate) fn render_session_payload_json(
    state: &ServiceState,
    renderer_location: &str,
    session: PlaybackSession,
) -> String {
    let current_track = current_track_for_renderer(state, renderer_location);
    let queued_metadata = if current_track.is_none() {
        current_queue_entry_for_renderer(state, renderer_location, session.queue_entry_id)
            .and_then(|entry| queue_entry_display_metadata(&entry))
    } else {
        None
    };
    let direct_stream = if current_track.is_none() {
        state.direct_stream_metadata(renderer_location)
    } else {
        None
    };
    format!(
        r#"{{"transport_state":"{}","queue_entry_id":{},"next_queue_entry_id":{},"current_track_uri":{},"position_seconds":{},"duration_seconds":{},"last_observed_unix":{},"server_unix":{},"last_error":{},"title":{},"artist":{},"album":{}}}"#,
        json_escape(&session.transport_state),
        option_i64_json(session.queue_entry_id),
        option_i64_json(session.next_queue_entry_id),
        option_string_json(session.current_track_uri.as_deref()),
        option_u64_json(session.position_seconds),
        option_u64_json(normalize_tidal_duration_seconds(session.duration_seconds)),
        session.last_observed_unix,
        now_unix_timestamp(),
        option_string_json(session.last_error.as_deref()),
        option_string_json(
            current_track
                .as_ref()
                .map(|track| track.title.as_str())
                .or_else(|| queued_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.title.as_deref()))
                .or_else(|| direct_stream.as_ref().map(|stream| stream.title.as_str()))
        ),
        option_string_json(
            current_track
                .as_ref()
                .map(|track| track.artist.as_str())
                .or_else(|| queued_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.artist.as_deref()))
                .or_else(|| direct_stream.as_ref().map(|_| "Internet radio"))
        ),
        option_string_json(
            current_track
                .as_ref()
                .map(|track| track.album.as_str())
                .or_else(|| queued_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.album.as_deref()))
        ),
    )
}

struct QueueEntryDisplayMetadata {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
}

fn current_queue_entry_for_renderer(
    state: &ServiceState,
    renderer_location: &str,
    session_entry_id: Option<i64>,
) -> Option<QueueEntry> {
    let queue = state.queue_snapshot(renderer_location)?;
    let queue_entry_id = session_entry_id.or(queue.current_entry_id)?;
    queue
        .entries
        .into_iter()
        .find(|entry| entry.id == queue_entry_id)
}

fn queue_entry_display_metadata(entry: &QueueEntry) -> Option<QueueEntryDisplayMetadata> {
    if entry.source_kind != "tidal" {
        return None;
    }
    let track = entry
        .source_ref
        .as_deref()
        .and_then(|value| serde_json::from_str::<TidalQueuedTrack>(value).ok())?;
    Some(QueueEntryDisplayMetadata {
        title: track.title,
        artist: track.artist,
        album: track.album,
    })
}

fn normalize_tidal_duration_seconds(value: Option<u64>) -> Option<u64> {
    let mut duration = value?;
    if duration > 100_000_000_000 {
        duration = duration.saturating_add(500_000_000) / 1_000_000_000;
    } else if duration > 100_000_000 {
        duration = duration.saturating_add(500_000) / 1_000_000;
    } else if duration > 86_400 {
        duration = duration.saturating_add(500) / 1_000;
    }
    (duration > 0 && duration <= 86_400).then_some(duration)
}

pub(crate) fn render_discovery_json(state: &ServiceState) -> String {
    let renderers =
        match discover_renderers(Duration::from_millis(state.config.discovery_timeout_ms)) {
            Ok(renderers) => renderers,
            Err(error) => {
                return format!(
                    r#"[{{"location":"","name":"Discovery failed","error":"{}"}}]"#,
                    json_escape(&error.to_string())
                );
            }
        };

    let entries = renderers
        .into_iter()
        .filter_map(|renderer| match inspect_renderer(&renderer.location) {
            Ok(details) => {
                let _ = state.remember_renderer_details(
                    &details.location,
                    &details.friendly_name,
                    details.manufacturer.as_deref(),
                    details.model_name.as_deref(),
                    Some(&details.av_transport_control_url),
                    Some(&details.capabilities),
                    None,
                );
                Some(renderer_record_json(
                    &RendererRecord {
                        location: details.location,
                        name: details.friendly_name,
                        manufacturer: details.manufacturer,
                        model_name: details.model_name,
                        av_transport_control_url: Some(details.av_transport_control_url),
                        capabilities: details.capabilities,
                        visibility: "public".to_string(),
                        owner_client_id: None,
                        last_checked_unix: now_unix_timestamp(),
                        last_reachable_unix: Some(now_unix_timestamp()),
                        last_error: None,
                        last_seen_unix: now_unix_timestamp(),
                    },
                    false,
                ))
            }
            Err(_) => None,
        })
        .collect::<Vec<_>>()
        .join(",");

    format!("[{entries}]")
}

pub(crate) fn current_track_for_renderer(
    state: &ServiceState,
    renderer_location: &str,
) -> Option<LibraryTrack> {
    let session_entry_id = state
        .playback_session(renderer_location)
        .and_then(|session| session.queue_entry_id)?;
    let queue = state.queue_snapshot(renderer_location)?;
    let queue_entry_id = queue.current_entry_id?;
    if session_entry_id != queue_entry_id {
        return None;
    }
    let entry = queue
        .entries
        .into_iter()
        .find(|entry| entry.id == queue_entry_id)?;
    state.find_track(&entry.track_id)
}

pub(crate) fn current_track_json_for_renderer(
    state: &ServiceState,
    renderer_location: &str,
) -> String {
    current_track_for_renderer(state, renderer_location)
        .map(|track| {
            let fallback_artwork_url = state
                .find_album(&track.album_id)
                .and_then(|album| album.artwork_url);
            track_summary_json(&track, fallback_artwork_url.as_deref())
        })
        .unwrap_or_else(|| "null".to_string())
}

pub(crate) fn queue_summary_json_for_renderer(
    state: &ServiceState,
    renderer_location: &str,
) -> String {
    match state.queue_snapshot(renderer_location) {
        Some(queue) => format!(
            r#"{{"status":"{}","name":"{}","entry_count":{},"current_entry_id":{},"updated_unix":{},"version":{}}}"#,
            json_escape(&queue.status),
            json_escape(&queue.name),
            queue.entries.len(),
            option_i64_json(queue.current_entry_id),
            queue.updated_unix,
            queue.version,
        ),
        None => r#"{"status":"empty","name":"","entry_count":0,"current_entry_id":null,"updated_unix":0,"version":0}"#.to_string(),
    }
}

pub(crate) fn track_summary_json(
    track: &LibraryTrack,
    fallback_album_artwork_url: Option<&str>,
) -> String {
    track_summary_json_with_likes(track, fallback_album_artwork_url, 0, false)
}

pub(crate) fn track_summary_json_with_likes(
    track: &LibraryTrack,
    fallback_album_artwork_url: Option<&str>,
    like_count: u64,
    liked_by_client: bool,
) -> String {
    let artwork_url = if track.artwork.is_some() {
        Some(format!("/artwork/track/{}", track.id))
    } else {
        fallback_album_artwork_url.map(ToString::to_string)
    };
    format!(
        r#"{{"id":"{}","album_id":"{}","title":"{}","artist":"{}","album":"{}","disc_number":{},"track_number":{},"duration_seconds":{},"artwork_url":{},"mime_type":"{}","metadata":{},"like_count":{},"liked_by_client":{}}}"#,
        json_escape(&track.id),
        json_escape(&track.album_id),
        json_escape(&track.title),
        json_escape(&track.artist),
        json_escape(&track.album),
        option_u32_json(track.disc_number),
        option_u32_json(track.track_number),
        option_u64_json(track.duration_seconds),
        option_string_json(artwork_url.as_deref()),
        json_escape(&track.mime_type),
        track_metadata_json(track),
        like_count,
        bool_json(liked_by_client),
    )
}

fn track_metadata_json(track: &LibraryTrack) -> String {
    format!(
        r#"{{"musicbrainz_release_id":{},"musicbrainz_release_group_id":{},"musicbrainz_recording_id":{},"musicbrainz_release_track_id":{},"release_date":{},"original_release_date":{},"release_country":{},"release_type":{},"genres":{}}}"#,
        option_string_json(track.metadata.musicbrainz_release_id.as_deref()),
        option_string_json(track.metadata.musicbrainz_release_group_id.as_deref()),
        option_string_json(track.metadata.musicbrainz_recording_id.as_deref()),
        option_string_json(track.metadata.musicbrainz_release_track_id.as_deref()),
        option_string_json(track.metadata.release_date.as_deref()),
        option_string_json(track.metadata.original_release_date.as_deref()),
        option_string_json(track.metadata.release_country.as_deref()),
        option_string_json(track.metadata.release_type.as_deref()),
        string_list_json(&track.metadata.genres),
    )
}

fn album_metadata_json(album: &AlbumSummary) -> String {
    format!(
        r#"{{"musicbrainz_release_id":{},"musicbrainz_release_group_id":{},"release_date":{},"original_release_date":{},"release_country":{},"release_type":{},"genres":{},"source_track_id":{}}}"#,
        option_string_json(album.metadata.musicbrainz_release_id.as_deref()),
        option_string_json(album.metadata.musicbrainz_release_group_id.as_deref()),
        option_string_json(album.metadata.release_date.as_deref()),
        option_string_json(album.metadata.original_release_date.as_deref()),
        option_string_json(album.metadata.release_country.as_deref()),
        option_string_json(album.metadata.release_type.as_deref()),
        string_list_json(&album.metadata.genres),
        option_string_json(album.metadata.source_track_id.as_deref()),
    )
}

pub(crate) fn renderer_record_json(renderer: &RendererRecord, selected: bool) -> String {
    renderer_record_json_with_access(renderer, selected, true)
}

pub(crate) fn renderer_record_json_with_access(
    renderer: &RendererRecord,
    selected: bool,
    direct_access: bool,
) -> String {
    let av_transport_actions_json = renderer
        .capabilities
        .av_transport_actions
        .as_ref()
        .map(|actions| string_list_json(actions));
    format!(
        r#"{{"location":"{}","name":"{}","manufacturer":{},"model_name":{},"av_transport_control_url":{},"capabilities":{{"av_transport_actions":{},"supports_set_next_av_transport_uri":{},"supports_pause":{},"supports_stop":{},"supports_next":{},"supports_previous":{},"supports_seek":{},"has_playlist_extension_service":{}}},"health":{{"last_checked_unix":{},"last_reachable_unix":{},"last_error":{},"reachable":{}}},"last_seen_unix":{},"selected":{},"kind":"{}","visibility":"{}","direct_access":{}}}"#,
        json_escape(&renderer.location),
        json_escape(&renderer.name),
        option_string_json(renderer.manufacturer.as_deref()),
        option_string_json(renderer.model_name.as_deref()),
        option_string_json(renderer.av_transport_control_url.as_deref()),
        option_json_fragment(av_transport_actions_json.as_deref()),
        option_bool_json(renderer.capabilities.supports_set_next_av_transport_uri()),
        option_bool_json(renderer.capabilities.supports_pause()),
        option_bool_json(renderer.capabilities.supports_stop()),
        option_bool_json(renderer.capabilities.supports_next()),
        option_bool_json(renderer.capabilities.supports_previous()),
        option_bool_json(renderer.capabilities.supports_seek()),
        option_bool_json(renderer.capabilities.has_playlist_extension_service),
        renderer.last_checked_unix,
        option_i64_json(renderer.last_reachable_unix),
        option_string_json(renderer.last_error.as_deref()),
        bool_json(renderer.last_error.is_none() && renderer.last_reachable_unix.is_some()),
        renderer.last_seen_unix,
        bool_json(selected),
        renderer_kind_name(renderer_kind_for_location(&renderer.location)),
        json_escape(&renderer.visibility),
        bool_json(direct_access),
    )
}

pub(crate) fn renderer_group_json(group: &RendererGroup) -> String {
    let location = renderer_group_queue_key(&group.id);
    format!(
        r#"{{"id":"{}","location":"{}","name":"{}","member_count":{},"members":{},"created_unix":{},"updated_unix":{}}}"#,
        json_escape(&group.id),
        json_escape(&location),
        json_escape(&group.name),
        group.members.len(),
        renderer_group_members_json(group),
        group.created_unix,
        group.updated_unix,
    )
}

pub(crate) fn renderer_group_as_renderer_json(group: &RendererGroup, selected: bool) -> String {
    let location = renderer_group_queue_key(&group.id);
    format!(
        r#"{{"location":"{}","name":"{}","manufacturer":"musicd","model_name":"Renderer Group","av_transport_control_url":null,"capabilities":{{"av_transport_actions":[],"supports_set_next_av_transport_uri":false,"supports_pause":false,"supports_stop":false,"supports_next":false,"supports_previous":false,"supports_seek":false,"has_playlist_extension_service":false}},"health":{{"last_checked_unix":{},"last_reachable_unix":{},"last_error":null,"reachable":true}},"last_seen_unix":{},"selected":{},"kind":"group","visibility":"public","direct_access":true,"group":{}}}"#,
        json_escape(&location),
        json_escape(&group.name),
        group.updated_unix,
        group.updated_unix,
        group.updated_unix,
        bool_json(selected),
        renderer_group_json(group),
    )
}

fn renderer_group_members_json(group: &RendererGroup) -> String {
    let entries = group
        .members
        .iter()
        .map(|member| {
            format!(
                r#"{{"renderer_location":"{}","position":{},"joined_unix":{}}}"#,
                json_escape(&member.renderer_location),
                member.position,
                member.joined_unix,
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{entries}]")
}
