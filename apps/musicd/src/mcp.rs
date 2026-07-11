use std::io::{self, BufRead, Write};

use musicd_core::AppConfig;
use serde_json::{Map, Value, json};

use crate::service::ServiceState;
use crate::types::{AlbumSummary, ArtistSummary, LibraryTrack, PlaybackQueue, PlaybackSession};

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "musicd";

pub(crate) fn run_stdio() -> io::Result<()> {
    let config = AppConfig::from_env();
    let state = ServiceState::load(config)?;
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle_message(&state, message),
            Err(error) => Some(error_response(
                Value::Null,
                -32700,
                "Parse error",
                Some(json!(error.to_string())),
            )),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }

    Ok(())
}

fn handle_message(state: &ServiceState, message: Value) -> Option<Value> {
    let Some(request) = message.as_object() else {
        return Some(error_response(Value::Null, -32600, "Invalid Request", None));
    };
    let id = request.get("id").cloned();
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return id.map(|id| error_response(id, -32600, "Invalid Request", None));
    };

    match method {
        "initialize" => id.map(|id| success_response(id, initialize_result(request))),
        "notifications/initialized" => None,
        "ping" => id.map(|id| success_response(id, json!({}))),
        "tools/list" => id.map(|id| success_response(id, tools_list_result())),
        "tools/call" => id.map(|id| match call_tool(state, request.get("params")) {
            Ok(result) => success_response(id, result),
            Err(error) => error_response(id, -32602, &error, None),
        }),
        _ => id.map(|id| error_response(id, -32601, "Method not found", None)),
    }
}

fn initialize_result(request: &Map<String, Value>) -> Value {
    let requested = request
        .get("params")
        .and_then(|params| params.get("protocolVersion"))
        .and_then(Value::as_str);
    let protocol_version = match requested {
        Some(MCP_PROTOCOL_VERSION) => MCP_PROTOCOL_VERSION,
        _ => MCP_PROTOCOL_VERSION,
    };

    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": SERVER_NAME,
            "title": "musicd",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": "Use these tools to search the local musicd library and control the selected musicd renderer."
    })
}

fn tools_list_result() -> Value {
    json!({
        "tools": [
            {
                "name": "list_artists",
                "title": "List Artists",
                "description": "List local library artists, optionally filtered by artist name.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "Optional case-insensitive artist-name search."},
                        "limit": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Maximum artists to return. Defaults to 50."}
                    }
                }
            },
            {
                "name": "list_albums",
                "title": "List Albums",
                "description": "List local library albums, optionally filtered by album name.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {"type": "string", "description": "Optional case-insensitive album-title search."},
                        "limit": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Maximum albums to return. Defaults to 50."}
                    }
                }
            },
            {
                "name": "get_homepage_spotlight_albums",
                "title": "Homepage Spotlight Albums",
                "description": "Return the albums currently spotlighted on the musicd homepage.",
                "inputSchema": {"type": "object", "properties": {}}
            },
            {
                "name": "clear_queue",
                "title": "Clear Queue",
                "description": "Clear the queue for a renderer. Uses the remembered or default renderer if renderer_location is omitted.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "renderer_location": {"type": "string", "description": "Renderer LOCATION URL or musicd group location."}
                    }
                }
            },
            {
                "name": "play_pause",
                "title": "Play Or Pause",
                "description": "Play, pause, or toggle playback for a renderer. Uses the remembered or default renderer if renderer_location is omitted.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "renderer_location": {"type": "string", "description": "Renderer LOCATION URL or musicd group location."},
                        "action": {"type": "string", "enum": ["play", "pause", "toggle"], "description": "Transport action. Defaults to toggle."}
                    }
                }
            },
            {
                "name": "add_album_to_queue",
                "title": "Add Album To Queue",
                "description": "Append a local library album to a renderer queue. Uses the remembered or default renderer if renderer_location is omitted.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "album_id": {"type": "string", "description": "Album id from list_albums or get_homepage_spotlight_albums."},
                        "renderer_location": {"type": "string", "description": "Renderer LOCATION URL or musicd group location."}
                    },
                    "required": ["album_id"]
                }
            },
            {
                "name": "get_currently_playing_track",
                "title": "Currently Playing Track",
                "description": "Return the current local-library track and playback session for a renderer. Uses the remembered or default renderer if renderer_location is omitted.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "renderer_location": {"type": "string", "description": "Renderer LOCATION URL or musicd group location."}
                    }
                }
            },
            {
                "name": "get_last_queued_album",
                "title": "Last Queued Album",
                "description": "Return the most recent album-source item currently present in a renderer queue. Uses the remembered or default renderer if renderer_location is omitted.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "renderer_location": {"type": "string", "description": "Renderer LOCATION URL or musicd group location."}
                    }
                }
            }
        ]
    })
}

fn call_tool(state: &ServiceState, params: Option<&Value>) -> Result<Value, String> {
    let params = params
        .and_then(Value::as_object)
        .ok_or_else(|| "tools/call params must be an object".to_string())?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "tools/call params.name must be a string".to_string())?;
    let empty = Map::new();
    let args = params
        .get("arguments")
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    let result = match name {
        "list_artists" => list_artists(state, args),
        "list_albums" => list_albums(state, args),
        "get_homepage_spotlight_albums" => get_homepage_spotlight_albums(state),
        "clear_queue" => clear_queue(state, args),
        "play_pause" => play_pause(state, args),
        "add_album_to_queue" => add_album_to_queue(state, args),
        "get_currently_playing_track" => get_currently_playing_track(state, args),
        "get_last_queued_album" => get_last_queued_album(state, args),
        other => return Err(format!("Unknown tool: {other}")),
    };

    Ok(match result {
        Ok(value) => tool_result(value, false),
        Err(error) => tool_result(json!({"error": error}), true),
    })
}

fn list_artists(state: &ServiceState, args: &Map<String, Value>) -> Result<Value, String> {
    let query = optional_string(args, "query");
    let limit = requested_limit(args, 50, 200);
    let mut artists = state.artists_snapshot().to_vec();
    artists.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    let matches = artists
        .into_iter()
        .filter(|artist| matches_query(&artist.name, query.as_deref()))
        .take(limit)
        .map(|artist| artist_json(&artist))
        .collect::<Vec<_>>();

    Ok(json!({
        "query": query,
        "limit": limit,
        "artists": matches
    }))
}

fn list_albums(state: &ServiceState, args: &Map<String, Value>) -> Result<Value, String> {
    let query = optional_string(args, "query");
    let limit = requested_limit(args, 50, 200);
    let mut albums = state.albums_snapshot().to_vec();
    albums.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.artist.to_lowercase().cmp(&b.artist.to_lowercase()))
            .then_with(|| a.id.cmp(&b.id))
    });
    let matches = albums
        .into_iter()
        .filter(|album| matches_query(&album.title, query.as_deref()))
        .take(limit)
        .map(|album| album_json(&album))
        .collect::<Vec<_>>();

    Ok(json!({
        "query": query,
        "limit": limit,
        "albums": matches
    }))
}

fn get_homepage_spotlight_albums(state: &ServiceState) -> Result<Value, String> {
    let library = state.library_snapshot();
    let mut eligible = library
        .albums
        .iter()
        .filter(|album| album.track_count > 3)
        .collect::<Vec<_>>();
    if eligible.is_empty() {
        eligible = library.albums.iter().collect();
    }
    let albums = eligible
        .into_iter()
        .take(5)
        .map(album_json)
        .collect::<Vec<_>>();

    Ok(json!({
        "albums": albums
    }))
}

fn clear_queue(state: &ServiceState, args: &Map<String, Value>) -> Result<Value, String> {
    let renderer_location = selected_renderer_location(state, args)?;
    state
        .clear_queue(&renderer_location)
        .map_err(|error| format!("queue clear failed: {error}"))?;

    Ok(json!({
        "renderer_location": renderer_location,
        "queue": queue_json(state.queue_snapshot(&renderer_location))
    }))
}

fn play_pause(state: &ServiceState, args: &Map<String, Value>) -> Result<Value, String> {
    let renderer_location = selected_renderer_location(state, args)?;
    let action = optional_string(args, "action").unwrap_or_else(|| "toggle".to_string());
    let normalized = action.trim().to_ascii_lowercase();
    let should_pause = match normalized.as_str() {
        "pause" => true,
        "play" => false,
        "toggle" => state
            .playback_session(&renderer_location)
            .map(|session| session.transport_state == "PLAYING")
            .unwrap_or(false),
        _ => return Err("action must be one of: play, pause, toggle".to_string()),
    };
    let message = if should_pause {
        state
            .pause_renderer(&renderer_location)
            .map_err(|error| format!("pause failed: {error}"))?
    } else {
        state
            .resume_renderer(&renderer_location)
            .map_err(|error| format!("play failed: {error}"))?
    };

    Ok(json!({
        "renderer_location": renderer_location,
        "action": if should_pause { "pause" } else { "play" },
        "message": message,
        "session": session_json(state.playback_session(&renderer_location)),
        "queue": queue_json(state.queue_snapshot(&renderer_location))
    }))
}

fn add_album_to_queue(state: &ServiceState, args: &Map<String, Value>) -> Result<Value, String> {
    let renderer_location = selected_renderer_location(state, args)?;
    let album_id = required_string(args, "album_id")?;
    let album = state
        .find_album(&album_id)
        .ok_or_else(|| format!("album not found: {album_id}"))?;
    let queue = state
        .append_album_to_queue(&renderer_location, &album)
        .map_err(|error| format!("queue update failed: {error}"))?;

    Ok(json!({
        "renderer_location": renderer_location,
        "album": album_json(&album),
        "queue": queue_json(Some(queue))
    }))
}

fn get_currently_playing_track(
    state: &ServiceState,
    args: &Map<String, Value>,
) -> Result<Value, String> {
    let renderer_location = selected_renderer_location(state, args)?;
    let track = current_track_for_renderer(state, &renderer_location);

    Ok(json!({
        "renderer_location": renderer_location,
        "track": track.as_ref().map(track_json),
        "session": session_json(state.playback_session(&renderer_location)),
        "queue": queue_json(state.queue_snapshot(&renderer_location))
    }))
}

fn get_last_queued_album(state: &ServiceState, args: &Map<String, Value>) -> Result<Value, String> {
    let renderer_location = selected_renderer_location(state, args)?;
    let queue = state.queue_snapshot(&renderer_location);
    let last_entry = queue
        .as_ref()
        .and_then(|queue| {
            queue
                .entries
                .iter()
                .filter(|entry| entry.source_kind == "album")
                .max_by_key(|entry| (entry.position, entry.id))
        })
        .cloned();
    let album_id = last_entry
        .as_ref()
        .and_then(|entry| entry.source_ref.as_deref().or(entry.album_id.as_deref()));
    let album = album_id.and_then(|album_id| state.find_album(album_id));
    let queued_track_count = album_id
        .and_then(|album_id| {
            queue.as_ref().map(|queue| {
                queue
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry.source_kind == "album"
                            && entry.source_ref.as_deref().or(entry.album_id.as_deref())
                                == Some(album_id)
                    })
                    .count()
            })
        })
        .unwrap_or(0);

    Ok(json!({
        "renderer_location": renderer_location,
        "album": album.as_ref().map(album_json),
        "album_id": album_id,
        "queued_track_count": queued_track_count,
        "queue_entry": last_entry.map(|entry| {
            json!({
                "id": entry.id,
                "position": entry.position,
                "track_id": entry.track_id,
                "album_id": entry.album_id,
                "source_kind": entry.source_kind,
                "source_ref": entry.source_ref,
                "entry_status": entry.entry_status
            })
        }),
        "queue": queue_json(queue)
    }))
}

fn current_track_for_renderer(
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

fn selected_renderer_location(
    state: &ServiceState,
    args: &Map<String, Value>,
) -> Result<String, String> {
    let requested = optional_string(args, "renderer_location");
    let renderer_location = state.preferred_renderer_location(requested.as_deref());
    if renderer_location.trim().is_empty() {
        return Err(
            "renderer_location is required unless a default or remembered renderer is configured"
                .to_string(),
        );
    }
    if requested.is_some() {
        let _ = state.remember_renderer_location(&renderer_location);
    }
    Ok(renderer_location)
}

fn artist_json(artist: &ArtistSummary) -> Value {
    json!({
        "id": artist.id,
        "name": artist.name,
        "album_count": artist.album_count,
        "track_count": artist.track_count,
        "artwork_url": artist.artwork_url,
        "first_album_id": artist.first_album_id
    })
}

fn album_json(album: &AlbumSummary) -> Value {
    json!({
        "id": album.id,
        "artist_id": album.artist_id,
        "title": album.title,
        "artist": album.artist,
        "track_count": album.track_count,
        "artwork_url": album.artwork_url,
        "first_track_id": album.first_track_id,
        "metadata": album.metadata
    })
}

fn track_json(track: &LibraryTrack) -> Value {
    json!({
        "id": track.id,
        "album_id": track.album_id,
        "title": track.title,
        "artist": track.artist,
        "album": track.album,
        "album_artist": track.album_artist,
        "disc_number": track.disc_number,
        "track_number": track.track_number,
        "duration_seconds": track.duration_seconds,
        "relative_path": track.relative_path,
        "mime_type": track.mime_type,
        "file_size": track.file_size
    })
}

fn queue_json(queue: Option<PlaybackQueue>) -> Value {
    match queue {
        Some(queue) => json!({
            "renderer_location": queue.renderer_location,
            "name": queue.name,
            "status": queue.status,
            "version": queue.version,
            "updated_unix": queue.updated_unix,
            "current_entry_id": queue.current_entry_id,
            "entry_count": queue.entries.len()
        }),
        None => json!({
            "status": "empty",
            "entry_count": 0
        }),
    }
}

fn session_json(session: Option<PlaybackSession>) -> Value {
    match session {
        Some(session) => json!({
            "renderer_location": session.renderer_location,
            "queue_entry_id": session.queue_entry_id,
            "next_queue_entry_id": session.next_queue_entry_id,
            "transport_state": session.transport_state,
            "current_track_uri": session.current_track_uri,
            "position_seconds": session.position_seconds,
            "duration_seconds": session.duration_seconds,
            "last_observed_unix": session.last_observed_unix,
            "last_error": session.last_error
        }),
        None => Value::Null,
    }
}

fn optional_string(args: &Map<String, Value>, name: &str) -> Option<String> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn required_string(args: &Map<String, Value>, name: &str) -> Result<String, String> {
    optional_string(args, name).ok_or_else(|| format!("{name} is required"))
}

fn requested_limit(args: &Map<String, Value>, default: usize, max: usize) -> usize {
    args.get("limit")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
        .min(max)
}

fn matches_query(value: &str, query: Option<&str>) -> bool {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return true;
    };
    value.to_lowercase().contains(&query.to_lowercase())
}

fn tool_result(structured: Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(&structured).unwrap_or_else(|_| structured.to_string());
    json!({
        "content": [
            {
                "type": "text",
                "text": text
            }
        ],
        "structuredContent": structured,
        "isError": is_error
    })
}

fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn error_response(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = Map::new();
    error.insert("code".to_string(), json!(code));
    error.insert("message".to_string(), json!(message));
    if let Some(data) = data {
        error.insert("data".to_string(), data);
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": Value::Object(error)
    })
}
