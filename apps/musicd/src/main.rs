use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use std::thread;
use std::time::Duration;

use musicd_core::AppConfig;
use musicd_upnp::{StreamResource, discover_renderers, inspect_renderer, play_stream};

mod artwork;
mod db;
mod http;
mod ids;
mod library;
mod metrics;
mod renderer;
mod service;
mod types;
mod util;

pub(crate) use crate::http::*;
pub(crate) use crate::library::*;
pub(crate) use crate::renderer::*;
pub(crate) use crate::service::*;
pub(crate) use crate::types::*;
pub(crate) use crate::util::*;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None => {
            print_help();
            Ok(())
        }
        Some("serve") => run_serve(),
        Some("discover") => {
            let timeout_ms = args
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1500);
            run_discover(Duration::from_millis(timeout_ms))
        }
        Some("inspect") => {
            let location = required_arg(args.next(), "location URL")?;
            run_inspect(&location)
        }
        Some("play-url") => {
            let renderer_location = required_arg(args.next(), "renderer location URL")?;
            let stream_url = required_arg(args.next(), "stream URL")?;
            let title = args.next().unwrap_or_else(|| "musicd track".to_string());
            run_play_url(&renderer_location, &stream_url, &title)
        }
        Some("serve-file") => {
            let file_path = PathBuf::from(required_arg(args.next(), "audio file path")?);
            let bind_address = args.next().unwrap_or_else(|| "0.0.0.0:7878".to_string());
            run_serve_file(file_path, &bind_address)
        }
        Some("play-file") => {
            let renderer_location = required_arg(args.next(), "renderer location URL")?;
            let file_path = PathBuf::from(required_arg(args.next(), "audio file path")?);
            let bind_address = required_arg(args.next(), "bind address")?;
            let public_base_url = required_arg(args.next(), "public base URL")?;
            let title = args.next().unwrap_or_else(|| inferred_title(&file_path));
            run_play_file(
                &renderer_location,
                file_path,
                &bind_address,
                &public_base_url,
                &title,
            )
        }
        Some("status") => {
            print_status();
            Ok(())
        }
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some(other) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown command: {other}"),
        )),
    }
}

fn run_serve() -> io::Result<()> {
    let config = AppConfig::from_env();
    let state = Arc::new(ServiceState::load(config.clone())?);
    state.install_metrics(Arc::new(metrics::Metrics::new(Arc::downgrade(&state))));
    let track_count = state.track_count();

    spawn_queue_worker(Arc::clone(&state));

    println!("{} service", config.instance_name);
    println!("Library path: {}", config.library_path.display());
    println!("Config path: {}", config.config_path.display());
    println!("Bind address: {}", config.bind_address);
    println!("HTTP base URL: {}", config.resolved_base_url());
    println!("Instance name: {}", config.instance_name);
    println!("Indexed tracks: {track_count}");
    if let Some(renderer) = &config.default_renderer_location {
        println!("Default renderer: {renderer}");
    }
    println!(
        "Debug mode: {}",
        if config.debug_mode {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "Open {}/ in a browser to browse and play music.",
        config.resolved_base_url()
    );

    serve_tcp(&config.bind_address, ServerMode::Service(state))
}


fn run_discover(timeout: Duration) -> io::Result<()> {
    let renderers = discover_renderers(timeout)?;
    if renderers.is_empty() {
        println!(
            "No UPnP media renderers discovered within {}ms.",
            timeout.as_millis()
        );
        return Ok(());
    }

    for renderer in renderers {
        println!("Location: {}", renderer.location);
        if let Some(server) = renderer.server {
            println!("Server: {server}");
        }
        if let Some(search_target) = renderer.search_target {
            println!("ST: {search_target}");
        }
        if let Some(usn) = renderer.usn {
            println!("USN: {usn}");
        }
        println!();
    }

    Ok(())
}

fn run_inspect(location: &str) -> io::Result<()> {
    let renderer = inspect_renderer(location)?;
    print!("{renderer}");
    Ok(())
}

fn run_play_url(renderer_location: &str, stream_url: &str, title: &str) -> io::Result<()> {
    let resource = StreamResource {
        stream_url: stream_url.to_string(),
        mime_type: infer_mime_type(Path::new(stream_url)).to_string(),
        title: title.to_string(),
        album_art_url: None,
    };

    let renderer = play_stream(renderer_location, &resource)?;
    println!(
        "Playback started on '{}' using {}",
        renderer.friendly_name, resource.stream_url
    );
    Ok(())
}

fn run_serve_file(file_path: PathBuf, bind_address: &str) -> io::Result<()> {
    let path = Arc::new(file_path);
    let title = inferred_title(path.as_path());

    println!(
        "Serving '{}' on http://{bind_address}/stream/current",
        path.display()
    );
    println!("Track title: {title}");
    println!("Press Ctrl+C to stop.");

    serve_tcp(bind_address, ServerMode::SingleFile(path))
}

fn run_play_file(
    renderer_location: &str,
    file_path: PathBuf,
    bind_address: &str,
    public_base_url: &str,
    title: &str,
) -> io::Result<()> {
    let path = Arc::new(file_path);
    let bind_address = bind_address.to_string();
    let server_path = Arc::clone(&path);
    let listener_address = bind_address.clone();

    thread::spawn(move || {
        if let Err(error) = serve_tcp(&listener_address, ServerMode::SingleFile(server_path)) {
            eprintln!("stream server stopped: {error}");
        }
    });

    thread::sleep(Duration::from_millis(200));

    let stream_url = format!("{}/stream/current", public_base_url.trim_end_matches('/'));
    let resource = StreamResource {
        stream_url: stream_url.clone(),
        mime_type: infer_mime_type(path.as_path()).to_string(),
        title: title.to_string(),
        album_art_url: None,
    };

    let renderer = play_stream(renderer_location, &resource)?;

    println!("Serving '{}'", path.display());
    println!("Playback started on '{}'", renderer.friendly_name);
    println!("Renderer location: {}", renderer.location);
    println!("HTTP stream URL: {stream_url}");
    println!("Listening on: {bind_address}");
    println!("Press Ctrl+C to keep serving while the renderer plays.");

    loop {
        thread::park();
    }
}

fn handle_service_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: Arc<ServiceState>,
) -> io::Result<()> {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") | ("HEAD", "/") => {
            let body = render_home_page(&state, request);
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
            let body = render_tracks_json(&state);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/albums") | ("HEAD", "/api/albums") => {
            let body = render_albums_json(&state);
            respond_text(
                writer,
                "200 OK",
                "application/json; charset=utf-8",
                body.as_bytes(),
                request.method == "HEAD",
            )
        }
        ("GET", "/api/renderers") | ("HEAD", "/api/renderers") => {
            let body = render_renderers_json(&state);
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
            let body = render_queue_json(&state, request);
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
        ("POST", "/api/renderers/register-android-local") => {
            handle_api_register_android_local_renderer_request(writer, request, &state)
        }
        ("POST", "/api/renderers/android-local/session") => {
            handle_api_android_local_session_request(writer, request, &state)
        }
        ("POST", "/api/renderers/android-local/completed") => {
            handle_api_android_local_completed_request(writer, request, &state)
        }
        ("POST", "/api/play") => handle_api_play_request(writer, request, &state),
        ("POST", "/api/play-album") => handle_api_play_album_request(writer, request, &state),
        ("POST", "/api/albums/artwork/select") => {
            handle_api_album_artwork_select_request(writer, request, &state)
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
        ("HEAD", "/play")
        | ("HEAD", "/play-album")
        | ("HEAD", "/api/play")
        | ("HEAD", "/api/play-album")
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

fn handle_play_request(
    writer: &mut TcpStream,
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

fn handle_play_album_request(
    writer: &mut TcpStream,
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

fn handle_queue_append_track_request(
    writer: &mut TcpStream,
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

fn handle_queue_play_next_track_request(
    writer: &mut TcpStream,
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

fn handle_transport_play_request(
    writer: &mut TcpStream,
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

fn handle_transport_pause_request(
    writer: &mut TcpStream,
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

fn handle_transport_stop_request(
    writer: &mut TcpStream,
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

fn handle_transport_next_request(
    writer: &mut TcpStream,
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

fn handle_transport_previous_request(
    writer: &mut TcpStream,
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

fn handle_queue_append_album_request(
    writer: &mut TcpStream,
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

fn handle_queue_play_next_album_request(
    writer: &mut TcpStream,
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

fn handle_queue_move_up_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_queue_entry_mutation_request(writer, request, state, "move up", |state, renderer, id| {
        state.move_queue_entry_up(renderer, id)
    })
}

fn handle_queue_move_down_request(
    writer: &mut TcpStream,
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

fn handle_queue_remove_entry_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_queue_entry_mutation_request(writer, request, state, "remove", |state, renderer, id| {
        state.remove_pending_queue_entry(renderer, id)
    })
}

fn handle_queue_entry_mutation_request(
    writer: &mut TcpStream,
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

fn handle_queue_clear_request(
    writer: &mut TcpStream,
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

fn handle_rescan_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = request.query.get("renderer_location").map(String::as_str);
    match state.rescan() {
        Ok(track_count) => redirect_home(
            writer,
            renderer_location,
            Some(&format!(
                "Library rescan complete. Indexed {track_count} tracks."
            )),
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

fn handle_api_renderer_discover_request(
    writer: &mut TcpStream,
    _request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let body = format!(
        r#"{{"ok":true,"renderers":{}}}"#,
        render_discovery_json(state)
    );
    respond_json(writer, "200 OK", &body)
}

fn handle_api_play_request(
    writer: &mut TcpStream,
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

fn handle_api_play_album_request(
    writer: &mut TcpStream,
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

fn handle_api_album_artwork_select_request(
    writer: &mut TcpStream,
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

fn handle_api_events_request(
    writer: &mut TcpStream,
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

    respond_sse_stream(writer, state, &renderer_location)
}

fn handle_api_register_android_local_renderer_request(
    writer: &mut TcpStream,
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
    let capabilities = android_local_renderer_capabilities();

    match state.remember_renderer_details(
        &renderer_location,
        &name,
        manufacturer.as_deref(),
        model_name.as_deref(),
        None,
        Some(&capabilities),
        None,
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

fn handle_api_android_local_session_request(
    writer: &mut TcpStream,
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

    let transport_state = match required_request_value(request, "transport_state") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    let current_track_uri = request_value(request, "current_track_uri");
    let position_seconds = request_value(request, "position_seconds").and_then(|value| value.parse::<u64>().ok());
    let duration_seconds = request_value(request, "duration_seconds").and_then(|value| value.parse::<u64>().ok());
    let renderer = match state.resolve_renderer(&renderer_location) {
        Ok(renderer) => renderer,
        Err(error) => {
            return api_error(
                writer,
                "500 Internal Server Error",
                &format!("failed to resolve renderer: {error}"),
            )
        }
    };
    let _ = state.mark_renderer_reachable(&renderer);
    let _ = state.database.record_transport_snapshot(
        &renderer_location,
        &transport_state,
        current_track_uri.as_deref(),
        position_seconds,
        duration_seconds,
    );
    let _ = state.database.sync_queue_status(
        &renderer_location,
        queue_status_for_transport(&transport_state),
    );

    api_renderer_state_response(writer, state, &renderer_location, "Session updated.")
}

fn handle_api_android_local_completed_request(
    writer: &mut TcpStream,
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

    match state.database.advance_queue_after_completion(&renderer_location) {
        Ok(next_entry_id) => {
            if next_entry_id.is_some() {
                if let Err(error) = state.start_current_queue_entry(&renderer_location) {
                    return api_error(
                        writer,
                        "500 Internal Server Error",
                        &format!("failed to start next queue entry: {error}"),
                    );
                }
                api_renderer_state_response(
                    writer,
                    state,
                    &renderer_location,
                    "Advanced to the next local queue entry.",
                )
            } else {
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

fn handle_api_transport_play_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_transport_action(writer, request, state, |state, renderer| {
        state.resume_renderer(renderer)
    })
}

fn handle_api_transport_pause_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_transport_action(writer, request, state, |state, renderer| {
        state.pause_renderer(renderer)
    })
}

fn handle_api_transport_stop_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_transport_action(writer, request, state, |state, renderer| {
        state.stop_renderer(renderer)
    })
}

fn handle_api_transport_next_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_transport_action(writer, request, state, |state, renderer| {
        state.skip_to_next(renderer)
    })
}

fn handle_api_transport_previous_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    handle_api_transport_action(writer, request, state, |state, renderer| {
        state.skip_to_previous(renderer)
    })
}

fn handle_api_transport_action(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: &ServiceState,
    apply: impl Fn(&ServiceState, &str) -> io::Result<String>,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    match apply(state, &renderer_location) {
        Ok(message) => api_renderer_state_response(writer, state, &renderer_location, &message),
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("transport action failed: {error}"),
        ),
    }
}

fn handle_api_queue_append_track_request(
    writer: &mut TcpStream,
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

fn handle_api_queue_play_next_track_request(
    writer: &mut TcpStream,
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

fn handle_api_queue_append_album_request(
    writer: &mut TcpStream,
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

fn handle_api_queue_play_next_album_request(
    writer: &mut TcpStream,
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

fn handle_api_queue_track_action(
    writer: &mut TcpStream,
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

fn handle_api_queue_album_action(
    writer: &mut TcpStream,
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

fn handle_api_queue_move_request(
    writer: &mut TcpStream,
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

fn handle_api_queue_remove_request(
    writer: &mut TcpStream,
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

fn handle_api_queue_clear_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let renderer_location = match required_request_value(request, "renderer_location") {
        Ok(value) => value,
        Err(error) => return api_error(writer, "400 Bad Request", error),
    };
    match state.clear_queue(&renderer_location) {
        Ok(()) => api_renderer_state_response(writer, state, &renderer_location, "Queue cleared."),
        Err(error) => api_error(
            writer,
            "500 Internal Server Error",
            &format!("queue clear failed: {error}"),
        ),
    }
}

fn required_request_value(request: &HttpRequest, key: &str) -> Result<String, &'static str> {
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

fn api_queue_response(
    writer: &mut TcpStream,
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

fn api_renderer_state_response(
    writer: &mut TcpStream,
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

fn handle_track_stream_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    state: &ServiceState,
) -> io::Result<()> {
    let track_id = request.path.trim_start_matches("/stream/track/");
    let Some(track) = state.find_track(track_id) else {
        return respond_not_found(writer, request.method == "HEAD");
    };

    respond_with_file(
        writer,
        &track.path,
        request.method == "HEAD",
        request.range_header.clone(),
    )
}

fn handle_track_artwork_request(
    writer: &mut TcpStream,
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

fn handle_album_artwork_request(
    writer: &mut TcpStream,
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

fn respond_sse_stream(
    writer: &mut TcpStream,
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

    let mut last_payload = String::new();
    let mut heartbeat_tick = 0usize;

    loop {
        let payload = render_playback_event_json_for_renderer(state, renderer_location);
        if payload != last_payload {
            write_sse_event(writer, "playback", &payload)?;
            last_payload = payload;
        } else if heartbeat_tick >= 14 {
            write_sse_comment(writer, "ping")?;
            heartbeat_tick = 0;
        }

        thread::sleep(Duration::from_secs(1));
        heartbeat_tick += 1;
    }
}

fn directory_metrics(path: &Path) -> io::Result<(u64, u64)> {
    if !path.exists() {
        return Ok((0, 0));
    }

    let mut file_count = 0_u64;
    let mut total_bytes = 0_u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            file_count += 1;
            total_bytes += metadata.len();
        }
    }
    Ok((file_count, total_bytes))
}

fn render_home_page(state: &ServiceState, request: &HttpRequest) -> String {
    let library = state.library_snapshot();
    let tracks = &library.tracks;
    let albums = &library.albums;
    let library_path = state.config.library_path.display().to_string();
    let renderer_location = state
        .preferred_renderer_location(request.query.get("renderer_location").map(String::as_str));
    let queue_html = render_queue_panel(state, &renderer_location, &library);
    let known_renderers = state.renderer_snapshot();
    let message = request.query.get("message").cloned().unwrap_or_default();
    let error = request.query.get("error").cloned().unwrap_or_default();

    let mut album_rows = String::new();
    for album in albums.iter() {
        let search_text = format!("{} {}", album.title, album.artist).to_ascii_lowercase();
        let cover_html = album
            .artwork_url
            .as_ref()
            .map(|artwork_url| {
                format!(
                    "<img class=\"cover-thumb\" src=\"{}\" alt=\"Artwork for {}\">",
                    html_escape(artwork_url),
                    html_escape(&album.title)
                )
            })
            .unwrap_or_else(|| "<div class=\"cover-thumb placeholder\">No Art</div>".to_string());
        let album_url = format!(
            "/album/{}?renderer_location={}",
            url_encode(&album.id),
            url_encode(&renderer_location)
        );
        album_rows.push_str(&format!(
            "<tr data-search=\"{}\"><td data-label=\"Cover\">{}</td><td data-label=\"Album\"><a class=\"album-link\" href=\"{}\">{}</a></td><td data-label=\"Artist\">{}</td><td data-label=\"Tracks\">{}</td><td data-label=\"Actions\" class=\"actions-cell\"><form class=\"inline-form\" action=\"/play-album\" method=\"get\"><input type=\"hidden\" name=\"album_id\" value=\"{}\"><input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\"><button type=\"submit\" class=\"secondary\">Play Album</button></form> <span class=\"muted-sep\">|</span> <form class=\"inline-form\" action=\"/queue/play-next-album\" method=\"get\"><input type=\"hidden\" name=\"album_id\" value=\"{}\"><input type=\"hidden\" name=\"return_to\" value=\"/\"><input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\"><button type=\"submit\" class=\"secondary\">Play Next</button></form> <span class=\"muted-sep\">|</span> <form class=\"inline-form\" action=\"/queue/append-album\" method=\"get\"><input type=\"hidden\" name=\"album_id\" value=\"{}\"><input type=\"hidden\" name=\"return_to\" value=\"/\"><input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\"><button type=\"submit\" class=\"secondary\">Queue Album</button></form> <span class=\"muted-sep\">|</span> <a href=\"{}\">View Album</a></td></tr>",
            html_escape(&search_text),
            cover_html,
            html_escape(&album_url),
            html_escape(&album.title),
            html_escape(&album.artist),
            album.track_count,
            html_escape(&album.id),
            html_escape(&renderer_location),
            html_escape(&album.id),
            html_escape(&renderer_location),
            html_escape(&album.id),
            html_escape(&renderer_location),
            html_escape(&album_url),
        ));
    }

    let mut rows = String::new();
    for track in tracks.iter() {
        let search_text = format!(
            "{} {} {} {}",
            track.title, track.artist, track.album, track.relative_path
        )
        .to_ascii_lowercase();
        let cover_html = track
            .artwork
            .as_ref()
            .map(|_| {
                format!(
                    "<img class=\"cover-thumb\" src=\"/artwork/track/{}\" alt=\"Artwork for {}\">",
                    html_escape(&track.id),
                    html_escape(&track.album)
                )
            })
            .unwrap_or_else(|| "<div class=\"cover-thumb placeholder\">No Art</div>".to_string());
        rows.push_str(&format!(
            "<tr data-search=\"{}\"><td data-label=\"Play\"><input type=\"radio\" form=\"playback_form\" name=\"track_id\" value=\"{}\"></td><td data-label=\"Cover\">{}</td><td data-label=\"Title\">{}</td><td data-label=\"Artist\">{}</td><td data-label=\"Album\">{}</td><td data-label=\"Actions\" class=\"actions-cell\"><form class=\"inline-form\" action=\"/queue/play-next-track\" method=\"get\"><input type=\"hidden\" name=\"track_id\" value=\"{}\"><input type=\"hidden\" name=\"return_to\" value=\"/\"><input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\"><button type=\"submit\" class=\"secondary\">Play Next</button></form> <span class=\"muted-sep\">|</span> <form class=\"inline-form\" action=\"/queue/append-track\" method=\"get\"><input type=\"hidden\" name=\"track_id\" value=\"{}\"><input type=\"hidden\" name=\"return_to\" value=\"/\"><input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\"><button type=\"submit\" class=\"secondary\">Queue</button></form> <span class=\"muted-sep\">|</span> <a href=\"/stream/track/{}\" target=\"_blank\" rel=\"noreferrer\">Preview</a> <span class=\"muted-sep\">|</span> <a href=\"/track/{}\" target=\"_blank\" rel=\"noreferrer\">Inspect</a></td></tr>",
            html_escape(&search_text),
            html_escape(&track.id),
            cover_html,
            html_escape(&track.title),
            html_escape(&track.artist),
            format!(
                "<a class=\"album-link\" href=\"/album/{}?renderer_location={}\">{}</a>",
                url_encode(&track.album_id),
                url_encode(&renderer_location),
                html_escape(&track.album)
            ),
            html_escape(&track.id),
            html_escape(&renderer_location),
            html_escape(&track.id),
            html_escape(&renderer_location),
            html_escape(&track.id),
            html_escape(&track.id),
        ));
    }

    let empty_state = if tracks.is_empty() {
        "<p class=\"empty\">No supported audio files were found under the library path. Add music or verify the volume mapping, then rescan.</p>"
    } else {
        ""
    };

    let message_html = if message.is_empty() {
        String::new()
    } else {
        format!("<p class=\"banner success\">{}</p>", html_escape(&message))
    };
    let error_html = if error.is_empty() {
        String::new()
    } else {
        format!("<p class=\"banner error\">{}</p>", html_escape(&error))
    };
    let album_empty_state = if albums.is_empty() {
        "<p class=\"empty\">No albums have been grouped yet. Add music or rescan the library.</p>"
    } else {
        ""
    };
    let renderer_options = if known_renderers.is_empty() {
        "<option value=\"\">Discovered renderers appear here</option>".to_string()
    } else {
        known_renderers
            .iter()
            .map(|renderer| {
                let selected = if renderer.location == renderer_location {
                    " selected"
                } else {
                    ""
                };
                format!(
                    "<option value=\"{}\"{}>{}</option>",
                    html_escape(&renderer.location),
                    selected,
                    html_escape(&renderer.name)
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{}</title>
  <style>
    :root {{
      color-scheme: light;
      --bg: #f5f1e8;
      --panel: rgba(255, 252, 245, 0.92);
      --ink: #1f1a17;
      --muted: #6f665f;
      --accent: #166534;
      --accent-2: #b45309;
      --line: rgba(31, 26, 23, 0.12);
      --danger: #991b1b;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      font-family: Georgia, "Iowan Old Style", serif;
      color: var(--ink);
      background:
        radial-gradient(circle at top left, rgba(22, 101, 52, 0.18), transparent 28rem),
        linear-gradient(160deg, #f8f4eb 0%, #efe5d4 55%, #f7f1e6 100%);
      min-height: 100vh;
    }}
    main {{
      width: min(1100px, calc(100vw - 2rem));
      margin: 2rem auto 3rem;
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 24px;
      box-shadow: 0 24px 60px rgba(31, 26, 23, 0.12);
      overflow: hidden;
    }}
    header {{
      padding: 2rem;
      border-bottom: 1px solid var(--line);
      background: linear-gradient(135deg, rgba(22, 101, 52, 0.12), rgba(180, 83, 9, 0.08));
    }}
    h1 {{
      margin: 0 0 0.4rem;
      font-size: clamp(2rem, 4vw, 3rem);
      line-height: 1;
    }}
    p {{
      margin: 0.25rem 0;
    }}
    .meta {{
      color: var(--muted);
      font-size: 0.98rem;
    }}
    .banner {{
      margin: 1rem 2rem 0;
      padding: 0.9rem 1rem;
      border-radius: 14px;
      font-size: 0.96rem;
    }}
    .banner.success {{
      background: rgba(22, 101, 52, 0.1);
      color: var(--accent);
    }}
    .banner.error {{
      background: rgba(153, 27, 27, 0.08);
      color: var(--danger);
    }}
    .controls {{
      padding: 1.5rem 2rem 0.5rem;
      display: grid;
      gap: 1rem;
    }}
    .control-row {{
      display: flex;
      gap: 0.75rem;
      flex-wrap: wrap;
      align-items: center;
    }}
    label {{
      font-weight: 600;
      min-width: 8rem;
    }}
    input[type="text"] {{
      flex: 1 1 24rem;
      min-width: 18rem;
      border: 1px solid var(--line);
      background: #fffdfa;
      border-radius: 12px;
      padding: 0.8rem 0.9rem;
      font: inherit;
    }}
    button, .button-link {{
      border: 0;
      border-radius: 999px;
      padding: 0.75rem 1.05rem;
      font: inherit;
      cursor: pointer;
      background: var(--ink);
      color: white;
      text-decoration: none;
    }}
    button.secondary {{
      background: #e8dcc9;
      color: var(--ink);
    }}
    select {{
      min-width: 20rem;
      border: 1px solid var(--line);
      background: #fffdfa;
      border-radius: 12px;
      padding: 0.8rem 0.9rem;
      font: inherit;
    }}
    .table-wrap {{
      padding: 0 1rem 1.5rem;
      overflow-x: auto;
    }}
    .library-grid {{
      display: grid;
      grid-template-columns: minmax(0, 1.8fr) minmax(18rem, 0.95fr);
      gap: 1rem;
      align-items: start;
      padding: 0 1rem 1.5rem;
    }}
    .library-grid .table-wrap {{
      padding: 0;
      overflow-x: auto;
    }}
    .track-sidebar {{
      position: sticky;
      top: 1rem;
      border: 1px solid var(--line);
      border-radius: 18px;
      background: rgba(255, 255, 255, 0.74);
      padding: 1rem;
      box-shadow: 0 10px 24px rgba(31, 26, 23, 0.08);
    }}
    .track-sidebar h3 {{
      margin: 0 0 0.35rem;
      font-size: 1.15rem;
    }}
    .track-sidebar p {{
      color: var(--muted);
    }}
    .track-sidebar .sidebar-artwork {{
      width: 100%;
      aspect-ratio: 1;
      object-fit: cover;
      border-radius: 16px;
      border: 1px solid var(--line);
      background: rgba(31, 26, 23, 0.06);
      margin-bottom: 0.9rem;
    }}
    .track-sidebar .sidebar-artwork.placeholder {{
      display: flex;
      align-items: center;
      justify-content: center;
      color: var(--muted);
      font-size: 0.82rem;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }}
    .track-sidebar-meta,
    .track-sidebar-tags {{
      display: grid;
      gap: 0.6rem;
      margin-top: 0.85rem;
    }}
    .track-sidebar-meta-row,
    .track-sidebar-tag {{
      display: grid;
      gap: 0.22rem;
      padding-top: 0.6rem;
      border-top: 1px solid var(--line);
    }}
    .track-sidebar-label {{
      color: var(--muted);
      font-size: 0.78rem;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }}
    .track-sidebar-value,
    .track-sidebar-tag-value {{
      word-break: break-word;
      font-size: 0.95rem;
    }}
    .track-sidebar-tag-value code,
    .track-sidebar-meta-row code {{
      font-family: "SFMono-Regular", Menlo, monospace;
      font-size: 0.92em;
    }}
    .track-sidebar-actions {{
      display: flex;
      gap: 0.6rem;
      flex-wrap: wrap;
      margin-top: 1rem;
    }}
    .track-sidebar-note-list {{
      margin: 0.75rem 0 0;
      padding-left: 1.1rem;
      color: var(--muted);
    }}
    .section-heading {{
      margin: 0;
      padding: 0 2rem 1rem;
      font-size: 1.3rem;
    }}
    .section-note {{
      margin: 0;
      padding: 0 2rem 1rem;
      color: var(--muted);
      font-size: 0.95rem;
    }}
    table {{
      width: 100%;
      border-collapse: collapse;
      margin-top: 0.5rem;
    }}
    th, td {{
      padding: 0.9rem 1rem;
      border-top: 1px solid var(--line);
      text-align: left;
      vertical-align: top;
    }}
    thead th {{
      color: var(--muted);
      font-size: 0.92rem;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }}
    tbody tr:hover {{
      background: rgba(22, 101, 52, 0.04);
    }}
    .empty {{
      margin: 1rem 2rem 2rem;
      padding: 1rem 1.1rem;
      background: rgba(180, 83, 9, 0.1);
      border-radius: 16px;
      color: #7c4210;
    }}
    .muted-sep {{
      color: var(--muted);
      margin: 0 0.2rem;
    }}
    .cover-thumb {{
      width: 3rem;
      height: 3rem;
      display: block;
      border-radius: 12px;
      object-fit: cover;
      background: rgba(31, 26, 23, 0.08);
      border: 1px solid var(--line);
    }}
    .cover-thumb.placeholder {{
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 0.68rem;
      color: var(--muted);
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }}
    .album-link {{
      color: inherit;
      text-decoration-thickness: 1px;
      text-underline-offset: 0.15em;
    }}
    .inline-form {{
      display: inline;
    }}
    .actions-cell {{
      line-height: 1.9;
    }}
    @media (max-width: 720px) {{
      main {{
        width: calc(100vw - 1rem);
        margin: 0.5rem auto 1rem;
        border-radius: 18px;
      }}
      header, .controls, .table-wrap {{
        padding-left: 1rem;
        padding-right: 1rem;
      }}
      .section-heading, .section-note, .banner {{
        margin-left: 1rem;
        margin-right: 1rem;
        padding-left: 0;
        padding-right: 0;
      }}
      .control-row {{
        align-items: stretch;
      }}
      .control-row > button,
      .control-row > .button-link,
      .control-row > select,
      .control-row > input[type="text"] {{
        width: 100%;
      }}
      button, .button-link {{
        min-height: 2.9rem;
      }}
      label {{
        min-width: auto;
        width: 100%;
      }}
      input[type="text"], select {{
        min-width: 0;
        width: 100%;
      }}
      .table-wrap {{
        overflow-x: visible;
      }}
      .library-grid {{
        grid-template-columns: 1fr;
        padding-left: 1rem;
        padding-right: 1rem;
      }}
      .track-sidebar {{
        position: static;
        order: -1;
      }}
      table {{
        display: block;
        margin-top: 0;
      }}
      thead {{
        display: none;
      }}
      tbody {{
        display: grid;
        gap: 0.9rem;
      }}
      tbody tr {{
        display: grid;
        gap: 0.65rem;
        padding: 0.95rem;
        border: 1px solid var(--line);
        border-radius: 18px;
        background: rgba(255, 255, 255, 0.7);
      }}
      tbody tr:hover {{
        background: rgba(255, 255, 255, 0.82);
      }}
      td {{
        display: grid;
        grid-template-columns: 5.8rem minmax(0, 1fr);
        gap: 0.75rem;
        padding: 0;
        border: 0;
        align-items: start;
      }}
      td::before {{
        content: attr(data-label);
        color: var(--muted);
        font-size: 0.82rem;
        text-transform: uppercase;
        letter-spacing: 0.05em;
      }}
      td[data-label="Cover"]::before,
      td[data-label="Play"]::before {{
        align-self: center;
      }}
      .cover-thumb {{
        width: 3.5rem;
        height: 3.5rem;
      }}
      .actions-cell {{
        line-height: 1.6;
      }}
      .actions-cell .inline-form {{
        display: block;
        margin-bottom: 0.45rem;
      }}
      .actions-cell button,
      .actions-cell a {{
        width: 100%;
        display: inline-flex;
        justify-content: center;
      }}
      .muted-sep {{
        display: none;
      }}
    }}
  </style>
</head>
<body>
  <main>
    <header>
      <h1>{}</h1>
      <p class="meta">Library path: {}</p>
      <p class="meta">Indexed tracks: {}</p>
      <p class="meta">Stream base URL: {}</p>
    </header>
    {}{}
    <section class="controls">
      <form id="playback_form" class="control-row" action="/play" method="get">
        <label for="renderer_location">Renderer LOCATION</label>
        <input id="renderer_location" name="renderer_location" type="text" value="{}" placeholder="http://192.168.1.55:49152/description.xml" oninput="syncRendererFields(this.value)" onchange="refreshQueuePanel()">
        <button type="submit">Play Selected Track</button>
      </form>
      <div class="control-row">
        <button type="button" class="secondary" onclick="discoverRenderers()">Discover Renderers</button>
        <select id="renderer_discovery">
          {}
        </select>
        <button type="button" class="secondary" onclick="applySelectedRenderer()">Use Selected Renderer</button>
      </div>
      <form class="control-row" action="/rescan" method="get">
        <input id="rescan_renderer_location" class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
        <label for="track_filter">Search Tracks</label>
        <input id="track_filter" type="text" placeholder="Filter by title, artist, album, or path" oninput="filterTracks()">
        <button type="submit" class="secondary">Rescan Library</button>
      </form>
    </section>
    {}
    <div id="queue_panel_host">
      {}
    </div>
    <h2 class="section-heading">Albums</h2>
    <p class="section-note">Album playback fills the queue in disc/track order and will advance automatically. Use the queue controls to pause, stop, or skip.</p>
    {}
    <section class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Cover</th>
            <th>Album</th>
            <th>Artist</th>
            <th>Tracks</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody id="album_table">
          {}
        </tbody>
      </table>
    </section>
    <h2 class="section-heading">Tracks</h2>
    <section class="library-grid">
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>Play</th>
              <th>Cover</th>
              <th>Title</th>
              <th>Artist</th>
              <th>Album</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody id="track_table">
            {}
          </tbody>
        </table>
      </div>
      <aside id="track_detail_panel" class="track-sidebar">
        <h3>Track Tags</h3>
        <p>Select a track to inspect its embedded tags, artwork source, and file metadata here.</p>
      </aside>
    </section>
  </main>
  <script>
    function escapeHtml(value) {{
      return String(value)
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
    }}

    function formatDuration(seconds) {{
      if (seconds === null || seconds === undefined || Number.isNaN(Number(seconds))) {{
        return 'Unknown';
      }}
      const total = Number(seconds);
      const hours = Math.floor(total / 3600);
      const minutes = Math.floor((total % 3600) / 60);
      const secs = total % 60;
      if (hours > 0) {{
        return `${{hours}}:${{String(minutes).padStart(2, '0')}}:${{String(secs).padStart(2, '0')}}`;
      }}
      return `${{minutes}}:${{String(secs).padStart(2, '0')}}`;
    }}

    function renderTrackDetailPanel(track) {{
      const host = document.getElementById('track_detail_panel');
      if (!host) {{
        return;
      }}
      if (!track || track.error) {{
        host.innerHTML = `<h3>Track Tags</h3><p>${{escapeHtml(track?.error || 'Track details are unavailable.')}}</p>`;
        return;
      }}

      const artworkHtml = track.artwork
        ? `<img class="sidebar-artwork" src="${{escapeHtml(track.artwork.url)}}" alt="Artwork for ${{escapeHtml(track.album)}}">`
        : '<div class="sidebar-artwork placeholder">No Art</div>';

      const metaRows = [
        {{ label: 'Artist', value: track.artist || 'Unknown' }},
        {{ label: 'Album', value: track.album || 'Unknown' }},
        {{ label: 'Disc / Track', value: `${{track.disc_number ?? '?'}} / ${{track.track_number ?? '?'}}` }},
        {{ label: 'Duration', value: formatDuration(track.duration_seconds) }},
        {{ label: 'Format', value: track.mime_type || 'Unknown' }},
        {{ label: 'Parser', value: track.embedded_metadata?.parser || 'Unknown' }},
        {{ label: 'Path', value: `<code>${{escapeHtml(track.relative_path || track.absolute_path || '')}}</code>`, isHtml: true }},
      ]
        .map((item) => `
          <div class="track-sidebar-meta-row">
            <div class="track-sidebar-label">${{escapeHtml(item.label)}}</div>
            <div class="track-sidebar-value">${{item.isHtml ? item.value : escapeHtml(item.value)}}</div>
          </div>
        `)
        .join('');

      const tagRows = (track.embedded_metadata?.fields || []).length
        ? track.embedded_metadata.fields
            .map((field) => `
              <div class="track-sidebar-tag">
                <div class="track-sidebar-label">${{escapeHtml(field.key)}}</div>
                <div class="track-sidebar-tag-value"><code>${{escapeHtml(field.value)}}</code></div>
              </div>
            `)
            .join('')
        : '<div class="track-sidebar-tag"><div class="track-sidebar-value">No embedded tag fields were parsed for this file.</div></div>';

      const notesHtml = (track.embedded_metadata?.notes || []).length
        ? `<ul class="track-sidebar-note-list">${{track.embedded_metadata.notes.map((note) => `<li>${{escapeHtml(note)}}</li>`).join('')}}</ul>`
        : '';

      host.innerHTML = `
        <h3>${{escapeHtml(track.title || 'Track Tags')}}</h3>
        <p>${{escapeHtml(track.artist || 'Unknown artist')}} • ${{escapeHtml(track.album || 'Unknown album')}}</p>
        ${{artworkHtml}}
        <div class="track-sidebar-actions">
          <a class="button-link secondary" href="/track/${{encodeURIComponent(track.id)}}" target="_blank" rel="noreferrer">Full Inspector</a>
          <a class="button-link secondary" href="/stream/track/${{encodeURIComponent(track.id)}}" target="_blank" rel="noreferrer">Preview</a>
        </div>
        <div class="track-sidebar-meta">${{metaRows}}</div>
        <div class="track-sidebar-tags">${{tagRows}}</div>
        ${{notesHtml}}
      `;
    }}

    async function loadTrackDetails(trackId) {{
      if (!trackId) {{
        return;
      }}
      try {{
        const response = await fetch(`/api/tracks/${{encodeURIComponent(trackId)}}`);
        const payload = await response.json();
        renderTrackDetailPanel(payload);
      }} catch (_error) {{
        renderTrackDetailPanel({{ error: 'Failed to load track details.' }});
      }}
    }}

    function syncSelectedTrackSidebar() {{
      const selected = document.querySelector('input[name="track_id"]:checked');
      if (selected) {{
        loadTrackDetails(selected.value);
      }}
    }}

    async function discoverRenderers() {{
      const select = document.getElementById('renderer_discovery');
      select.innerHTML = '<option value="">Discovering renderers...</option>';
      try {{
        const response = await fetch('/api/renderers/discover');
        const items = await response.json();
        select.innerHTML = '';
        if (!items.length) {{
          select.innerHTML = '<option value="">No renderers discovered</option>';
          return;
        }}
        for (const item of items) {{
          const option = document.createElement('option');
          option.value = item.location;
          option.textContent = item.name || item.location;
          select.appendChild(option);
        }}
      }} catch (error) {{
        select.innerHTML = '<option value="">Discovery failed</option>';
      }}
    }}

    function applySelectedRenderer() {{
      const select = document.getElementById('renderer_discovery');
      const input = document.getElementById('renderer_location');
      if (select.value) {{
        input.value = select.value;
        syncRendererFields(select.value);
        refreshQueuePanel();
      }}
    }}

    function syncRendererFields(value) {{
      const hidden = document.getElementById('rescan_renderer_location');
      if (hidden) {{
        hidden.value = value;
      }}
      const proxies = document.querySelectorAll('.renderer-location-proxy');
      for (const proxy of proxies) {{
        proxy.value = value;
      }}
    }}

    let queueRefreshTimer = null;
    let queueRefreshInFlight = false;

    async function refreshQueuePanel() {{
      const rendererInput = document.getElementById('renderer_location');
      const host = document.getElementById('queue_panel_host');
      if (!rendererInput || !host) {{
        return;
      }}
      const rendererLocation = rendererInput.value.trim();
      if (queueRefreshInFlight) {{
        return;
      }}
      queueRefreshInFlight = true;
      try {{
        const url = rendererLocation
          ? `/queue/panel?renderer_location=${{encodeURIComponent(rendererLocation)}}`
          : '/queue/panel';
        const response = await fetch(url, {{
          headers: {{
            'X-Requested-With': 'musicd-live-refresh'
          }}
        }});
        if (!response.ok) {{
          return;
        }}
        host.innerHTML = await response.text();
        syncRendererFields(rendererLocation);
      }} catch (_error) {{
      }} finally {{
        queueRefreshInFlight = false;
      }}
    }}

    function startQueueRefresh() {{
      if (queueRefreshTimer !== null) {{
        clearInterval(queueRefreshTimer);
      }}
      document.addEventListener('visibilitychange', () => {{
        if (!document.hidden) {{
          refreshQueuePanel();
        }}
      }});
      queueRefreshTimer = setInterval(() => {{
        if (document.hidden) {{
          return;
        }}
        refreshQueuePanel();
      }}, 2500);
    }}

    function filterTracks() {{
      const needle = document.getElementById('track_filter').value.trim().toLowerCase();
      const rows = document.querySelectorAll('#track_table tr');
      for (const row of rows) {{
        row.style.display = !needle || row.dataset.search.includes(needle) ? '' : 'none';
      }}
      const albumRows = document.querySelectorAll('#album_table tr');
      for (const row of albumRows) {{
        row.style.display = !needle || row.dataset.search.includes(needle) ? '' : 'none';
      }}
    }}

    document.addEventListener('change', (event) => {{
      if (event.target instanceof HTMLInputElement && event.target.name === 'track_id') {{
        loadTrackDetails(event.target.value);
      }}
    }});

    refreshQueuePanel();
    startQueueRefresh();
    syncSelectedTrackSidebar();
  </script>
</body>
</html>"#,
        html_escape(&state.config.instance_name),
        html_escape(&state.config.instance_name),
        html_escape(&library_path),
        tracks.len(),
        html_escape(&state.config.resolved_base_url()),
        message_html,
        error_html,
        html_escape(&renderer_location),
        renderer_options,
        html_escape(&renderer_location),
        empty_state,
        queue_html,
        album_empty_state,
        album_rows,
        rows,
    )
}

fn render_queue_panel(
    state: &ServiceState,
    renderer_location: &str,
    library: &Library,
) -> String {
    if renderer_location.trim().is_empty() {
        return "<section class=\"table-wrap\"><p class=\"empty\">Enter a renderer LOCATION URL to inspect or build a queue.</p></section>".to_string();
    }

    let lookup_track = |track_id: &str| -> Option<&LibraryTrack> {
        library
            .track_index
            .get(track_id)
            .map(|&idx| &library.tracks[idx])
    };

    let queue = state.queue_snapshot(renderer_location);
    let session = state.playback_session(renderer_location);
    let current_track = queue.as_ref().and_then(|queue| {
        queue.current_entry_id.and_then(|current_entry_id| {
            queue
                .entries
                .iter()
                .find(|entry| entry.id == current_entry_id)
                .and_then(|entry| lookup_track(&entry.track_id))
        })
    });
    let progress_note = session
        .as_ref()
        .and_then(|session| {
            session.position_seconds.map(|position| {
                let duration = session
                    .duration_seconds
                    .map(format_duration_seconds)
                    .unwrap_or_else(|| "Unknown".to_string());
                format!("{} / {}", format_duration_seconds(position), duration)
            })
        })
        .unwrap_or_else(|| "Unknown progress".to_string());

    let session_meta = session
        .as_ref()
        .map(|session| {
            let error_note = session
                .last_error
                .as_ref()
                .map(|error| format!(" Error: {}.", html_escape(error)))
                .unwrap_or_default();
            let current_note = current_track
                .map(|track| {
                    format!(
                        " Current track: '{}' by {} from {}.",
                        html_escape(&track.title),
                        html_escape(&track.artist),
                        html_escape(&track.album)
                    )
                })
                .unwrap_or_default();
            format!(
                "<p class=\"section-note\">Renderer session: {}. Progress: {}. Last observed: {}.{}{} </p>",
                html_escape(&session.transport_state),
                html_escape(&progress_note),
                session.last_observed_unix,
                error_note,
                current_note
            )
        })
        .unwrap_or_else(|| {
            "<p class=\"section-note\">No playback session has been recorded for this renderer yet.</p>"
                .to_string()
        });

    let Some(queue) = queue else {
        return format!(
            "<h2 class=\"section-heading\">Queue</h2>{session_meta}<section class=\"table-wrap\"><p class=\"empty\">No queue has been saved for this renderer yet.</p></section>"
        );
    };

    let rows = queue
        .entries
        .iter()
        .map(|entry| {
            let track = lookup_track(&entry.track_id);
            let title = track
                .map(|track| track.title.clone())
                .unwrap_or_else(|| "Missing track".to_string());
            let album = track
                .map(|track| track.album.clone())
                .unwrap_or_else(|| "Unknown album".to_string());
            let marker = if Some(entry.id) == queue.current_entry_id {
                "Current"
            } else {
                ""
            };
            let duration = track
                .and_then(|track| track.duration_seconds)
                .map(format_duration_seconds)
                .unwrap_or_else(|| "Unknown".to_string());
            let actions = if Some(entry.id) == queue.current_entry_id {
                "<span class=\"meta\">Current entry</span>".to_string()
            } else {
                let mut actions = Vec::new();
                if previous_queue_entry_before(&queue, entry.id).is_some() {
                    actions.push(format!(
                        "<form class=\"inline-form\" action=\"/queue/move-up\" method=\"get\"><input type=\"hidden\" name=\"entry_id\" value=\"{}\"><input type=\"hidden\" name=\"return_to\" value=\"/\"><input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\"><button type=\"submit\" class=\"secondary\">Move Up</button></form>",
                        entry.id,
                        html_escape(renderer_location)
                    ));
                }
                if next_queue_entry_after(&queue, entry.id).is_some() {
                    actions.push(format!(
                        "<form class=\"inline-form\" action=\"/queue/move-down\" method=\"get\"><input type=\"hidden\" name=\"entry_id\" value=\"{}\"><input type=\"hidden\" name=\"return_to\" value=\"/\"><input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\"><button type=\"submit\" class=\"secondary\">Move Down</button></form>",
                        entry.id,
                        html_escape(renderer_location)
                    ));
                }
                actions.push(format!(
                    "<form class=\"inline-form\" action=\"/queue/remove-entry\" method=\"get\"><input type=\"hidden\" name=\"entry_id\" value=\"{}\"><input type=\"hidden\" name=\"return_to\" value=\"/\"><input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\"><button type=\"submit\" class=\"secondary\">Remove</button></form>",
                    entry.id,
                    html_escape(renderer_location)
                ));
                actions.join(" <span class=\"muted-sep\">|</span> ")
            };
            format!(
                "<tr><td data-label=\"Position\">{}</td><td data-label=\"Marker\">{}</td><td data-label=\"Title\">{}</td><td data-label=\"Album\">{}</td><td data-label=\"Duration\">{}</td><td data-label=\"Actions\" class=\"actions-cell\">{}</td></tr>",
                entry.position,
                html_escape(marker),
                html_escape(&title),
                html_escape(&album),
                html_escape(&duration),
                actions
            )
        })
        .collect::<Vec<_>>()
        .join("");

    format!(
        r#"<h2 class="section-heading">Queue</h2>
{session_meta}
<p class="section-note">Renderer: {}. Status: {}. Queue version: {}.</p>
<section class="table-wrap">
  <div class="control-row" style="padding: 0 0 1rem;">
    <form class="inline-form" action="/transport/previous" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit" class="secondary">Previous</button>
    </form>
    <form class="inline-form" action="/transport/play" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit">Play</button>
    </form>
    <form class="inline-form" action="/transport/pause" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit" class="secondary">Pause</button>
    </form>
    <form class="inline-form" action="/transport/stop" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit" class="secondary">Stop</button>
    </form>
    <form class="inline-form" action="/transport/next" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit" class="secondary">Next</button>
    </form>
    <form class="inline-form" action="/queue/clear" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit" class="secondary">Clear Queue</button>
    </form>
  </div>
  <table>
    <thead>
      <tr>
        <th>Position</th>
        <th>Marker</th>
        <th>Title</th>
        <th>Album</th>
        <th>Duration</th>
        <th>Actions</th>
      </tr>
    </thead>
    <tbody>{}</tbody>
  </table>
</section>"#,
        html_escape(renderer_location),
        html_escape(&queue.status),
        queue.version,
        html_escape(renderer_location),
        html_escape(renderer_location),
        html_escape(renderer_location),
        html_escape(renderer_location),
        html_escape(renderer_location),
        html_escape(renderer_location),
        rows,
    )
}

fn render_queue_panel_html(state: &ServiceState, request: &HttpRequest) -> String {
    let renderer_location = state
        .preferred_renderer_location(request.query.get("renderer_location").map(String::as_str));
    let library = state.library_snapshot();
    render_queue_panel(state, &renderer_location, &library)
}

fn render_album_detail_page(state: &ServiceState, request: &HttpRequest) -> String {
    let album_id = request.path.trim_start_matches("/album/");
    let renderer_location = state
        .preferred_renderer_location(request.query.get("renderer_location").map(String::as_str));
    let message = request.query.get("message").cloned().unwrap_or_default();
    let error = request.query.get("error").cloned().unwrap_or_default();

    let Some(album) = state.find_album(album_id) else {
        return render_detail_error_page("Album not found");
    };

    let tracks = state.tracks_for_album(&album.id);
    let artwork_html = album
        .artwork_url
        .as_ref()
        .map(|artwork_url| {
            format!(
                "<img class=\"detail-artwork\" src=\"{}\" alt=\"Artwork for {}\">",
                html_escape(artwork_url),
                html_escape(&album.title)
            )
        })
        .unwrap_or_else(|| {
            "<div class=\"detail-artwork placeholder\">No album artwork found yet.</div>"
                .to_string()
        });
    let message_html = if message.is_empty() {
        String::new()
    } else {
        format!("<p class=\"banner success\">{}</p>", html_escape(&message))
    };
    let error_html = if error.is_empty() {
        String::new()
    } else {
        format!("<p class=\"banner error\">{}</p>", html_escape(&error))
    };
    let track_rows = tracks
        .iter()
        .map(|track| {
            let play_url = format!(
                "/play?track_id={}&renderer_location={}",
                url_encode(&track.id),
                url_encode(&renderer_location)
            );
            let play_next_url = format!(
                "/queue/play-next-track?track_id={}&renderer_location={}&return_to=%2Falbum%2F{}",
                url_encode(&track.id),
                url_encode(&renderer_location),
                url_encode(&album.id)
            );
            let queue_url = format!(
                "/queue/append-track?track_id={}&renderer_location={}&return_to=%2Falbum%2F{}",
                url_encode(&track.id),
                url_encode(&renderer_location),
                url_encode(&album.id)
            );
            format!(
                "<tr><td data-label=\"Position\">{}</td><td data-label=\"Title\">{}</td><td data-label=\"Artist\">{}</td><td data-label=\"Actions\" class=\"actions-cell\"><a href=\"{}\">Play Track</a> <span class=\"muted-sep\">|</span> <a href=\"{}\">Play Next</a> <span class=\"muted-sep\">|</span> <a href=\"{}\">Queue</a> <span class=\"muted-sep\">|</span> <a href=\"/track/{}?renderer_location={}\" target=\"_blank\" rel=\"noreferrer\">Inspect</a></td></tr>",
                html_escape(&format_track_position(track.disc_number, track.track_number)),
                html_escape(&track.title),
                html_escape(&track.artist),
                html_escape(&play_url),
                html_escape(&play_next_url),
                html_escape(&queue_url),
                html_escape(&track.id),
                html_escape(&renderer_location),
            )
        })
        .collect::<Vec<_>>()
        .join("");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{}</title>
  <style>
    :root {{
      --bg: #f8f4eb;
      --panel: #fffdf8;
      --ink: #1f1a17;
      --muted: #6f665f;
      --line: rgba(31, 26, 23, 0.12);
      --accent: #166534;
      --danger: #991b1b;
    }}
    body {{
      margin: 0;
      font-family: Georgia, "Iowan Old Style", serif;
      background: linear-gradient(180deg, #f7f0e2 0%, #fdfaf2 100%);
      color: var(--ink);
    }}
    main {{
      width: min(980px, calc(100vw - 2rem));
      margin: 1.5rem auto 3rem;
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 20px;
      overflow: hidden;
      box-shadow: 0 18px 42px rgba(31, 26, 23, 0.1);
    }}
    header, section {{
      padding: 1.4rem 1.5rem;
    }}
    header {{
      border-bottom: 1px solid var(--line);
      background: rgba(22, 101, 52, 0.06);
    }}
    h1, h2 {{
      margin: 0 0 0.6rem;
    }}
    p {{
      margin: 0.25rem 0;
      color: var(--muted);
    }}
    .layout {{
      display: grid;
      grid-template-columns: minmax(0, 18rem) minmax(0, 1fr);
      gap: 1.5rem;
      align-items: start;
    }}
    .detail-artwork {{
      width: 100%;
      display: block;
      border-radius: 18px;
      border: 1px solid var(--line);
      box-shadow: 0 14px 28px rgba(31, 26, 23, 0.12);
      background: rgba(31, 26, 23, 0.05);
      min-height: 18rem;
      object-fit: cover;
    }}
    .detail-artwork.placeholder {{
      display: flex;
      align-items: center;
      justify-content: center;
      padding: 1rem;
      text-align: center;
    }}
    .actions {{
      display: flex;
      gap: 0.75rem;
      flex-wrap: wrap;
      margin-top: 1rem;
    }}
    .actions-cell {{
      line-height: 1.8;
    }}
    .actions a, .actions button {{
      text-decoration: none;
      color: white;
      background: #1f1a17;
      padding: 0.75rem 1rem;
      border-radius: 999px;
      border: 0;
      font: inherit;
      cursor: pointer;
    }}
    .actions a.secondary {{
      color: #1f1a17;
      background: #eadfce;
    }}
    input[type="text"] {{
      width: 100%;
      border: 1px solid var(--line);
      background: #fffdfa;
      border-radius: 12px;
      padding: 0.8rem 0.9rem;
      font: inherit;
    }}
    table {{
      width: 100%;
      border-collapse: collapse;
    }}
    th, td {{
      text-align: left;
      vertical-align: top;
      border-top: 1px solid var(--line);
      padding: 0.85rem 0.9rem;
    }}
    th {{
      color: var(--muted);
      font-weight: 600;
    }}
    .banner {{
      margin: 0 1.5rem 1rem;
      padding: 0.9rem 1rem;
      border-radius: 14px;
    }}
    .banner.success {{
      background: rgba(22, 101, 52, 0.1);
      color: var(--accent);
    }}
    .banner.error {{
      background: rgba(153, 27, 27, 0.08);
      color: var(--danger);
    }}
    @media (max-width: 760px) {{
      .layout {{
        grid-template-columns: 1fr;
      }}
      main {{
        width: calc(100vw - 1rem);
        margin: 0.5rem auto 1rem;
        border-radius: 18px;
      }}
      header, section {{
        padding: 1rem;
      }}
      .actions a, .actions button {{
        width: 100%;
        justify-content: center;
        text-align: center;
      }}
      table {{
        display: block;
      }}
      thead {{
        display: none;
      }}
      tbody {{
        display: grid;
        gap: 0.8rem;
      }}
      tbody tr {{
        display: grid;
        gap: 0.65rem;
        padding: 0.9rem;
        border: 1px solid var(--line);
        border-radius: 16px;
        background: rgba(255, 255, 255, 0.72);
      }}
      th, td {{
        padding: 0;
        border: 0;
      }}
      td {{
        display: grid;
        grid-template-columns: 5.3rem minmax(0, 1fr);
        gap: 0.7rem;
      }}
      td::before {{
        content: attr(data-label);
        color: var(--muted);
        font-size: 0.82rem;
        text-transform: uppercase;
        letter-spacing: 0.05em;
      }}
      .actions-cell a {{
        display: block;
        padding: 0.65rem 0.8rem;
        border-radius: 12px;
        margin-bottom: 0.4rem;
        background: rgba(31, 26, 23, 0.08);
      }}
      .muted-sep {{
        display: none;
      }}
    }}
  </style>
</head>
<body>
  <main>
    <header>
      <h1>{}</h1>
      <p>{} tracks • {}</p>
      <div class="actions">
        <a href="/">Back to Library</a>
      </div>
    </header>
    {}{}
    <section class="layout">
      <div>{}</div>
      <div>
        <h2>Play Album</h2>
        <p>Play the album now, place it next in line, or add it to the end of the queue.</p>
        <form action="/play-album" method="get">
          <input type="hidden" name="album_id" value="{}">
          <label for="renderer_location" style="display:block; font-weight:600; margin-bottom:0.5rem;">Renderer LOCATION</label>
          <input id="renderer_location" name="renderer_location" type="text" value="{}" placeholder="http://192.168.1.55:49152/description.xml">
          <div class="actions">
            <button type="submit">Play Album</button>
            <button type="submit" formaction="/queue/play-next-album">Play Next</button>
            <button type="submit" formaction="/queue/append-album">Queue Album</button>
            <input type="hidden" name="return_to" value="/album/{}">
            <a class="secondary" href="/stream/track/{}" target="_blank" rel="noreferrer">Preview First Track</a>
          </div>
        </form>
      </div>
    </section>
    <section>
      <h2>Tracks</h2>
      <table>
        <thead>
          <tr>
            <th>Position</th>
            <th>Title</th>
            <th>Artist</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>{}</tbody>
      </table>
    </section>
  </main>
</body>
</html>"#,
        html_escape(&album.title),
        html_escape(&album.title),
        album.track_count,
        html_escape(&album.artist),
        message_html,
        error_html,
        artwork_html,
        html_escape(&album.id),
        html_escape(&renderer_location),
        html_escape(&album.id),
        html_escape(&album.first_track_id),
        track_rows,
    )
}

fn render_track_detail_page(state: &ServiceState, request: &HttpRequest) -> String {
    let track_id = request.path.trim_start_matches("/track/");
    let renderer_location = request
        .query
        .get("renderer_location")
        .cloned()
        .or_else(|| state.config.default_renderer_location.clone())
        .unwrap_or_default();

    let Some(track) = state.find_track(track_id) else {
        return render_detail_error_page("Track not found");
    };

    let metadata =
        inspect_embedded_metadata(&track.path).unwrap_or_else(|error| EmbeddedMetadata {
            format_name: "Unreadable".to_string(),
            fields: Vec::new(),
            notes: vec![format!("Failed to inspect embedded metadata: {error}")],
        });
    let artwork_url = state.relative_artwork_url_for_track(&track);

    let inferred_rows = [
        ("Track ID", track.id.clone()),
        ("Album ID", track.album_id.clone()),
        ("Title", track.title.clone()),
        ("Artist", track.artist.clone()),
        ("Album", track.album.clone()),
        (
            "Disc / Track",
            format_track_position(track.disc_number, track.track_number),
        ),
        (
            "Duration",
            track
                .duration_seconds
                .map(format_duration_seconds)
                .unwrap_or_else(|| "Unknown".to_string()),
        ),
        ("Relative path", track.relative_path.clone()),
        ("Absolute path", track.path.display().to_string()),
        ("MIME type", track.mime_type.clone()),
        ("File size", format!("{} bytes", track.file_size)),
        (
            "Artwork URL",
            artwork_url.clone().unwrap_or_else(|| "None".to_string()),
        ),
        (
            "Artwork source",
            track
                .artwork
                .as_ref()
                .map(|artwork| artwork.source.clone())
                .unwrap_or_else(|| "None".to_string()),
        ),
        (
            "Artwork MIME type",
            track
                .artwork
                .as_ref()
                .map(|artwork| artwork.mime_type.clone())
                .unwrap_or_else(|| "None".to_string()),
        ),
        (
            "Artwork cache key",
            track
                .artwork
                .as_ref()
                .map(|artwork| artwork.cache_key.clone())
                .unwrap_or_else(|| "None".to_string()),
        ),
    ]
    .into_iter()
    .map(|(label, value)| {
        format!(
            "<tr><th>{}</th><td><code>{}</code></td></tr>",
            html_escape(label),
            html_escape(&value)
        )
    })
    .collect::<Vec<_>>()
    .join("");

    let embedded_rows = if metadata.fields.is_empty() {
        "<tr><th>Embedded fields</th><td><em>No parsed embedded fields for this file yet.</em></td></tr>".to_string()
    } else {
        metadata
            .fields
            .iter()
            .map(|(label, value)| {
                format!(
                    "<tr><th>{}</th><td><code>{}</code></td></tr>",
                    html_escape(label),
                    html_escape(value)
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };

    let notes_html = if metadata.notes.is_empty() {
        String::new()
    } else {
        let items = metadata
            .notes
            .iter()
            .map(|note| format!("<li>{}</li>", html_escape(note)))
            .collect::<Vec<_>>()
            .join("");
        format!("<ul>{items}</ul>")
    };

    let play_url = format!(
        "/play?track_id={}&renderer_location={}",
        url_encode(&track.id),
        url_encode(&renderer_location)
    );
    let queue_url = format!(
        "/queue/append-track?track_id={}&renderer_location={}&return_to={}",
        url_encode(&track.id),
        url_encode(&renderer_location),
        url_encode(&format!("/track/{}", track.id))
    );
    let artwork_html = artwork_url
        .map(|url| {
            format!(
                "<section><h2>Artwork</h2><img class=\"detail-artwork\" src=\"{}\" alt=\"Artwork for {}\"></section>",
                html_escape(&url),
                html_escape(&track.album)
            )
        })
        .unwrap_or_default();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Inspect {}</title>
  <style>
    :root {{
      --bg: #f8f4eb;
      --panel: #fffdf8;
      --ink: #1f1a17;
      --muted: #6f665f;
      --line: rgba(31, 26, 23, 0.12);
    }}
    body {{
      margin: 0;
      font-family: Georgia, "Iowan Old Style", serif;
      background: linear-gradient(180deg, #f7f0e2 0%, #fdfaf2 100%);
      color: var(--ink);
    }}
    main {{
      width: min(980px, calc(100vw - 2rem));
      margin: 1.5rem auto 3rem;
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 20px;
      overflow: hidden;
      box-shadow: 0 18px 42px rgba(31, 26, 23, 0.1);
    }}
    header, section {{
      padding: 1.4rem 1.5rem;
    }}
    header {{
      border-bottom: 1px solid var(--line);
      background: rgba(22, 101, 52, 0.06);
    }}
    h1, h2 {{
      margin: 0 0 0.6rem;
    }}
    p {{
      margin: 0.25rem 0;
      color: var(--muted);
    }}
    table {{
      width: 100%;
      border-collapse: collapse;
    }}
    th, td {{
      text-align: left;
      vertical-align: top;
      border-top: 1px solid var(--line);
      padding: 0.85rem 0.9rem;
    }}
    th {{
      width: 14rem;
      color: var(--muted);
      font-weight: 600;
    }}
    code {{
      font-family: "SFMono-Regular", Menlo, monospace;
      font-size: 0.95em;
      word-break: break-word;
    }}
    .actions {{
      display: flex;
      gap: 0.75rem;
      flex-wrap: wrap;
    }}
    .metadata-table td {{
      word-break: break-word;
    }}
    .actions a {{
      text-decoration: none;
      color: white;
      background: #1f1a17;
      padding: 0.75rem 1rem;
      border-radius: 999px;
    }}
    .actions a.secondary {{
      color: #1f1a17;
      background: #eadfce;
    }}
    .detail-artwork {{
      width: min(18rem, 100%);
      display: block;
      border-radius: 18px;
      border: 1px solid var(--line);
      box-shadow: 0 14px 28px rgba(31, 26, 23, 0.12);
    }}
    @media (max-width: 760px) {{
      main {{
        width: calc(100vw - 1rem);
        margin: 0.5rem auto 1rem;
        border-radius: 18px;
      }}
      header, section {{
        padding: 1rem;
      }}
      .actions a {{
        width: 100%;
        text-align: center;
      }}
      table {{
        display: block;
      }}
      tbody {{
        display: grid;
        gap: 0.8rem;
      }}
      tr {{
        display: grid;
        gap: 0.4rem;
        padding: 0.9rem;
        border: 1px solid var(--line);
        border-radius: 16px;
        background: rgba(255, 255, 255, 0.72);
      }}
      th, td {{
        display: block;
        width: auto;
        padding: 0;
        border: 0;
      }}
      th {{
        font-size: 0.82rem;
        text-transform: uppercase;
        letter-spacing: 0.05em;
      }}
      code {{
        white-space: pre-wrap;
      }}
    }}
  </style>
</head>
<body>
  <main>
    <header>
      <h1>{}</h1>
      <p>{} • {}</p>
      <div class="actions">
        <a href="/">Back to Library</a>
        <a class="secondary" href="/album/{}?renderer_location={}">View Album</a>
        <a class="secondary" href="/stream/track/{}" target="_blank" rel="noreferrer">Preview Stream</a>
        <a class="secondary" href="{}">Queue Track</a>
        <a href="{}">Play On Renderer</a>
      </div>
    </header>
    <section>
      <h2>Inferred Library Metadata</h2>
      <table class="metadata-table"><tbody>{}</tbody></table>
    </section>
    {}
    <section>
      <h2>Embedded File Metadata</h2>
      <p>Parser: {}</p>
      <table class="metadata-table"><tbody>{}</tbody></table>
      {}
    </section>
  </main>
</body>
</html>"#,
        html_escape(&track.title),
        html_escape(&track.title),
        html_escape(&track.artist),
        html_escape(&track.album),
        html_escape(&track.album_id),
        html_escape(&renderer_location),
        html_escape(&track.id),
        html_escape(&queue_url),
        html_escape(&play_url),
        inferred_rows,
        artwork_html,
        html_escape(&metadata.format_name),
        embedded_rows,
        notes_html,
    )
}

fn render_track_detail_json(state: &ServiceState, request: &HttpRequest) -> String {
    let track_id = request.path.trim_start_matches("/api/tracks/");
    let Some(track) = state.find_track(track_id) else {
        return r#"{"error":"track not found"}"#.to_string();
    };

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
        r#"{{"id":"{}","album_id":"{}","title":"{}","artist":"{}","album":"{}","disc_number":{},"track_number":{},"duration_seconds":{},"relative_path":"{}","absolute_path":"{}","mime_type":"{}","size":{},"artwork":{},"embedded_metadata":{{"parser":"{}","fields":[{}],"notes":[{}]}}}}"#,
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
        json_escape(&metadata.format_name),
        fields,
        notes,
    )
}

fn render_detail_error_page(message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Track Error</title></head>
<body style="font-family: Georgia, serif; margin: 2rem;">
  <h1>Track Inspector</h1>
  <p>{}</p>
  <p><a href="/">Back to Library</a></p>
</body>
</html>"#,
        html_escape(message)
    )
}

fn render_tracks_json(state: &ServiceState) -> String {
    let tracks = state.tracks_snapshot();
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
            let fallback_artwork_url =
                album_artwork_by_id.get(&track.album_id).map(String::as_str);
            let summary_json = track_summary_json(track, fallback_artwork_url);
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

fn render_albums_json(state: &ServiceState) -> String {
    let albums = state.albums_snapshot();
    let entries = albums
        .iter()
        .map(album_summary_json)
        .collect::<Vec<_>>()
        .join(",");
    format!("[{entries}]")
}

fn render_artists_json(state: &ServiceState) -> String {
    let artists = state.artists_snapshot();
    let entries = artists
        .into_iter()
        .map(|artist| artist_summary_json(&artist))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{entries}]")
}

fn render_album_detail_json(state: &ServiceState, request: &HttpRequest) -> String {
    let album_id = request
        .path
        .trim_start_matches("/api/albums/")
        .trim_end_matches("/artwork/candidates");
    let Some(album) = state.find_album(album_id) else {
        return r#"{"error":"album not found"}"#.to_string();
    };
    let tracks = state.tracks_for_album(&album.id);
    let tracks_json = tracks
        .into_iter()
        .map(|track| track_summary_json(&track, album.artwork_url.as_deref()))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"id":"{}","title":"{}","artist":"{}","track_count":{},"first_track_id":"{}","artwork_url":"{}","tracks":[{}]}}"#,
        json_escape(&album.id),
        json_escape(&album.title),
        json_escape(&album.artist),
        album.track_count,
        json_escape(&album.first_track_id),
        json_escape(album.artwork_url.as_deref().unwrap_or_default()),
        tracks_json,
    )
}

fn album_summary_json(album: &AlbumSummary) -> String {
    format!(
        r#"{{"id":"{}","title":"{}","artist":"{}","track_count":{},"first_track_id":"{}","artwork_url":"{}"}}"#,
        json_escape(&album.id),
        json_escape(&album.title),
        json_escape(&album.artist),
        album.track_count,
        json_escape(&album.first_track_id),
        json_escape(album.artwork_url.as_deref().unwrap_or_default()),
    )
}

fn artist_summary_json(artist: &ArtistSummary) -> String {
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

fn render_artist_detail_json(state: &ServiceState, request: &HttpRequest) -> String {
    let artist_id = request.path.trim_start_matches("/api/artists/");
    let Some(artist) = state.find_artist(artist_id) else {
        return r#"{"error":"artist not found"}"#.to_string();
    };
    let albums = state.albums_for_artist(&artist.id);
    let albums_json = albums
        .into_iter()
        .map(|album| album_summary_json(&album))
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

fn render_album_artwork_candidates_json(state: &ServiceState, request: &HttpRequest) -> String {
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

fn render_renderers_json(state: &ServiceState) -> String {
    let renderers = state.enriched_renderer_snapshot();
    let selected = state
        .database
        .last_selected_renderer_location()
        .ok()
        .flatten();
    let entries = renderers
        .into_iter()
        .map(|renderer| {
            renderer_record_json(
                &renderer,
                selected.as_deref() == Some(renderer.location.as_str()),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{entries}]")
}

fn render_server_json(state: &ServiceState) -> String {
    format!(
        r#"{{"name":"{}","base_url":"{}","bind_address":"{}"}}"#,
        json_escape(&state.config.instance_name),
        json_escape(&state.config.resolved_base_url()),
        json_escape(&state.config.bind_address),
    )
}

fn render_metrics_text(state: &ServiceState) -> String {
    state.metrics().map(|m| m.encode()).unwrap_or_default()
}

fn render_now_playing_json(state: &ServiceState, request: &HttpRequest) -> String {
    let renderer_location =
        state.preferred_renderer_location(request_value(request, "renderer_location"));
    render_now_playing_json_for_renderer(state, &renderer_location)
}

fn render_now_playing_json_for_renderer(state: &ServiceState, renderer_location: &str) -> String {
    let renderer_json = state
        .enriched_renderer_record(&renderer_location)
        .map(|renderer| renderer_record_json(&renderer, true))
        .unwrap_or_else(|| "null".to_string());
    let current_track_json = current_track_json_for_renderer(state, &renderer_location);
    let session_json = session_payload_json_for_renderer(state, &renderer_location);
    let queue_summary_json = queue_summary_json_for_renderer(state, &renderer_location);
    format!(
        r#"{{"renderer_location":"{}","renderer":{},"current_track":{},"session":{},"queue_summary":{}}}"#,
        json_escape(&renderer_location),
        renderer_json,
        current_track_json,
        session_json,
        queue_summary_json,
    )
}

fn render_playback_event_json_for_renderer(
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

fn render_queue_json(state: &ServiceState, request: &HttpRequest) -> String {
    let renderer_location =
        state.preferred_renderer_location(request_value(request, "renderer_location"));
    render_queue_json_for_renderer(state, &renderer_location)
}

fn render_session_json(state: &ServiceState, request: &HttpRequest) -> String {
    let renderer_location =
        state.preferred_renderer_location(request_value(request, "renderer_location"));
    render_session_json_for_renderer(state, &renderer_location)
}

fn render_queue_json_for_renderer(state: &ServiceState, renderer_location: &str) -> String {
    let session = state.playback_session(&renderer_location);
    let session_json =
        |session: PlaybackSession| render_session_payload_json(state, renderer_location, session);
    let Some(queue) = state.queue_snapshot(&renderer_location) else {
        let session_json = session.map_or_else(|| "null".to_string(), session_json);
        return format!(
            r#"{{"renderer_location":"{}","status":"empty","entries":[],"session":{}}}"#,
            json_escape(&renderer_location),
            session_json,
        );
    };

    let entries = queue
        .entries
        .iter()
        .map(|entry| {
            let track = state.find_track(&entry.track_id);
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
                option_string_json(track.as_ref().map(|track| track.title.as_str())),
                option_string_json(track.as_ref().map(|track| track.artist.as_str())),
                option_string_json(track.as_ref().map(|track| track.album.as_str())),
                option_u64_json(track.and_then(|track| track.duration_seconds)),
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

fn render_session_json_for_renderer(state: &ServiceState, renderer_location: &str) -> String {
    let session_json = session_payload_json_for_renderer(state, renderer_location);
    format!(
        r#"{{"renderer_location":"{}","session":{}}}"#,
        json_escape(renderer_location),
        session_json,
    )
}

fn session_payload_json_for_renderer(state: &ServiceState, renderer_location: &str) -> String {
    state
        .playback_session(renderer_location)
        .map(|session| render_session_payload_json(state, renderer_location, session))
        .unwrap_or_else(|| "null".to_string())
}

fn render_session_payload_json(
    state: &ServiceState,
    renderer_location: &str,
    session: PlaybackSession,
) -> String {
    let current_track = current_track_for_renderer(state, renderer_location);
    format!(
        r#"{{"transport_state":"{}","queue_entry_id":{},"next_queue_entry_id":{},"current_track_uri":{},"position_seconds":{},"duration_seconds":{},"last_observed_unix":{},"last_error":{},"title":{},"artist":{},"album":{}}}"#,
        json_escape(&session.transport_state),
        option_i64_json(session.queue_entry_id),
        option_i64_json(session.next_queue_entry_id),
        option_string_json(session.current_track_uri.as_deref()),
        option_u64_json(session.position_seconds),
        option_u64_json(session.duration_seconds),
        session.last_observed_unix,
        option_string_json(session.last_error.as_deref()),
        option_string_json(current_track.as_ref().map(|track| track.title.as_str())),
        option_string_json(current_track.as_ref().map(|track| track.artist.as_str())),
        option_string_json(current_track.as_ref().map(|track| track.album.as_str())),
    )
}

fn render_discovery_json(state: &ServiceState) -> String {
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

fn current_track_for_renderer(
    state: &ServiceState,
    renderer_location: &str,
) -> Option<LibraryTrack> {
    let queue = state.queue_snapshot(renderer_location)?;
    let queue_entry_id = queue.current_entry_id?;
    let entry = queue
        .entries
        .into_iter()
        .find(|entry| entry.id == queue_entry_id)?;
    state.find_track(&entry.track_id)
}

fn current_track_json_for_renderer(state: &ServiceState, renderer_location: &str) -> String {
    current_track_for_renderer(state, renderer_location)
        .map(|track| {
            let fallback_artwork_url = state
                .find_album(&track.album_id)
                .and_then(|album| album.artwork_url);
            track_summary_json(&track, fallback_artwork_url.as_deref())
        })
        .unwrap_or_else(|| "null".to_string())
}

fn queue_summary_json_for_renderer(state: &ServiceState, renderer_location: &str) -> String {
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

fn track_summary_json(track: &LibraryTrack, fallback_album_artwork_url: Option<&str>) -> String {
    let artwork_url = if track.artwork.is_some() {
        Some(format!("/artwork/track/{}", track.id))
    } else {
        fallback_album_artwork_url.map(ToString::to_string)
    };
    format!(
        r#"{{"id":"{}","album_id":"{}","title":"{}","artist":"{}","album":"{}","disc_number":{},"track_number":{},"duration_seconds":{},"artwork_url":{},"mime_type":"{}"}}"#,
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
    )
}

fn renderer_record_json(renderer: &RendererRecord, selected: bool) -> String {
    let av_transport_actions_json = renderer
        .capabilities
        .av_transport_actions
        .as_ref()
        .map(|actions| string_list_json(actions));
    format!(
        r#"{{"location":"{}","name":"{}","manufacturer":{},"model_name":{},"av_transport_control_url":{},"capabilities":{{"av_transport_actions":{},"supports_set_next_av_transport_uri":{},"supports_pause":{},"supports_stop":{},"supports_next":{},"supports_previous":{},"supports_seek":{},"has_playlist_extension_service":{}}},"health":{{"last_checked_unix":{},"last_reachable_unix":{},"last_error":{},"reachable":{}}},"last_seen_unix":{},"selected":{},"kind":"{}"}}"#,
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
    )
}




fn print_status() {
    let config = AppConfig::from_env();
    println!("musicd service scaffold");
    println!();
    println!("Library path: {}", config.library_path.display());
    println!("Config path: {}", config.config_path.display());
    println!("Bind address: {}", config.bind_address);
    println!("HTTP base URL: {}", config.resolved_base_url());
    println!("Discovery timeout: {}ms", config.discovery_timeout_ms);
    println!(
        "Debug mode: {}",
        if config.debug_mode {
            "enabled"
        } else {
            "disabled"
        }
    );
    if let Some(renderer) = config.default_renderer_location {
        println!("Default renderer: {renderer}");
    }
    println!();
    println!("Commands:");
    println!("- serve");
    println!("- discover [timeout_ms]");
    println!("- inspect <renderer_location_url>");
    println!("- play-url <renderer_location_url> <stream_url> [title]");
    println!("- serve-file <audio_file_path> [bind_addr]");
    println!(
        "- play-file <renderer_location_url> <audio_file_path> <bind_addr> <public_base_url> [title]"
    );
}

fn print_help() {
    println!("musicd");
    println!();
    println!("Commands:");
    println!("  serve");
    println!("    Scan the library and run the long-lived browser UI and stream service.");
    println!();
    println!("  status");
    println!("    Show the current scaffold status and command summary.");
    println!();
    println!("  discover [timeout_ms]");
    println!("    Send SSDP M-SEARCH and print discovered UPnP media renderers.");
    println!();
    println!("  inspect <renderer_location_url>");
    println!("    Fetch the device description and print AVTransport details.");
    println!();
    println!("  play-url <renderer_location_url> <stream_url> [title]");
    println!("    Tell the renderer to play an existing LAN-accessible stream URL.");
    println!();
    println!("  serve-file <audio_file_path> [bind_addr]");
    println!("    Serve one local audio file at /stream/current.");
    println!();
    println!(
        "  play-file <renderer_location_url> <audio_file_path> <bind_addr> <public_base_url> [title]"
    );
    println!("    Serve a local file and immediately send SetAVTransportURI + Play.");
}

fn required_arg(value: Option<String>, label: &str) -> io::Result<String> {
    value.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("missing {label}")))
}

fn inferred_title(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("musicd track");
    cleanup_track_label(stem)
}


#[cfg(test)]
mod tests {
    use super::{
        LibraryTrack, PlaybackQueue, PlaybackSession, QueueEntry, QueueMutationEntry,
        RendererBackends, RendererKind, ServiceState, cleanup_track_label, infer_artist_and_album,
        infer_disc_and_track_numbers, next_queue_entry_after, previous_queue_entry_before,
        queue_status_for_transport, renderer_is_viable, renderer_kind_for_location,
        should_adopt_preloaded_next_entry, should_auto_advance, should_skip_entry,
    };
    use crate::artwork::{artwork_name_priority, infer_image_mime_from_bytes};
    use crate::db::Database;
    use crate::http::{parse_query_string, parse_range_header, parse_request_form};
    use crate::ids::{stable_album_id, stable_artist_id, stable_track_id};
    use crate::library::{
        build_artist_summaries, compare_track_album_order, decode_id3v1_text,
        parse_vorbis_comment_block,
    };
    use std::sync::OnceLock;
    use musicd_core::AppConfig;
    use musicd_upnp::{PositionInfo, RendererCapabilities, TransportInfo, TransportSnapshot};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_standard_http_ranges() {
        assert_eq!(parse_range_header("bytes=100-199", 1000), Some((100, 199)));
        assert_eq!(parse_range_header("bytes=100-", 1000), Some((100, 999)));
        assert_eq!(parse_range_header("bytes=-100", 1000), Some((900, 999)));
    }

    #[test]
    fn rejects_invalid_ranges() {
        assert_eq!(parse_range_header("items=100-200", 1000), None);
        assert_eq!(parse_range_header("bytes=500-200", 1000), None);
        assert_eq!(parse_range_header("bytes=100-2000", 1000), None);
    }

    #[test]
    fn query_parser_decodes_renderer_locations() {
        let parsed = parse_query_string(
            "renderer_location=http%3A%2F%2F192.168.1.55%3A49152%2Fdescription.xml&message=Now+playing",
        );
        assert_eq!(
            parsed.get("renderer_location").map(String::as_str),
            Some("http://192.168.1.55:49152/description.xml")
        );
        assert_eq!(
            parsed.get("message").map(String::as_str),
            Some("Now playing")
        );
    }

    #[test]
    fn form_parser_decodes_urlencoded_bodies() {
        let parsed = parse_request_form(
            Some("application/x-www-form-urlencoded; charset=utf-8"),
            b"renderer_location=http%3A%2F%2F192.168.1.55%3A49152%2Fdescription.xml&track_id=abc123",
        );
        assert_eq!(
            parsed.get("renderer_location").map(String::as_str),
            Some("http://192.168.1.55:49152/description.xml")
        );
        assert_eq!(parsed.get("track_id").map(String::as_str), Some("abc123"));
    }

    #[test]
    fn infers_renderer_kind_from_location() {
        assert_eq!(
            renderer_kind_for_location("http://192.168.1.55:49152/description.xml"),
            RendererKind::Upnp
        );
        assert_eq!(
            renderer_kind_for_location("sonos:RINCON_1234567890"),
            RendererKind::Sonos
        );
    }

    #[test]
    fn stable_track_ids_are_repeatable() {
        let left = stable_track_id("Artist/Album/01 - Track.flac");
        let right = stable_track_id("Artist/Album/01 - Track.flac");
        assert_eq!(left, right);
    }

    #[test]
    fn cleanup_track_label_strips_common_number_prefixes() {
        assert_eq!(cleanup_track_label("01 - Example_Track"), "Example Track");
        assert_eq!(cleanup_track_label("1. Intro"), "Intro");
    }

    #[test]
    fn infers_artist_and_album_from_relative_components() {
        let (artist, album) = infer_artist_and_album(&[
            "Boards of Canada".to_string(),
            "Music Has the Right to Children".to_string(),
            "01 - Wildlife Analysis.flac".to_string(),
        ]);
        assert_eq!(artist, "Boards of Canada");
        assert_eq!(album, "Music Has the Right to Children");

        let (artist, album) = infer_artist_and_album(&[
            "Biosphere".to_string(),
            "Substrata".to_string(),
            "Disc 1".to_string(),
            "01 - As the Sun Kissed the Horizon.flac".to_string(),
        ]);
        assert_eq!(artist, "Biosphere");
        assert_eq!(album, "Substrata");
    }

    #[test]
    fn infers_disc_and_track_numbers_from_paths() {
        let (disc, track) = infer_disc_and_track_numbers(&[
            "Biosphere".to_string(),
            "Substrata".to_string(),
            "Disc 2".to_string(),
            "03 - Chukhung.flac".to_string(),
        ]);
        assert_eq!(disc, Some(2));
        assert_eq!(track, Some(3));

        let (disc, track) = infer_disc_and_track_numbers(&[
            "Album".to_string(),
            "Track Without Prefix.flac".to_string(),
        ]);
        assert_eq!(disc, None);
        assert_eq!(track, None);
    }

    #[test]
    fn skips_hidden_metadata_entries() {
        assert!(should_skip_entry(".AppleDouble"));
        assert!(should_skip_entry("._Track.flac"));
        assert!(should_skip_entry("@eaDir"));
        assert!(!should_skip_entry("Track.flac"));
    }

    #[test]
    fn parses_vorbis_comment_block_fields() {
        let mut block = Vec::new();
        block.extend_from_slice(&5_u32.to_le_bytes());
        block.extend_from_slice(b"music");
        block.extend_from_slice(&2_u32.to_le_bytes());

        let title = b"TITLE=Roygbiv";
        block.extend_from_slice(&(title.len() as u32).to_le_bytes());
        block.extend_from_slice(title);

        let artist = b"ARTIST=Boards of Canada";
        block.extend_from_slice(&(artist.len() as u32).to_le_bytes());
        block.extend_from_slice(artist);

        let (fields, notes) = parse_vorbis_comment_block(&block);
        assert!(notes.is_empty());
        assert!(fields.contains(&(String::from("VENDOR"), String::from("music"))));
        assert!(fields.contains(&(String::from("TITLE"), String::from("Roygbiv"))));
        assert!(fields.contains(&(String::from("ARTIST"), String::from("Boards of Canada"))));
    }

    #[test]
    fn decodes_id3v1_text() {
        let bytes = b"Example Track\x00\x00\x00";
        assert_eq!(decode_id3v1_text(bytes), "Example Track");
    }

    #[test]
    fn stable_album_ids_are_repeatable() {
        let left = stable_album_id("Boards of Canada", "Music Has the Right to Children");
        let right = stable_album_id("boards of canada", "music has the right to children");
        assert_eq!(left, right);
    }

    #[test]
    fn track_album_order_prefers_numeric_positions() {
        let mut tracks = vec![
            sample_track("c", Some(1), Some(3), "Track 3"),
            sample_track("a", Some(1), Some(1), "Track 1"),
            sample_track("b", Some(1), Some(2), "Track 2"),
        ];
        tracks.sort_by(compare_track_album_order);
        let ordered_ids = tracks.into_iter().map(|track| track.id).collect::<Vec<_>>();
        assert_eq!(ordered_ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn queue_replace_and_append_round_trip() {
        let config_path = temp_config_path("queue-round-trip");
        let database = Database::open(&config_path).expect("database should open");

        let replaced = database
            .replace_queue(
                "http://renderer.local/description.xml",
                "Album Queue",
                &[QueueMutationEntry {
                    track_id: "track-1".to_string(),
                    album_id: Some("album-1".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album-1".to_string()),
                }],
            )
            .expect("queue replace should succeed");
        assert_eq!(replaced.entries.len(), 1);
        assert_eq!(replaced.current_entry_id, Some(replaced.entries[0].id));

        let appended = database
            .append_queue_entries(
                "http://renderer.local/description.xml",
                "Album Queue",
                &[QueueMutationEntry {
                    track_id: "track-2".to_string(),
                    album_id: Some("album-1".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album-1".to_string()),
                }],
            )
            .expect("queue append should succeed");
        assert_eq!(appended.entries.len(), 2);
        assert_eq!(appended.entries[0].track_id, "track-1");
        assert_eq!(appended.entries[1].track_id, "track-2");

        let _ = std::fs::remove_dir_all(config_path);
    }

    #[test]
    fn queue_insert_move_and_remove_round_trip() {
        let config_path = temp_config_path("queue-mutations");
        let database = Database::open(&config_path).expect("database should open");

        let initial = database
            .replace_queue(
                "http://renderer.local/description.xml",
                "Album Queue",
                &[
                    QueueMutationEntry {
                        track_id: "track-1".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                    QueueMutationEntry {
                        track_id: "track-2".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                    QueueMutationEntry {
                        track_id: "track-3".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                ],
            )
            .expect("queue replace should succeed");

        let inserted = database
            .insert_queue_entries_after_current(
                "http://renderer.local/description.xml",
                "Album Queue",
                &[QueueMutationEntry {
                    track_id: "track-x".to_string(),
                    album_id: Some("album-2".to_string()),
                    source_kind: "track".to_string(),
                    source_ref: Some("track-x".to_string()),
                }],
            )
            .expect("queue insert should succeed");
        assert_eq!(
            inserted
                .entries
                .iter()
                .map(|entry| entry.track_id.as_str())
                .collect::<Vec<_>>(),
            vec!["track-1", "track-x", "track-2", "track-3"]
        );

        let moved = database
            .move_queue_entry(
                "http://renderer.local/description.xml",
                inserted.entries[3].id,
                -1,
            )
            .expect("queue move should succeed");
        assert_eq!(
            moved
                .entries
                .iter()
                .map(|entry| entry.track_id.as_str())
                .collect::<Vec<_>>(),
            vec!["track-1", "track-x", "track-3", "track-2"]
        );

        let removed = database
            .remove_queue_entry("http://renderer.local/description.xml", moved.entries[1].id)
            .expect("queue remove should succeed");
        assert_eq!(removed.current_entry_id, initial.current_entry_id);
        assert_eq!(
            removed
                .entries
                .iter()
                .map(|entry| entry.track_id.as_str())
                .collect::<Vec<_>>(),
            vec!["track-1", "track-3", "track-2"]
        );

        let _ = std::fs::remove_dir_all(config_path);
    }

    #[test]
    fn auto_advance_requires_stop_near_end() {
        let state = sample_state(vec![sample_track("track-1", Some(1), Some(1), "Track 1")]);
        let queue = PlaybackQueue {
            renderer_location: "http://renderer.local/description.xml".to_string(),
            name: "Queue".to_string(),
            current_entry_id: Some(1),
            status: "playing".to_string(),
            version: 1,
            updated_unix: 0,
            entries: vec![QueueEntry {
                id: 1,
                position: 1,
                track_id: "track-1".to_string(),
                album_id: Some("album".to_string()),
                source_kind: "track".to_string(),
                source_ref: Some("track-1".to_string()),
                entry_status: "playing".to_string(),
                started_unix: Some(1),
                completed_unix: None,
            }],
        };
        let session = PlaybackSession {
            renderer_location: queue.renderer_location.clone(),
            queue_entry_id: Some(1),
            next_queue_entry_id: None,
            transport_state: "PLAYING".to_string(),
            current_track_uri: Some("http://musicd.local/stream/track/track-1".to_string()),
            position_seconds: Some(179),
            duration_seconds: Some(180),
            last_observed_unix: 1,
            last_error: None,
        };
        let snapshot = TransportSnapshot {
            transport_info: TransportInfo {
                transport_state: "STOPPED".to_string(),
                transport_status: Some("OK".to_string()),
                current_speed: Some("1".to_string()),
            },
            position_info: PositionInfo {
                track_uri: Some("http://musicd.local/stream/track/track-1".to_string()),
                rel_time_seconds: Some(179),
                track_duration_seconds: Some(180),
            },
        };
        assert!(should_auto_advance(
            &queue,
            Some(&session),
            &snapshot,
            &state
        ));

        let early_session = PlaybackSession {
            position_seconds: Some(40),
            ..session
        };
        let early_snapshot = TransportSnapshot {
            transport_info: snapshot.transport_info.clone(),
            position_info: PositionInfo {
                track_uri: snapshot.position_info.track_uri.clone(),
                rel_time_seconds: Some(40),
                track_duration_seconds: snapshot.position_info.track_duration_seconds,
            },
        };
        assert!(!should_auto_advance(
            &queue,
            Some(&early_session),
            &early_snapshot,
            &state
        ));

        let paused_session = PlaybackSession {
            transport_state: "PAUSED_PLAYBACK".to_string(),
            position_seconds: Some(179),
            ..early_session
        };
        let stopped_snapshot = TransportSnapshot {
            transport_info: TransportInfo {
                transport_state: "STOPPED".to_string(),
                transport_status: Some("OK".to_string()),
                current_speed: Some("1".to_string()),
            },
            position_info: PositionInfo {
                track_uri: Some("http://musicd.local/stream/track/track-1".to_string()),
                rel_time_seconds: Some(179),
                track_duration_seconds: Some(180),
            },
        };
        assert!(!should_auto_advance(
            &queue,
            Some(&paused_session),
            &stopped_snapshot,
            &state
        ));
    }

    #[test]
    fn next_queue_entry_uses_queue_order() {
        let queue = PlaybackQueue {
            renderer_location: "http://renderer.local/description.xml".to_string(),
            name: "Queue".to_string(),
            current_entry_id: Some(2),
            status: "playing".to_string(),
            version: 1,
            updated_unix: 0,
            entries: vec![
                QueueEntry {
                    id: 10,
                    position: 1,
                    track_id: "track-1".to_string(),
                    album_id: Some("album".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album".to_string()),
                    entry_status: "completed".to_string(),
                    started_unix: Some(1),
                    completed_unix: Some(2),
                },
                QueueEntry {
                    id: 20,
                    position: 2,
                    track_id: "track-2".to_string(),
                    album_id: Some("album".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album".to_string()),
                    entry_status: "playing".to_string(),
                    started_unix: Some(3),
                    completed_unix: None,
                },
                QueueEntry {
                    id: 30,
                    position: 3,
                    track_id: "track-3".to_string(),
                    album_id: Some("album".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album".to_string()),
                    entry_status: "pending".to_string(),
                    started_unix: None,
                    completed_unix: None,
                },
            ],
        };

        let next = next_queue_entry_after(&queue, 20).expect("next queue entry should exist");
        assert_eq!(next.id, 30);
        assert!(next_queue_entry_after(&queue, 30).is_none());
        let previous =
            previous_queue_entry_before(&queue, 20).expect("previous queue entry should exist");
        assert_eq!(previous.id, 10);
        assert!(previous_queue_entry_before(&queue, 10).is_none());
    }

    #[test]
    fn adopts_preloaded_next_entry_when_renderer_reports_next_uri() {
        let queue = PlaybackQueue {
            renderer_location: "http://renderer.local/description.xml".to_string(),
            name: "Queue".to_string(),
            current_entry_id: Some(20),
            status: "playing".to_string(),
            version: 1,
            updated_unix: 0,
            entries: vec![
                QueueEntry {
                    id: 20,
                    position: 1,
                    track_id: "track-1".to_string(),
                    album_id: Some("album".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album".to_string()),
                    entry_status: "playing".to_string(),
                    started_unix: Some(1),
                    completed_unix: None,
                },
                QueueEntry {
                    id: 30,
                    position: 2,
                    track_id: "track-2".to_string(),
                    album_id: Some("album".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album".to_string()),
                    entry_status: "pending".to_string(),
                    started_unix: None,
                    completed_unix: None,
                },
            ],
        };
        let snapshot = TransportSnapshot {
            transport_info: TransportInfo {
                transport_state: "PLAYING".to_string(),
                transport_status: Some("OK".to_string()),
                current_speed: Some("1".to_string()),
            },
            position_info: PositionInfo {
                track_uri: Some("http://musicd.local/stream/track/track-2".to_string()),
                rel_time_seconds: Some(1),
                track_duration_seconds: Some(180),
            },
        };

        assert!(should_adopt_preloaded_next_entry(
            &queue,
            &snapshot,
            Some("http://musicd.local/stream/track/track-2")
        ));
        assert!(!should_adopt_preloaded_next_entry(
            &queue,
            &snapshot,
            Some("http://musicd.local/stream/track/track-3")
        ));
    }

    #[test]
    fn queue_status_follows_transport_state() {
        assert_eq!(queue_status_for_transport("PLAYING"), "playing");
        assert_eq!(queue_status_for_transport("TRANSITIONING"), "playing");
        assert_eq!(queue_status_for_transport("PAUSED_PLAYBACK"), "paused");
        assert_eq!(queue_status_for_transport("STOPPED"), "stopped");
    }

    #[test]
    fn prioritizes_cover_art_names() {
        assert!(
            artwork_name_priority("cover.jpg") < artwork_name_priority("folder.jpg"),
            "cover.jpg should outrank folder.jpg"
        );
        assert!(
            artwork_name_priority("folder.jpg") < artwork_name_priority("front.png"),
            "folder.jpg should outrank front.png"
        );
        assert_eq!(artwork_name_priority("booklet.jpg"), None);
    }

    #[test]
    fn detects_common_artwork_signatures() {
        assert_eq!(
            infer_image_mime_from_bytes(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0]),
            Some("image/jpeg")
        );
        assert_eq!(
            infer_image_mime_from_bytes(b"\x89PNG\r\n\x1a\nrest"),
            Some("image/png")
        );
        assert_eq!(
            infer_image_mime_from_bytes(b"RIFFxxxxWEBPrest"),
            Some("image/webp")
        );
        assert_eq!(infer_image_mime_from_bytes(b"not an image"), None);
    }

    #[test]
    fn merges_artists_with_same_normalized_name() {
        let mut first = sample_track("track-1", Some(1), Some(1), "Song A");
        first.artist = "Radiohead".to_string();
        first.album = "In Rainbows".to_string();
        first.album_id = stable_album_id(&first.artist, &first.album);

        let mut second = sample_track("track-2", Some(1), Some(1), "Song B");
        second.artist = " radiohead ".to_string();
        second.album = "Kid A".to_string();
        second.album_id = stable_album_id(&second.artist, &second.album);

        let artists = build_artist_summaries(&[first, second]);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].track_count, 2);
        assert_eq!(artists[0].album_count, 2);
        assert_eq!(artists[0].id, stable_artist_id("Radiohead"));
    }

    #[test]
    fn records_track_play_history_per_started_entry() {
        let config_path = temp_config_path("track-play-history");
        let database = Database::open(&config_path).expect("database should open");
        let queue = database
            .replace_queue(
                "renderer-1",
                "Test Queue",
                &[
                    QueueMutationEntry {
                        track_id: "track-1".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                    QueueMutationEntry {
                        track_id: "track-2".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                ],
            )
            .expect("queue should be created");

        let first_entry = queue.entries.first().expect("first entry").id;
        let second_entry = queue.entries.get(1).expect("second entry").id;

        database
            .mark_queue_play_started(
                "renderer-1",
                first_entry,
                "track-1",
                "http://musicd.local/stream/track/track-1",
                Some(180),
            )
            .expect("first play should be recorded");
        database
            .adopt_next_queue_entry_as_current(
                "renderer-1",
                second_entry,
                "track-2",
                "http://musicd.local/stream/track/track-2",
                Some(200),
            )
            .expect("second play should be recorded");

        assert_eq!(database.count_track_plays("track-1").unwrap_or(0), 1);
        assert_eq!(database.count_track_plays("track-2").unwrap_or(0), 1);

        let history = database
            .load_track_play_history("track-2")
            .expect("history should load");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].track_id, "track-2");
        assert_eq!(history[0].renderer_location, "renderer-1");
        assert_eq!(history[0].queue_entry_id, Some(second_entry));
    }

    #[test]
    fn persists_normalized_albums_and_artists() {
        let config_path = temp_config_path("normalized-library");
        let database = Database::open(&config_path).expect("database should open");

        let mut first = sample_track("track-1", Some(1), Some(1), "15 Step");
        first.artist = "Radiohead".to_string();
        first.album = "In Rainbows".to_string();
        first.album_id = stable_album_id(&first.artist, &first.album);
        first.artwork = Some(super::TrackArtwork {
            cache_key: "cover.jpg".to_string(),
            source: "Embedded artwork".to_string(),
            mime_type: "image/jpeg".to_string(),
        });

        let mut second = sample_track("track-2", Some(1), Some(2), "Bodysnatchers");
        second.artist = "Radiohead".to_string();
        second.album = "In Rainbows".to_string();
        second.album_id = stable_album_id(&second.artist, &second.album);

        let mut third = sample_track("track-3", Some(1), Some(1), "Everything In Its Right Place");
        third.artist = "Radiohead".to_string();
        third.album = "Kid A".to_string();
        third.album_id = stable_album_id(&third.artist, &third.album);

        let library = super::Library::build(
            PathBuf::from("/music"),
            vec![first.clone(), second.clone(), third.clone()],
            &[],
        );

        database
            .save_library(&library)
            .expect("library should be persisted");

        let albums = database.load_albums().expect("albums should load");
        assert_eq!(albums.len(), 2);
        let in_rainbows = albums
            .iter()
            .find(|album| album.id == stable_album_id("Radiohead", "In Rainbows"))
            .expect("in rainbows album should exist");
        let expected_artwork_url = format!(
            "/artwork/album/{}",
            stable_album_id("Radiohead", "In Rainbows")
        );
        assert_eq!(in_rainbows.artist_id, stable_artist_id("Radiohead"));
        assert_eq!(in_rainbows.track_count, 2);
        assert_eq!(in_rainbows.first_track_id, "track-1");
        assert_eq!(
            in_rainbows.artwork_url.as_deref(),
            Some(expected_artwork_url.as_str())
        );
        assert_eq!(
            in_rainbows
                .artwork
                .as_ref()
                .map(|artwork| artwork.cache_key.as_str()),
            Some("cover.jpg")
        );

        let artists = database.load_artists().expect("artists should load");
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].id, stable_artist_id("Radiohead"));
        assert_eq!(artists[0].album_count, 2);
        assert_eq!(artists[0].track_count, 3);
        assert_eq!(
            artists[0].first_album_id,
            stable_album_id("Radiohead", "In Rainbows")
        );
    }

    #[test]
    fn persists_renderer_capabilities_and_health() {
        let config_path = temp_config_path("renderer-capabilities");
        let database = Database::open(&config_path).expect("database should open");

        database
            .upsert_renderer(&super::RendererRecord {
                location: "http://192.168.1.55:49152/description.xml".to_string(),
                name: "CXN V2".to_string(),
                manufacturer: Some("Cambridge Audio".to_string()),
                model_name: Some("CXN V2".to_string()),
                av_transport_control_url: Some(
                    "http://192.168.1.55:49152/upnp/control/avtransport1".to_string(),
                ),
                capabilities: RendererCapabilities {
                    av_transport_actions: Some(vec![
                        "Next".to_string(),
                        "Pause".to_string(),
                        "SetNextAVTransportURI".to_string(),
                    ]),
                    has_playlist_extension_service: Some(true),
                },
                last_checked_unix: 100,
                last_reachable_unix: Some(95),
                last_error: Some("timed out".to_string()),
                last_seen_unix: 95,
            })
            .expect("renderer should persist");

        let renderer = database
            .load_renderer("http://192.168.1.55:49152/description.xml")
            .expect("renderer should load")
            .expect("renderer record should exist");
        assert_eq!(renderer.name, "CXN V2");
        assert_eq!(
            renderer.capabilities.supports_set_next_av_transport_uri(),
            Some(true)
        );
        assert_eq!(renderer.capabilities.supports_pause(), Some(true));
        assert_eq!(renderer.capabilities.supports_previous(), Some(false));
        assert_eq!(
            renderer.capabilities.has_playlist_extension_service,
            Some(true)
        );
        assert_eq!(renderer.last_checked_unix, 100);
        assert_eq!(renderer.last_reachable_unix, Some(95));
        assert_eq!(renderer.last_error.as_deref(), Some("timed out"));
        assert_eq!(renderer.last_seen_unix, 95);
    }

    #[test]
    fn renderer_refresh_targets_incomplete_upnp_records() {
        let complete = super::RendererRecord {
            location: "http://192.168.1.55:49152/description.xml".to_string(),
            name: "CXN V2".to_string(),
            manufacturer: Some("Cambridge Audio".to_string()),
            model_name: Some("CXN V2".to_string()),
            av_transport_control_url: Some("http://renderer/avtransport".to_string()),
            capabilities: RendererCapabilities {
                av_transport_actions: Some(vec!["Pause".to_string()]),
                has_playlist_extension_service: Some(true),
            },
            last_checked_unix: 100,
            last_reachable_unix: Some(100),
            last_error: None,
            last_seen_unix: 100,
        };
        assert!(!super::renderer_needs_refresh(&complete));

        let mut missing_actions = complete.clone();
        missing_actions.capabilities.av_transport_actions = None;
        assert!(super::renderer_needs_refresh(&missing_actions));

        let mut fallback_name = complete.clone();
        fallback_name.name = fallback_name.location.clone();
        assert!(super::renderer_needs_refresh(&fallback_name));
    }

    #[test]
    fn rejects_non_playable_upnp_renderer_records() {
        let invalid = super::RendererRecord {
            location: "http://192.168.1.173:80/description.xml".to_string(),
            name: "Hue Bridge".to_string(),
            manufacturer: Some("Signify".to_string()),
            model_name: Some("Philips hue bridge 2015".to_string()),
            av_transport_control_url: None,
            capabilities: RendererCapabilities::default(),
            last_checked_unix: 10,
            last_reachable_unix: Some(10),
            last_error: None,
            last_seen_unix: 10,
        };
        assert!(!renderer_is_viable(&invalid));

        let mut valid = invalid.clone();
        valid.av_transport_control_url = Some("http://renderer/avtransport".to_string());
        assert!(renderer_is_viable(&valid));
    }

    fn sample_track(
        id: &str,
        disc_number: Option<u32>,
        track_number: Option<u32>,
        title: &str,
    ) -> LibraryTrack {
        LibraryTrack {
            id: id.to_string(),
            album_id: "album".to_string(),
            title: title.to_string(),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            disc_number,
            track_number,
            duration_seconds: Some(180),
            relative_path: format!("{title}.flac"),
            path: PathBuf::from(format!("/music/{title}.flac")),
            mime_type: "audio/flac".to_string(),
            file_size: 123,
            artwork: None,
        }
    }

    fn sample_state(tracks: Vec<LibraryTrack>) -> ServiceState {
        let config_path = temp_config_path("service-state");
        let database = Database::open(&config_path).expect("database should open");
        ServiceState {
            config: AppConfig {
                instance_name: "musicd".to_string(),
                library_path: PathBuf::from("/music"),
                config_path,
                bind_address: "0.0.0.0:7878".to_string(),
                base_url: "http://192.168.1.10:7878".to_string(),
                discovery_timeout_ms: 1500,
                default_renderer_location: None,
                debug_mode: false,
            },
            database,
            library: arc_swap::ArcSwap::from_pointee(super::Library::build(
                PathBuf::from("/music"),
                tracks,
                &[],
            )),
            renderer_backends: RendererBackends::default(),
            metrics: OnceLock::new(),
        }
    }

    fn temp_config_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        std::env::temp_dir().join(format!("musicd-{label}-{unique}"))
    }
}
