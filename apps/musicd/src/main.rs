use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::read_from_path;
use lofty::tag::Accessor;
use musicd_core::AppConfig;
use musicd_upnp::{
    RendererCapabilities, StreamResource, TransportSnapshot, discover_renderers,
    get_transport_snapshot, inspect_renderer, next, pause, play, play_stream, previous,
    set_av_transport_uri, set_next_av_transport_uri, stop,
};
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;

mod metrics;

type SqlitePool = Pool<SqliteConnectionManager>;
type SqliteConn = PooledConnection<SqliteConnectionManager>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LibraryTrack {
    id: String,
    album_id: String,
    title: String,
    artist: String,
    album: String,
    disc_number: Option<u32>,
    track_number: Option<u32>,
    duration_seconds: Option<u64>,
    relative_path: String,
    path: PathBuf,
    mime_type: String,
    file_size: u64,
    artwork: Option<TrackArtwork>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AlbumSummary {
    id: String,
    artist_id: String,
    title: String,
    artist: String,
    track_count: usize,
    artwork_track_id: Option<String>,
    artwork: Option<TrackArtwork>,
    artwork_url: Option<String>,
    first_track_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArtistSummary {
    id: String,
    name: String,
    album_count: usize,
    track_count: usize,
    artwork_track_id: Option<String>,
    artwork_url: Option<String>,
    first_album_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackArtwork {
    cache_key: String,
    source: String,
    mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AlbumArtworkOverride {
    album_id: String,
    cache_key: String,
    source: String,
    mime_type: String,
    musicbrainz_release_id: Option<String>,
    applied_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AlbumArtworkSearchCandidate {
    release_id: String,
    release_group_id: Option<String>,
    title: String,
    artist: String,
    date: Option<String>,
    country: Option<String>,
    score: i32,
    thumbnail_url: String,
    image_url: String,
    source: String,
}

#[derive(Debug, Deserialize)]
struct MusicBrainzSearchResponse {
    #[serde(default)]
    releases: Vec<MusicBrainzSearchRelease>,
}

#[derive(Debug, Deserialize)]
struct MusicBrainzSearchRelease {
    id: String,
    title: String,
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    score: Option<i32>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<MusicBrainzArtistCredit>,
    #[serde(rename = "release-group", default)]
    release_group: Option<MusicBrainzReleaseGroupRef>,
}

#[derive(Debug, Deserialize)]
struct MusicBrainzArtistCredit {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MusicBrainzReleaseGroupRef {
    id: String,
}

#[derive(Debug, Deserialize)]
struct CoverArtArchiveResponse {
    #[serde(default)]
    images: Vec<CoverArtArchiveImage>,
    #[serde(default)]
    release: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CoverArtArchiveImage {
    #[serde(default)]
    front: bool,
    #[serde(default)]
    approved: bool,
    image: String,
    #[serde(default)]
    thumbnails: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EmbeddedMetadata {
    format_name: String,
    fields: Vec<(String, String)>,
    notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ParsedTrackTags {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    disc_number: Option<u32>,
    track_number: Option<u32>,
    duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RendererRecord {
    location: String,
    name: String,
    manufacturer: Option<String>,
    model_name: Option<String>,
    av_transport_control_url: Option<String>,
    capabilities: RendererCapabilities,
    last_checked_unix: i64,
    last_reachable_unix: Option<i64>,
    last_error: Option<String>,
    last_seen_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaybackQueue {
    renderer_location: String,
    name: String,
    current_entry_id: Option<i64>,
    status: String,
    version: i64,
    updated_unix: i64,
    entries: Vec<QueueEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueueEntry {
    id: i64,
    position: i64,
    track_id: String,
    album_id: Option<String>,
    source_kind: String,
    source_ref: Option<String>,
    entry_status: String,
    started_unix: Option<i64>,
    completed_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaybackSession {
    renderer_location: String,
    queue_entry_id: Option<i64>,
    next_queue_entry_id: Option<i64>,
    transport_state: String,
    current_track_uri: Option<String>,
    position_seconds: Option<u64>,
    duration_seconds: Option<u64>,
    last_observed_unix: i64,
    last_error: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackPlayRecord {
    id: i64,
    track_id: String,
    renderer_location: String,
    queue_entry_id: Option<i64>,
    played_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueueMutationEntry {
    track_id: String,
    album_id: Option<String>,
    source_kind: String,
    source_ref: Option<String>,
}

#[derive(Debug)]
struct Database {
    pool: SqlitePool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Library {
    scan_root: PathBuf,
    tracks: Vec<LibraryTrack>,
}

#[derive(Debug)]
struct ServiceState {
    config: AppConfig,
    database: Database,
    library: Mutex<Library>,
    renderer_backends: RendererBackends,
    metrics: OnceLock<Arc<metrics::Metrics>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpRequest {
    method: String,
    target: String,
    path: String,
    query: HashMap<String, String>,
    form: HashMap<String, String>,
    range_header: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

#[derive(Debug, Clone)]
enum ServerMode {
    SingleFile(Arc<PathBuf>),
    Service(Arc<ServiceState>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RendererKind {
    Upnp,
    Sonos,
    AndroidLocal,
}

#[derive(Debug, Default)]
struct RendererBackends {
    upnp: UpnpRendererBackend,
    android_local: AndroidLocalRendererBackend,
}

#[derive(Debug, Default)]
struct UpnpRendererBackend;

#[derive(Debug, Default)]
struct AndroidLocalRendererBackend;

trait RendererBackend: Send + Sync {
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
    fn backend_for_location(&self, renderer_location: &str) -> io::Result<&dyn RendererBackend> {
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

fn renderer_kind_for_location(renderer_location: &str) -> RendererKind {
    if renderer_location.starts_with("android-local://") {
        RendererKind::AndroidLocal
    } else if renderer_location.starts_with("sonos:") {
        RendererKind::Sonos
    } else {
        RendererKind::Upnp
    }
}

fn android_local_renderer_capabilities() -> RendererCapabilities {
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

fn upnp_control_url(renderer: &RendererRecord) -> io::Result<&str> {
    renderer
        .av_transport_control_url
        .as_deref()
        .ok_or_else(|| io::Error::other("renderer is missing an AVTransport control URL"))
}

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

fn spawn_queue_worker(state: Arc<ServiceState>) {
    thread::spawn(move || {
        loop {
            if let Err(error) = state.poll_active_queues() {
                eprintln!("queue worker error: {error}");
            }
            thread::sleep(Duration::from_secs(2));
        }
    });
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

fn serve_tcp(bind_address: &str, mode: ServerMode) -> io::Result<()> {
    let listener = TcpListener::bind(bind_address)?;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let mode = mode.clone();
                thread::spawn(move || {
                    if let Err(error) = handle_client(stream, mode) {
                        if !is_expected_client_disconnect(&error) {
                            eprintln!("request failed: {error}");
                        }
                    }
                });
            }
            Err(error) => eprintln!("accept failed: {error}"),
        }
    }
    Ok(())
}

fn handle_client(stream: TcpStream, mode: ServerMode) -> io::Result<()> {
    let peer = stream.peer_addr().ok();
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    let request = match read_http_request(&mut reader)? {
        Some(request) => request,
        None => return Ok(()),
    };

    if let Some(peer) = peer {
        eprintln!("{peer} -> {} {}", request.method, request.target);
    } else {
        eprintln!("unknown-peer -> {} {}", request.method, request.target);
    }

    metrics::take_response_status();
    let start = Instant::now();

    let result = match &mode {
        ServerMode::SingleFile(path) => {
            handle_single_file_request(&mut writer, &request, Arc::clone(path))
        }
        ServerMode::Service(state) => {
            handle_service_request(&mut writer, &request, Arc::clone(state))
        }
    };

    if let ServerMode::Service(state) = &mode {
        if let Some(metrics) = state.metrics() {
            let status = metrics::take_response_status();
            if status != 0 {
                let route = metrics::route_template(&request.path);
                metrics.record_request(&request.method, &route, status, start.elapsed());
            }
        }
    }

    result
}

fn handle_single_file_request(
    writer: &mut TcpStream,
    request: &HttpRequest,
    file_path: Arc<PathBuf>,
) -> io::Result<()> {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/stream/current") | ("HEAD", "/stream/current") => respond_with_file(
            writer,
            file_path.as_path(),
            request.method == "HEAD",
            request.range_header.clone(),
        ),
        ("GET", "/health") | ("HEAD", "/health") => respond_text(
            writer,
            "200 OK",
            "text/plain; charset=utf-8",
            b"ok",
            request.method == "HEAD",
        ),
        _ => respond_not_found(writer, request.method == "HEAD"),
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

fn read_http_request(reader: &mut BufReader<TcpStream>) -> io::Result<Option<HttpRequest>> {
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(None);
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query) = split_target_and_query(&target);

    let mut range_header = None;
    let mut content_type = None;
    let mut content_length = 0_usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("Range") {
                range_header = Some(value.trim().to_string());
            } else if name.eq_ignore_ascii_case("Content-Type") {
                content_type = Some(value.trim().to_string());
            } else if name.eq_ignore_ascii_case("Content-Length") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }

    let mut body = vec![0_u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    let form = parse_request_form(content_type.as_deref(), &body);

    Ok(Some(HttpRequest {
        method,
        target,
        path,
        query,
        form,
        range_header,
        content_type,
        body,
    }))
}

fn respond_with_file(
    writer: &mut TcpStream,
    file_path: &Path,
    head_only: bool,
    range_header: Option<String>,
) -> io::Result<()> {
    let mut file = match File::open(file_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return respond_not_found(writer, head_only);
        }
        Err(error) => return Err(error),
    };

    let total_len = file.metadata()?.len();
    let mime_type = infer_mime_type(file_path);
    let response_range = range_header
        .as_deref()
        .and_then(|value| parse_range_header(value, total_len));

    match response_range {
        Some((start, end)) => {
            let content_len = end - start + 1;
            let content_length_text = content_len.to_string();
            let content_range_text = format!("bytes {start}-{end}/{total_len}");

            write_response_owned(
                writer,
                "206 Partial Content",
                &[
                    ("Content-Type".to_string(), mime_type.to_string()),
                    ("Accept-Ranges".to_string(), "bytes".to_string()),
                    ("Content-Length".to_string(), content_length_text),
                    ("Content-Range".to_string(), content_range_text),
                ],
                None,
            )?;

            if !head_only {
                file.seek(SeekFrom::Start(start))?;
                copy_exact_bytes(&mut file, writer, content_len)?;
            }

            Ok(())
        }
        None => {
            let content_length_text = total_len.to_string();
            write_response_owned(
                writer,
                "200 OK",
                &[
                    ("Content-Type".to_string(), mime_type.to_string()),
                    ("Accept-Ranges".to_string(), "bytes".to_string()),
                    ("Content-Length".to_string(), content_length_text),
                ],
                None,
            )?;

            if !head_only {
                io::copy(&mut file, writer)?;
            }

            Ok(())
        }
    }
}

fn copy_exact_bytes(
    reader: &mut File,
    writer: &mut TcpStream,
    mut bytes_left: u64,
) -> io::Result<()> {
    let mut buffer = [0_u8; 16 * 1024];
    while bytes_left > 0 {
        let to_read = usize::try_from(bytes_left.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let read = reader.read(&mut buffer[..to_read])?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        bytes_left -= read as u64;
    }
    Ok(())
}

fn respond_text(
    writer: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> io::Result<()> {
    write_response_owned(
        writer,
        status,
        &[
            ("Content-Type".to_string(), content_type.to_string()),
            ("Content-Length".to_string(), body.len().to_string()),
        ],
        if head_only { None } else { Some(body) },
    )
}

fn respond_json(writer: &mut TcpStream, status: &str, body: &str) -> io::Result<()> {
    respond_text(
        writer,
        status,
        "application/json; charset=utf-8",
        body.as_bytes(),
        false,
    )
}

fn api_error(writer: &mut TcpStream, status: &str, error: &str) -> io::Result<()> {
    respond_json(
        writer,
        status,
        &format!(r#"{{"ok":false,"error":"{}"}}"#, json_escape(error)),
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

fn is_expected_client_disconnect(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::UnexpectedEof
    )
}

fn write_sse_event(writer: &mut TcpStream, event: &str, data: &str) -> io::Result<()> {
    write!(writer, "event: {event}\r\n")?;
    for line in data.lines() {
        write!(writer, "data: {line}\r\n")?;
    }
    write!(writer, "\r\n")?;
    writer.flush()
}

fn write_sse_comment(writer: &mut TcpStream, comment: &str) -> io::Result<()> {
    write!(writer, ": {comment}\r\n\r\n")?;
    writer.flush()
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

fn respond_not_found(writer: &mut TcpStream, head_only: bool) -> io::Result<()> {
    respond_text(
        writer,
        "404 Not Found",
        "text/plain; charset=utf-8",
        b"not found",
        head_only,
    )
}

fn respond_method_not_allowed(writer: &mut TcpStream) -> io::Result<()> {
    respond_text(
        writer,
        "405 Method Not Allowed",
        "text/plain; charset=utf-8",
        b"method not allowed",
        false,
    )
}

fn redirect_home(
    writer: &mut TcpStream,
    renderer_location: Option<&str>,
    message: Option<&str>,
    error: Option<&str>,
) -> io::Result<()> {
    redirect_to_path(writer, "/", renderer_location, message, error)
}

fn redirect_to_path(
    writer: &mut TcpStream,
    path: &str,
    renderer_location: Option<&str>,
    message: Option<&str>,
    error: Option<&str>,
) -> io::Result<()> {
    let mut params = Vec::new();
    if let Some(renderer_location) = renderer_location {
        if !renderer_location.is_empty() {
            params.push(format!(
                "renderer_location={}",
                url_encode(renderer_location)
            ));
        }
    }
    if let Some(message) = message {
        params.push(format!("message={}", url_encode(message)));
    }
    if let Some(error) = error {
        params.push(format!("error={}", url_encode(error)));
    }

    let location = if params.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", params.join("&"))
    };

    write_response_owned(
        writer,
        "303 See Other",
        &[("Location".to_string(), location)],
        None,
    )
}

fn redirect_album(
    writer: &mut TcpStream,
    album_id: &str,
    renderer_location: Option<&str>,
    message: Option<&str>,
    error: Option<&str>,
) -> io::Result<()> {
    let mut params = Vec::new();
    if let Some(renderer_location) = renderer_location {
        if !renderer_location.is_empty() {
            params.push(format!(
                "renderer_location={}",
                url_encode(renderer_location)
            ));
        }
    }
    if let Some(message) = message {
        params.push(format!("message={}", url_encode(message)));
    }
    if let Some(error) = error {
        params.push(format!("error={}", url_encode(error)));
    }

    let location = if params.is_empty() {
        format!("/album/{}", url_encode(album_id))
    } else {
        format!("/album/{}?{}", url_encode(album_id), params.join("&"))
    };

    write_response_owned(
        writer,
        "303 See Other",
        &[("Location".to_string(), location)],
        None,
    )
}

fn write_response_owned(
    writer: &mut TcpStream,
    status: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> io::Result<()> {
    let status_code = status
        .split_whitespace()
        .next()
        .and_then(|n| n.parse::<u16>().ok())
        .unwrap_or(0);
    metrics::set_response_status(status_code);
    write!(writer, "HTTP/1.1 {status}\r\nConnection: close\r\n")?;
    for (name, value) in headers {
        write!(writer, "{name}: {value}\r\n")?;
    }
    write!(writer, "\r\n")?;
    if let Some(body) = body {
        writer.write_all(body)?;
    }
    writer.flush()
}

fn split_target_and_query(target: &str) -> (String, HashMap<String, String>) {
    match target.split_once('?') {
        Some((path, query)) => (path.to_string(), parse_query_string(query)),
        None => (target.to_string(), HashMap::new()),
    }
}

fn parse_query_string(query: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };
        values.insert(percent_decode(key), percent_decode(value));
    }
    values
}

fn parse_request_form(content_type: Option<&str>, body: &[u8]) -> HashMap<String, String> {
    if body.is_empty() {
        return HashMap::new();
    }

    let is_form = content_type
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/x-www-form-urlencoded")
        })
        .unwrap_or(false);
    if !is_form {
        return HashMap::new();
    }

    let decoded = String::from_utf8_lossy(body);
    parse_query_string(&decoded)
}

fn request_value<'a>(request: &'a HttpRequest, key: &str) -> Option<&'a str> {
    request
        .form
        .get(key)
        .or_else(|| request.query.get(key))
        .map(String::as_str)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &value[index + 1..index + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    output.push(byte);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

fn url_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn render_home_page(state: &ServiceState, request: &HttpRequest) -> String {
    let tracks = state.tracks_snapshot();
    let albums = state.albums_snapshot();
    let library_path = state.config.library_path.display().to_string();
    let renderer_location = state
        .preferred_renderer_location(request.query.get("renderer_location").map(String::as_str));
    let queue_html = render_queue_panel(state, &renderer_location, &tracks);
    let known_renderers = state.renderer_snapshot();
    let message = request.query.get("message").cloned().unwrap_or_default();
    let error = request.query.get("error").cloned().unwrap_or_default();

    let mut album_rows = String::new();
    for album in &albums {
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
    for track in &tracks {
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
    tracks: &[LibraryTrack],
) -> String {
    if renderer_location.trim().is_empty() {
        return "<section class=\"table-wrap\"><p class=\"empty\">Enter a renderer LOCATION URL to inspect or build a queue.</p></section>".to_string();
    }

    let queue = state.queue_snapshot(renderer_location);
    let session = state.playback_session(renderer_location);
    let current_track = queue.as_ref().and_then(|queue| {
        queue.current_entry_id.and_then(|current_entry_id| {
            queue
                .entries
                .iter()
                .find(|entry| entry.id == current_entry_id)
                .and_then(|entry| tracks.iter().find(|track| track.id == entry.track_id))
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
            let track = tracks.iter().find(|track| track.id == entry.track_id);
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
    let tracks = state.tracks_snapshot();
    render_queue_panel(state, &renderer_location, &tracks)
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
        .into_iter()
        .filter_map(|album| album.artwork_url.map(|artwork_url| (album.id, artwork_url)))
        .collect::<HashMap<_, _>>();
    let entries = tracks
        .into_iter()
        .map(|track| {
            let fallback_artwork_url =
                album_artwork_by_id.get(&track.album_id).map(String::as_str);
            let summary_json = track_summary_json(&track, fallback_artwork_url);
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
        .into_iter()
        .map(|album| album_summary_json(&album))
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

fn renderer_kind_name(kind: RendererKind) -> &'static str {
    match kind {
        RendererKind::Upnp => "upnp",
        RendererKind::Sonos => "sonos",
        RendererKind::AndroidLocal => "android_local",
    }
}

impl Database {
    fn open(config_path: &Path) -> io::Result<Self> {
        fs::create_dir_all(config_path)?;
        let path = config_path.join("musicd.db");
        let manager = SqliteConnectionManager::file(&path).with_init(|connection| {
            connection.pragma_update(None, "journal_mode", "WAL")?;
            connection.pragma_update(None, "synchronous", "NORMAL")?;
            connection.pragma_update(None, "foreign_keys", true)?;
            connection.busy_timeout(Duration::from_secs(2))?;
            Ok(())
        });
        let pool = Pool::builder()
            .max_size(8)
            .build(manager)
            .map_err(|error| io::Error::other(format!("failed to build sqlite pool: {error}")))?;
        let database = Self { pool };
        database.initialize()?;
        Ok(database)
    }

    fn initialize(&self) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS tracks (
                    id TEXT PRIMARY KEY,
                    album_id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    artist TEXT NOT NULL,
                    album TEXT NOT NULL,
                    disc_number INTEGER,
                    track_number INTEGER,
                    duration_seconds INTEGER,
                    relative_path TEXT NOT NULL,
                    path TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    file_size INTEGER NOT NULL,
                    artwork_cache_key TEXT,
                    artwork_source TEXT,
                    artwork_mime_type TEXT
                );

                CREATE TABLE IF NOT EXISTS albums (
                    id TEXT PRIMARY KEY,
                    artist_id TEXT NOT NULL,
                    title TEXT NOT NULL,
                    artist_name TEXT NOT NULL,
                    track_count INTEGER NOT NULL,
                    artwork_track_id TEXT,
                    artwork_cache_key TEXT,
                    artwork_source TEXT,
                    artwork_mime_type TEXT,
                    first_track_id TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS artists (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    album_count INTEGER NOT NULL,
                    track_count INTEGER NOT NULL,
                    artwork_track_id TEXT,
                    first_album_id TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS renderers (
                    location TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    manufacturer TEXT,
                    model_name TEXT,
                    av_transport_control_url TEXT,
                    av_transport_actions_json TEXT,
                    has_playlist_extension_service INTEGER,
                    last_checked_unix INTEGER NOT NULL DEFAULT 0,
                    last_reachable_unix INTEGER,
                    last_error TEXT,
                    last_seen_unix INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS app_state (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS playback_queues (
                    renderer_location TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    current_entry_id INTEGER,
                    status TEXT NOT NULL,
                    version INTEGER NOT NULL DEFAULT 1,
                    updated_unix INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS queue_entries (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    renderer_location TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    track_id TEXT NOT NULL,
                    album_id TEXT,
                    source_kind TEXT NOT NULL,
                    source_ref TEXT,
                    entry_status TEXT NOT NULL,
                    started_unix INTEGER,
                    completed_unix INTEGER
                );

                CREATE TABLE IF NOT EXISTS playback_sessions (
                    renderer_location TEXT PRIMARY KEY,
                    queue_entry_id INTEGER,
                    next_queue_entry_id INTEGER,
                    transport_state TEXT NOT NULL,
                    current_track_uri TEXT,
                    position_seconds INTEGER,
                    duration_seconds INTEGER,
                    last_observed_unix INTEGER NOT NULL DEFAULT 0,
                    last_error TEXT
                );

                CREATE TABLE IF NOT EXISTS track_play_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    track_id TEXT NOT NULL,
                    renderer_location TEXT NOT NULL,
                    queue_entry_id INTEGER,
                    played_unix INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS album_artwork_overrides (
                    album_id TEXT PRIMARY KEY,
                    cache_key TEXT NOT NULL,
                    source TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    musicbrainz_release_id TEXT,
                    applied_unix INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_track_play_history_track_id
                ON track_play_history(track_id, played_unix DESC);

                CREATE INDEX IF NOT EXISTS idx_track_play_history_renderer
                ON track_play_history(renderer_location, played_unix DESC);
                "#,
            )
            .map_err(db_error)?;
        ensure_column(&connection, "tracks", "album_id", "TEXT")?;
        ensure_column(&connection, "tracks", "disc_number", "INTEGER")?;
        ensure_column(&connection, "tracks", "track_number", "INTEGER")?;
        ensure_column(&connection, "tracks", "duration_seconds", "INTEGER")?;
        ensure_column(&connection, "tracks", "artwork_cache_key", "TEXT")?;
        ensure_column(&connection, "tracks", "artwork_source", "TEXT")?;
        ensure_column(&connection, "tracks", "artwork_mime_type", "TEXT")?;
        ensure_column(&connection, "albums", "artist_id", "TEXT")?;
        ensure_column(&connection, "albums", "title", "TEXT")?;
        ensure_column(&connection, "albums", "artist_name", "TEXT")?;
        ensure_column(&connection, "albums", "track_count", "INTEGER")?;
        ensure_column(&connection, "albums", "artwork_track_id", "TEXT")?;
        ensure_column(&connection, "albums", "artwork_cache_key", "TEXT")?;
        ensure_column(&connection, "albums", "artwork_source", "TEXT")?;
        ensure_column(&connection, "albums", "artwork_mime_type", "TEXT")?;
        ensure_column(&connection, "albums", "first_track_id", "TEXT")?;
        ensure_column(&connection, "artists", "album_count", "INTEGER")?;
        ensure_column(&connection, "artists", "track_count", "INTEGER")?;
        ensure_column(&connection, "artists", "artwork_track_id", "TEXT")?;
        ensure_column(&connection, "artists", "first_album_id", "TEXT")?;
        ensure_column(
            &connection,
            "renderers",
            "av_transport_actions_json",
            "TEXT",
        )?;
        ensure_column(
            &connection,
            "renderers",
            "has_playlist_extension_service",
            "INTEGER",
        )?;
        ensure_column(
            &connection,
            "renderers",
            "last_checked_unix",
            "INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(&connection, "renderers", "last_reachable_unix", "INTEGER")?;
        ensure_column(&connection, "renderers", "last_error", "TEXT")?;
        connection
            .execute(
                "UPDATE renderers
                 SET last_reachable_unix = last_seen_unix
                 WHERE last_reachable_unix IS NULL AND last_seen_unix > 0",
                [],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "UPDATE renderers
                 SET last_checked_unix = last_seen_unix
                 WHERE last_checked_unix = 0 AND last_seen_unix > 0",
                [],
            )
            .map_err(db_error)?;
        ensure_column(
            &connection,
            "playback_sessions",
            "next_queue_entry_id",
            "INTEGER",
        )?;
        ensure_column(
            &connection,
            "album_artwork_overrides",
            "musicbrainz_release_id",
            "TEXT",
        )?;
        if table_is_empty(&connection, "albums")? && !table_is_empty(&connection, "tracks")? {
            rebuild_normalized_library_tables(&connection)?;
        }
        Ok(())
    }

    fn connection(&self) -> io::Result<SqliteConn> {
        self.pool
            .get()
            .map_err(|error| io::Error::other(format!("failed to acquire sqlite connection: {error}")))
    }

    fn load_library(&self, scan_root: PathBuf) -> io::Result<Library> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, album_id, title, artist, album, disc_number, track_number,
                        duration_seconds, relative_path, path, mime_type, file_size,
                        artwork_cache_key, artwork_source, artwork_mime_type
                 FROM tracks
                 ORDER BY artist, album, COALESCE(disc_number, 0), COALESCE(track_number, 0), title, relative_path",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                let artist: String = row.get(3)?;
                let album: String = row.get(4)?;
                Ok(LibraryTrack {
                    id: row.get(0)?,
                    album_id: row
                        .get::<_, Option<String>>(1)?
                        .unwrap_or_else(|| stable_album_id(&artist, &album)),
                    title: row.get(2)?,
                    artist,
                    album,
                    disc_number: row.get(5)?,
                    track_number: row.get(6)?,
                    duration_seconds: row.get(7)?,
                    relative_path: row.get(8)?,
                    path: PathBuf::from(row.get::<_, String>(9)?),
                    mime_type: row.get(10)?,
                    file_size: row.get(11)?,
                    artwork: match (
                        row.get::<_, Option<String>>(12)?,
                        row.get::<_, Option<String>>(13)?,
                        row.get::<_, Option<String>>(14)?,
                    ) {
                        (Some(cache_key), Some(source), Some(mime_type)) => Some(TrackArtwork {
                            cache_key,
                            source,
                            mime_type,
                        }),
                        _ => None,
                    },
                })
            })
            .map_err(db_error)?;

        let mut tracks = Vec::new();
        for row in rows {
            tracks.push(row.map_err(db_error)?);
        }

        Ok(Library { scan_root, tracks })
    }

    fn save_library(&self, library: &Library) -> io::Result<()> {
        let albums = build_album_summaries(&library.tracks);
        let artists = build_artist_summaries_from_albums(&library.tracks, &albums);
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        transaction
            .execute("DELETE FROM tracks", [])
            .map_err(db_error)?;
        transaction
            .execute("DELETE FROM albums", [])
            .map_err(db_error)?;
        transaction
            .execute("DELETE FROM artists", [])
            .map_err(db_error)?;
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO tracks
                     (id, album_id, title, artist, album, disc_number, track_number,
                      duration_seconds, relative_path, path, mime_type, file_size,
                      artwork_cache_key, artwork_source, artwork_mime_type)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .map_err(db_error)?;

            for track in &library.tracks {
                let artwork_cache_key = track
                    .artwork
                    .as_ref()
                    .map(|artwork| artwork.cache_key.clone());
                let artwork_source = track.artwork.as_ref().map(|artwork| artwork.source.clone());
                let artwork_mime_type = track
                    .artwork
                    .as_ref()
                    .map(|artwork| artwork.mime_type.clone());
                statement
                    .execute(params![
                        track.id,
                        track.album_id,
                        track.title,
                        track.artist,
                        track.album,
                        track.disc_number,
                        track.track_number,
                        track.duration_seconds,
                        track.relative_path,
                        track.path.display().to_string(),
                        track.mime_type,
                        track.file_size,
                        artwork_cache_key,
                        artwork_source,
                        artwork_mime_type
                    ])
                    .map_err(db_error)?;
            }
        }
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO albums
                     (id, artist_id, title, artist_name, track_count, artwork_track_id,
                      artwork_cache_key, artwork_source, artwork_mime_type, first_track_id)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .map_err(db_error)?;

            for album in &albums {
                let artwork_cache_key = album
                    .artwork
                    .as_ref()
                    .map(|artwork| artwork.cache_key.clone());
                let artwork_source = album.artwork.as_ref().map(|artwork| artwork.source.clone());
                let artwork_mime_type = album
                    .artwork
                    .as_ref()
                    .map(|artwork| artwork.mime_type.clone());
                statement
                    .execute(params![
                        album.id,
                        album.artist_id,
                        album.title,
                        album.artist,
                        album.track_count,
                        album.artwork_track_id,
                        artwork_cache_key,
                        artwork_source,
                        artwork_mime_type,
                        album.first_track_id,
                    ])
                    .map_err(db_error)?;
            }
        }
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO artists
                     (id, name, album_count, track_count, artwork_track_id, first_album_id)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .map_err(db_error)?;

            for artist in &artists {
                statement
                    .execute(params![
                        artist.id,
                        artist.name,
                        artist.album_count,
                        artist.track_count,
                        artist.artwork_track_id,
                        artist.first_album_id,
                    ])
                    .map_err(db_error)?;
            }
        }
        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    fn load_albums(&self) -> io::Result<Vec<AlbumSummary>> {
        let connection = self.connection()?;
        load_albums_from_connection(&connection)
    }

    fn load_artists(&self) -> io::Result<Vec<ArtistSummary>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, album_count, track_count, artwork_track_id, first_album_id
                 FROM artists
                 ORDER BY name ASC, id ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                let artwork_track_id = row.get::<_, Option<String>>(4)?;
                Ok(ArtistSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    album_count: row.get(2)?,
                    track_count: row.get(3)?,
                    artwork_track_id: artwork_track_id.clone(),
                    artwork_url: artwork_track_id
                        .map(|track_id| format!("/artwork/track/{track_id}")),
                    first_album_id: row.get(5)?,
                })
            })
            .map_err(db_error)?;

        let mut artists = Vec::new();
        for row in rows {
            artists.push(row.map_err(db_error)?);
        }
        Ok(artists)
    }

    fn list_renderers(&self) -> io::Result<Vec<RendererRecord>> {
        let connection = self.connection()?;
        let selected = Self::last_selected_renderer_location_with(&connection)?;
        let mut statement = connection
            .prepare(
                "SELECT location, name, manufacturer, model_name, av_transport_control_url,
                        av_transport_actions_json, has_playlist_extension_service,
                        last_checked_unix, last_reachable_unix, last_error, last_seen_unix
                 FROM renderers
                 ORDER BY last_seen_unix DESC, name ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(RendererRecord {
                    location: row.get(0)?,
                    name: row.get(1)?,
                    manufacturer: row.get(2)?,
                    model_name: row.get(3)?,
                    av_transport_control_url: row.get(4)?,
                    capabilities: RendererCapabilities {
                        av_transport_actions: parse_renderer_actions_json(row.get(5)?),
                        has_playlist_extension_service: row.get(6)?,
                    },
                    last_checked_unix: row.get(7)?,
                    last_reachable_unix: row.get(8)?,
                    last_error: row.get(9)?,
                    last_seen_unix: row.get(10)?,
                })
            })
            .map_err(db_error)?;

        let mut renderers = Vec::new();
        for row in rows {
            renderers.push(row.map_err(db_error)?);
        }
        if let Some(selected) = selected {
            renderers.sort_by_key(|renderer| {
                (
                    renderer.location != selected,
                    -renderer.last_seen_unix,
                    renderer.name.clone(),
                )
            });
        }
        Ok(renderers)
    }

    fn list_album_artwork_overrides(&self) -> io::Result<Vec<AlbumArtworkOverride>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT album_id, cache_key, source, mime_type, musicbrainz_release_id, applied_unix
                 FROM album_artwork_overrides",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(AlbumArtworkOverride {
                    album_id: row.get(0)?,
                    cache_key: row.get(1)?,
                    source: row.get(2)?,
                    mime_type: row.get(3)?,
                    musicbrainz_release_id: row.get(4)?,
                    applied_unix: row.get(5)?,
                })
            })
            .map_err(db_error)?;

        let mut overrides = Vec::new();
        for row in rows {
            overrides.push(row.map_err(db_error)?);
        }
        Ok(overrides)
    }

    fn upsert_album_artwork_override(
        &self,
        override_record: &AlbumArtworkOverride,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO album_artwork_overrides
                 (album_id, cache_key, source, mime_type, musicbrainz_release_id, applied_unix)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(album_id) DO UPDATE SET
                   cache_key = excluded.cache_key,
                   source = excluded.source,
                   mime_type = excluded.mime_type,
                   musicbrainz_release_id = excluded.musicbrainz_release_id,
                   applied_unix = excluded.applied_unix",
                params![
                    override_record.album_id,
                    override_record.cache_key,
                    override_record.source,
                    override_record.mime_type,
                    override_record.musicbrainz_release_id,
                    override_record.applied_unix,
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn load_renderer(&self, location: &str) -> io::Result<Option<RendererRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT location, name, manufacturer, model_name, av_transport_control_url,
                        av_transport_actions_json, has_playlist_extension_service,
                        last_checked_unix, last_reachable_unix, last_error, last_seen_unix
                 FROM renderers
                 WHERE location = ?",
                [location],
                |row| {
                    Ok(RendererRecord {
                        location: row.get(0)?,
                        name: row.get(1)?,
                        manufacturer: row.get(2)?,
                        model_name: row.get(3)?,
                        av_transport_control_url: row.get(4)?,
                        capabilities: RendererCapabilities {
                            av_transport_actions: parse_renderer_actions_json(row.get(5)?),
                            has_playlist_extension_service: row.get(6)?,
                        },
                        last_checked_unix: row.get(7)?,
                        last_reachable_unix: row.get(8)?,
                        last_error: row.get(9)?,
                        last_seen_unix: row.get(10)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    fn upsert_renderer(&self, renderer: &RendererRecord) -> io::Result<()> {
        let connection = self.connection()?;
        let av_transport_actions_json =
            renderer_actions_json(&renderer.capabilities.av_transport_actions);
        connection
            .execute(
                "INSERT INTO renderers
                 (location, name, manufacturer, model_name, av_transport_control_url,
                  av_transport_actions_json, has_playlist_extension_service, last_checked_unix,
                  last_reachable_unix, last_error, last_seen_unix)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(location) DO UPDATE SET
                    name = excluded.name,
                    manufacturer = COALESCE(excluded.manufacturer, renderers.manufacturer),
                    model_name = COALESCE(excluded.model_name, renderers.model_name),
                    av_transport_control_url = COALESCE(excluded.av_transport_control_url, renderers.av_transport_control_url),
                    av_transport_actions_json = COALESCE(excluded.av_transport_actions_json, renderers.av_transport_actions_json),
                    has_playlist_extension_service = COALESCE(excluded.has_playlist_extension_service, renderers.has_playlist_extension_service),
                    last_checked_unix = excluded.last_checked_unix,
                    last_reachable_unix = COALESCE(excluded.last_reachable_unix, renderers.last_reachable_unix),
                    last_error = excluded.last_error,
                    last_seen_unix = excluded.last_seen_unix",
                params![
                    renderer.location,
                    renderer.name,
                    renderer.manufacturer,
                    renderer.model_name,
                    renderer.av_transport_control_url,
                    av_transport_actions_json,
                    renderer.capabilities.has_playlist_extension_service,
                    renderer.last_checked_unix,
                    renderer.last_reachable_unix,
                    renderer.last_error,
                    renderer.last_seen_unix
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn set_last_selected_renderer_location(&self, location: &str) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO app_state (key, value) VALUES ('last_renderer_location', ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [location],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn last_selected_renderer_location(&self) -> io::Result<Option<String>> {
        let connection = self.connection()?;
        Self::last_selected_renderer_location_with(&connection)
    }

    fn last_selected_renderer_location_with(
        connection: &Connection,
    ) -> io::Result<Option<String>> {
        connection
            .query_row(
                "SELECT value FROM app_state WHERE key = 'last_renderer_location'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)
    }

    fn load_queue(&self, renderer_location: &str) -> io::Result<Option<PlaybackQueue>> {
        let connection = self.connection()?;
        Self::load_queue_with(&connection, renderer_location)
    }

    fn load_queue_with(
        connection: &Connection,
        renderer_location: &str,
    ) -> io::Result<Option<PlaybackQueue>> {
        let queue_row = connection
            .query_row(
                "SELECT renderer_location, name, current_entry_id, status, version, updated_unix
                 FROM playback_queues
                 WHERE renderer_location = ?",
                [renderer_location],
                |row| {
                    Ok(PlaybackQueue {
                        renderer_location: row.get(0)?,
                        name: row.get(1)?,
                        current_entry_id: row.get(2)?,
                        status: row.get(3)?,
                        version: row.get(4)?,
                        updated_unix: row.get(5)?,
                        entries: Vec::new(),
                    })
                },
            )
            .optional()
            .map_err(db_error)?;

        let Some(mut queue) = queue_row else {
            return Ok(None);
        };

        let mut statement = connection
            .prepare(
                "SELECT id, position, track_id, album_id, source_kind, source_ref,
                        entry_status, started_unix, completed_unix
                 FROM queue_entries
                 WHERE renderer_location = ?
                 ORDER BY position ASC, id ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([renderer_location], |row| {
                Ok(QueueEntry {
                    id: row.get(0)?,
                    position: row.get(1)?,
                    track_id: row.get(2)?,
                    album_id: row.get(3)?,
                    source_kind: row.get(4)?,
                    source_ref: row.get(5)?,
                    entry_status: row.get(6)?,
                    started_unix: row.get(7)?,
                    completed_unix: row.get(8)?,
                })
            })
            .map_err(db_error)?;

        for row in rows {
            queue.entries.push(row.map_err(db_error)?);
        }

        Ok(Some(queue))
    }

    fn load_playback_session(
        &self,
        renderer_location: &str,
    ) -> io::Result<Option<PlaybackSession>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                        position_seconds, duration_seconds, last_observed_unix, last_error
                 FROM playback_sessions
                 WHERE renderer_location = ?",
                [renderer_location],
                |row| {
                    Ok(PlaybackSession {
                        renderer_location: row.get(0)?,
                        queue_entry_id: row.get(1)?,
                        next_queue_entry_id: row.get(2)?,
                        transport_state: row.get(3)?,
                        current_track_uri: row.get(4)?,
                        position_seconds: row.get::<_, Option<u64>>(5)?,
                        duration_seconds: row.get::<_, Option<u64>>(6)?,
                        last_observed_unix: row.get(7)?,
                        last_error: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    #[allow(dead_code)]
    fn count_track_plays(&self, track_id: &str) -> io::Result<u64> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT COUNT(*) FROM track_play_history WHERE track_id = ?",
                [track_id],
                |row| row.get::<_, u64>(0),
            )
            .map_err(db_error)
    }

    #[allow(dead_code)]
    fn load_track_play_history(&self, track_id: &str) -> io::Result<Vec<TrackPlayRecord>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, track_id, renderer_location, queue_entry_id, played_unix
                 FROM track_play_history
                 WHERE track_id = ?
                 ORDER BY played_unix DESC, id DESC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([track_id], |row| {
                Ok(TrackPlayRecord {
                    id: row.get(0)?,
                    track_id: row.get(1)?,
                    renderer_location: row.get(2)?,
                    queue_entry_id: row.get(3)?,
                    played_unix: row.get(4)?,
                })
            })
            .map_err(db_error)?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(db_error)?);
        }
        Ok(records)
    }

    fn replace_queue(
        &self,
        renderer_location: &str,
        name: &str,
        entries: &[QueueMutationEntry],
    ) -> io::Result<PlaybackQueue> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_version = transaction
            .query_row(
                "SELECT version FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?
            .unwrap_or(0);
        transaction
            .execute(
                "DELETE FROM queue_entries WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;

        let mut current_entry_id = None;
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO queue_entries
                     (renderer_location, position, track_id, album_id, source_kind, source_ref,
                      entry_status, started_unix, completed_unix)
                     VALUES (?, ?, ?, ?, ?, ?, 'pending', NULL, NULL)",
                )
                .map_err(db_error)?;
            for (index, entry) in entries.iter().enumerate() {
                statement
                    .execute(params![
                        renderer_location,
                        i64::try_from(index + 1).unwrap_or(i64::MAX),
                        entry.track_id,
                        entry.album_id,
                        entry.source_kind,
                        entry.source_ref,
                    ])
                    .map_err(db_error)?;
                if index == 0 {
                    current_entry_id = Some(transaction.last_insert_rowid());
                }
            }
        }

        transaction
            .execute(
                "INSERT INTO playback_queues
                 (renderer_location, name, current_entry_id, status, version, updated_unix)
                 VALUES (?, ?, ?, ?, ?, ?)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    name = excluded.name,
                    current_entry_id = excluded.current_entry_id,
                    status = excluded.status,
                    version = excluded.version,
                    updated_unix = excluded.updated_unix",
                params![
                    renderer_location,
                    name,
                    current_entry_id,
                    if entries.is_empty() { "empty" } else { "ready" },
                    current_version + 1,
                    now_unix_timestamp(),
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM playback_sessions WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;

        Self::load_queue_with(&connection, renderer_location)?
            .ok_or_else(|| io::Error::other("queue disappeared after replace"))
    }

    fn append_queue_entries(
        &self,
        renderer_location: &str,
        name: &str,
        entries: &[QueueMutationEntry],
    ) -> io::Result<PlaybackQueue> {
        if entries.is_empty() {
            return self
                .load_queue(renderer_location)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue does not exist"));
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_version = transaction
            .query_row(
                "SELECT version FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?
            .unwrap_or(0);
        let max_position = transaction
            .query_row(
                "SELECT MAX(position) FROM queue_entries WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(db_error)?
            .unwrap_or(0);
        let mut first_inserted_id = None;
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO queue_entries
                     (renderer_location, position, track_id, album_id, source_kind, source_ref,
                      entry_status, started_unix, completed_unix)
                     VALUES (?, ?, ?, ?, ?, ?, 'pending', NULL, NULL)",
                )
                .map_err(db_error)?;
            for (index, entry) in entries.iter().enumerate() {
                statement
                    .execute(params![
                        renderer_location,
                        max_position + i64::try_from(index + 1).unwrap_or(i64::MAX),
                        entry.track_id,
                        entry.album_id,
                        entry.source_kind,
                        entry.source_ref,
                    ])
                    .map_err(db_error)?;
                if index == 0 {
                    first_inserted_id = Some(transaction.last_insert_rowid());
                }
            }
        }

        let existing_current_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();
        transaction
            .execute(
                "INSERT INTO playback_queues
                 (renderer_location, name, current_entry_id, status, version, updated_unix)
                 VALUES (?, ?, ?, 'ready', ?, ?)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    name = excluded.name,
                    current_entry_id = COALESCE(playback_queues.current_entry_id, excluded.current_entry_id),
                    status = CASE
                        WHEN playback_queues.status = 'playing' THEN playback_queues.status
                        ELSE excluded.status
                    END,
                    version = excluded.version,
                    updated_unix = excluded.updated_unix",
                params![
                    renderer_location,
                    name,
                    existing_current_entry_id.or(first_inserted_id),
                    current_version + 1,
                    now_unix_timestamp(),
                ],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;

        Self::load_queue_with(&connection, renderer_location)?
            .ok_or_else(|| io::Error::other("queue disappeared after append"))
    }

    fn insert_queue_entries_after_current(
        &self,
        renderer_location: &str,
        name: &str,
        entries: &[QueueMutationEntry],
    ) -> io::Result<PlaybackQueue> {
        if entries.is_empty() {
            return self
                .load_queue(renderer_location)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue does not exist"));
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_version = transaction
            .query_row(
                "SELECT version FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?
            .unwrap_or(0);
        let existing_current_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();
        let insert_after_position = if let Some(current_entry_id) = existing_current_entry_id {
            transaction
                .query_row(
                    "SELECT position FROM queue_entries WHERE id = ?",
                    [current_entry_id],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(db_error)?
                .unwrap_or_else(|| {
                    transaction
                        .query_row(
                            "SELECT MAX(position) FROM queue_entries WHERE renderer_location = ?",
                            [renderer_location],
                            |row| row.get::<_, Option<i64>>(0),
                        )
                        .map_err(db_error)
                        .ok()
                        .flatten()
                        .unwrap_or(0)
                })
        } else {
            transaction
                .query_row(
                    "SELECT MAX(position) FROM queue_entries WHERE renderer_location = ?",
                    [renderer_location],
                    |row| row.get::<_, Option<i64>>(0),
                )
                .map_err(db_error)?
                .unwrap_or(0)
        };

        transaction
            .execute(
                "UPDATE queue_entries
                 SET position = position + ?
                 WHERE renderer_location = ?
                   AND position > ?",
                params![
                    i64::try_from(entries.len()).unwrap_or(i64::MAX),
                    renderer_location,
                    insert_after_position
                ],
            )
            .map_err(db_error)?;

        let mut first_inserted_id = None;
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO queue_entries
                     (renderer_location, position, track_id, album_id, source_kind, source_ref,
                      entry_status, started_unix, completed_unix)
                     VALUES (?, ?, ?, ?, ?, ?, 'pending', NULL, NULL)",
                )
                .map_err(db_error)?;
            for (index, entry) in entries.iter().enumerate() {
                statement
                    .execute(params![
                        renderer_location,
                        insert_after_position + i64::try_from(index + 1).unwrap_or(i64::MAX),
                        entry.track_id,
                        entry.album_id,
                        entry.source_kind,
                        entry.source_ref,
                    ])
                    .map_err(db_error)?;
                if index == 0 {
                    first_inserted_id = Some(transaction.last_insert_rowid());
                }
            }
        }

        transaction
            .execute(
                "INSERT INTO playback_queues
                 (renderer_location, name, current_entry_id, status, version, updated_unix)
                 VALUES (?, ?, ?, 'ready', ?, ?)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    name = excluded.name,
                    current_entry_id = COALESCE(playback_queues.current_entry_id, excluded.current_entry_id),
                    status = CASE
                        WHEN playback_queues.status = 'playing' THEN playback_queues.status
                        WHEN playback_queues.status = 'paused' THEN playback_queues.status
                        ELSE excluded.status
                    END,
                    version = excluded.version,
                    updated_unix = excluded.updated_unix",
                params![
                    renderer_location,
                    name,
                    existing_current_entry_id.or(first_inserted_id),
                    current_version + 1,
                    now_unix_timestamp(),
                ],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;

        Self::load_queue_with(&connection, renderer_location)?
            .ok_or_else(|| io::Error::other("queue disappeared after insert"))
    }

    fn move_queue_entry(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
        direction: i64,
    ) -> io::Result<PlaybackQueue> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();
        if current_entry_id == Some(queue_entry_id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot move the currently playing queue entry",
            ));
        }

        let current_position = transaction
            .query_row(
                "SELECT position FROM queue_entries
                 WHERE renderer_location = ? AND id = ?",
                params![renderer_location, queue_entry_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue entry not found"))?;

        let neighbor = if direction < 0 {
            transaction
                .query_row(
                    "SELECT id, position
                     FROM queue_entries
                     WHERE renderer_location = ?
                       AND position < ?
                     ORDER BY position DESC, id DESC
                     LIMIT 1",
                    params![renderer_location, current_position],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()
                .map_err(db_error)?
        } else {
            transaction
                .query_row(
                    "SELECT id, position
                     FROM queue_entries
                     WHERE renderer_location = ?
                       AND position > ?
                     ORDER BY position ASC, id ASC
                     LIMIT 1",
                    params![renderer_location, current_position],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()
                .map_err(db_error)?
        };

        let Some((neighbor_id, neighbor_position)) = neighbor else {
            transaction.commit().map_err(db_error)?;
            return self
                .load_queue(renderer_location)?
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue not found"));
        };

        transaction
            .execute(
                "UPDATE queue_entries SET position = ? WHERE id = ?",
                params![neighbor_position, queue_entry_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE queue_entries SET position = ? WHERE id = ?",
                params![current_position, neighbor_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE playback_queues
                 SET updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![now_unix_timestamp(), renderer_location],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;

        Self::load_queue_with(&connection, renderer_location)?
            .ok_or_else(|| io::Error::other("queue disappeared after move"))
    }

    fn remove_queue_entry(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<PlaybackQueue> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();
        if current_entry_id == Some(queue_entry_id) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot remove the currently playing queue entry",
            ));
        }

        transaction
            .execute(
                "DELETE FROM queue_entries
                 WHERE renderer_location = ? AND id = ?",
                params![renderer_location, queue_entry_id],
            )
            .map_err(db_error)?;

        let ids = {
            let mut statement = transaction
                .prepare(
                    "SELECT id FROM queue_entries
                     WHERE renderer_location = ?
                     ORDER BY position ASC, id ASC",
                )
                .map_err(db_error)?;
            statement
                .query_map([renderer_location], |row| row.get::<_, i64>(0))
                .map_err(db_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_error)?
        };
        for (index, id) in ids.iter().enumerate() {
            transaction
                .execute(
                    "UPDATE queue_entries SET position = ? WHERE id = ?",
                    params![i64::try_from(index + 1).unwrap_or(i64::MAX), id],
                )
                .map_err(db_error)?;
        }
        transaction
            .execute(
                "UPDATE playback_queues
                 SET updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![now_unix_timestamp(), renderer_location],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;

        Self::load_queue_with(&connection, renderer_location)?
            .ok_or_else(|| io::Error::other("queue disappeared after remove"))
    }

    fn clear_queue(&self, renderer_location: &str) -> io::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM queue_entries WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "DELETE FROM playback_sessions WHERE renderer_location = ?",
                [renderer_location],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    fn mark_queue_play_started(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
        track_id: &str,
        current_track_uri: &str,
        duration_seconds: Option<u64>,
    ) -> io::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let now = now_unix_timestamp();
        transaction
            .execute(
                "UPDATE queue_entries
                 SET entry_status = 'playing', started_unix = COALESCE(started_unix, ?), completed_unix = NULL
                 WHERE id = ?",
                params![now, queue_entry_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE queue_entries
                 SET entry_status = CASE
                    WHEN id = ? THEN entry_status
                    WHEN completed_unix IS NOT NULL THEN 'completed'
                    ELSE 'pending'
                 END
                 WHERE renderer_location = ?",
                params![queue_entry_id, renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE playback_queues
                 SET current_entry_id = ?, status = 'playing', updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![queue_entry_id, now, renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (?, ?, NULL, 'PLAYING', ?, 0, ?, ?, NULL)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    queue_entry_id = excluded.queue_entry_id,
                    next_queue_entry_id = excluded.next_queue_entry_id,
                    transport_state = excluded.transport_state,
                    current_track_uri = excluded.current_track_uri,
                    position_seconds = excluded.position_seconds,
                    duration_seconds = excluded.duration_seconds,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    queue_entry_id,
                    current_track_uri,
                    duration_seconds,
                    now
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO track_play_history
                 (track_id, renderer_location, queue_entry_id, played_unix)
                 VALUES (?, ?, ?, ?)",
                params![track_id, renderer_location, queue_entry_id, now],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    fn adopt_next_queue_entry_as_current(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
        track_id: &str,
        current_track_uri: &str,
        duration_seconds: Option<u64>,
    ) -> io::Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let now = now_unix_timestamp();
        let previous_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();

        if let Some(previous_entry_id) = previous_entry_id {
            transaction
                .execute(
                    "UPDATE queue_entries
                     SET entry_status = 'completed',
                         completed_unix = COALESCE(completed_unix, ?)
                     WHERE id = ?",
                    params![now, previous_entry_id],
                )
                .map_err(db_error)?;
        }
        transaction
            .execute(
                "UPDATE queue_entries
                 SET entry_status = 'playing', started_unix = COALESCE(started_unix, ?), completed_unix = NULL
                 WHERE id = ?",
                params![now, queue_entry_id],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE queue_entries
                 SET entry_status = CASE
                    WHEN id = ? THEN entry_status
                    WHEN completed_unix IS NOT NULL THEN 'completed'
                    ELSE 'pending'
                 END
                 WHERE renderer_location = ?",
                params![queue_entry_id, renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE playback_queues
                 SET current_entry_id = ?, status = 'playing', updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![queue_entry_id, now, renderer_location],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (?, ?, NULL, 'PLAYING', ?, 0, ?, ?, NULL)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    queue_entry_id = excluded.queue_entry_id,
                    next_queue_entry_id = excluded.next_queue_entry_id,
                    transport_state = excluded.transport_state,
                    current_track_uri = excluded.current_track_uri,
                    position_seconds = excluded.position_seconds,
                    duration_seconds = excluded.duration_seconds,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    queue_entry_id,
                    current_track_uri,
                    duration_seconds,
                    now
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO track_play_history
                 (track_id, renderer_location, queue_entry_id, played_unix)
                 VALUES (?, ?, ?, ?)",
                params![track_id, renderer_location, queue_entry_id, now],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    fn mark_queue_play_error(
        &self,
        renderer_location: &str,
        queue_entry_id: Option<i64>,
        error: &str,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (?, ?, NULL, 'ERROR', NULL, NULL, NULL, ?, ?)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    queue_entry_id = excluded.queue_entry_id,
                    next_queue_entry_id = excluded.next_queue_entry_id,
                    transport_state = excluded.transport_state,
                    current_track_uri = excluded.current_track_uri,
                    position_seconds = excluded.position_seconds,
                    duration_seconds = excluded.duration_seconds,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    queue_entry_id,
                    now_unix_timestamp(),
                    error
                ],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "UPDATE playback_queues
                 SET status = 'error', updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![now_unix_timestamp(), renderer_location],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn record_transport_snapshot(
        &self,
        renderer_location: &str,
        transport_state: &str,
        current_track_uri: Option<&str>,
        position_seconds: Option<u64>,
        duration_seconds: Option<u64>,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (
                    ?,
                    (SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?),
                    (SELECT next_queue_entry_id FROM playback_sessions WHERE renderer_location = ?),
                    ?, ?, ?, ?, ?, NULL
                 )
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    transport_state = excluded.transport_state,
                    current_track_uri = excluded.current_track_uri,
                    position_seconds = excluded.position_seconds,
                    duration_seconds = excluded.duration_seconds,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    transport_state,
                    current_track_uri,
                    position_seconds,
                    duration_seconds,
                    now_unix_timestamp(),
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn record_transport_poll_error(&self, renderer_location: &str, error: &str) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (
                    ?,
                    (SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?),
                    (SELECT next_queue_entry_id FROM playback_sessions WHERE renderer_location = ?),
                    'ERROR', NULL, NULL, NULL, ?, ?
                 )
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    transport_state = excluded.transport_state,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    now_unix_timestamp(),
                    error
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn mark_next_queue_entry_preloaded(
        &self,
        renderer_location: &str,
        next_queue_entry_id: Option<i64>,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (
                    ?,
                    (SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?),
                    ?,
                    COALESCE((SELECT transport_state FROM playback_sessions WHERE renderer_location = ?), 'READY'),
                    (SELECT current_track_uri FROM playback_sessions WHERE renderer_location = ?),
                    (SELECT position_seconds FROM playback_sessions WHERE renderer_location = ?),
                    (SELECT duration_seconds FROM playback_sessions WHERE renderer_location = ?),
                    ?,
                    (SELECT last_error FROM playback_sessions WHERE renderer_location = ?)
                 )
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    next_queue_entry_id = excluded.next_queue_entry_id,
                    last_observed_unix = excluded.last_observed_unix",
                params![
                    renderer_location,
                    renderer_location,
                    next_queue_entry_id,
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    now_unix_timestamp(),
                    renderer_location,
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn sync_queue_status(&self, renderer_location: &str, queue_status: &str) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE playback_queues
                 SET status = ?, updated_unix = ?
                 WHERE renderer_location = ?
                   AND status != ?",
                params![
                    queue_status,
                    now_unix_timestamp(),
                    renderer_location,
                    queue_status
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn set_queue_status(
        &self,
        renderer_location: &str,
        queue_status: &str,
        transport_state: &str,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        let now = now_unix_timestamp();
        connection
            .execute(
                "UPDATE playback_queues
                 SET status = ?, updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![queue_status, now, renderer_location],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (
                    ?,
                    (SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?),
                    (SELECT next_queue_entry_id FROM playback_sessions WHERE renderer_location = ?),
                    ?,
                    (SELECT current_track_uri FROM playback_sessions WHERE renderer_location = ?),
                    (SELECT position_seconds FROM playback_sessions WHERE renderer_location = ?),
                    (SELECT duration_seconds FROM playback_sessions WHERE renderer_location = ?),
                    ?,
                    NULL
                 )
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    transport_state = excluded.transport_state,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    transport_state,
                    renderer_location,
                    renderer_location,
                    renderer_location,
                    now,
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn select_queue_entry(&self, renderer_location: &str, queue_entry_id: i64) -> io::Result<()> {
        let connection = self.connection()?;
        let now = now_unix_timestamp();
        connection
            .execute(
                "UPDATE playback_queues
                 SET current_entry_id = ?, status = 'ready', updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![queue_entry_id, now, renderer_location],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "UPDATE queue_entries
                 SET entry_status = CASE
                    WHEN id = ? THEN 'pending'
                    WHEN completed_unix IS NOT NULL THEN 'completed'
                    ELSE 'pending'
                 END
                 WHERE renderer_location = ?",
                params![queue_entry_id, renderer_location],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "INSERT INTO playback_sessions
                 (renderer_location, queue_entry_id, next_queue_entry_id, transport_state, current_track_uri,
                  position_seconds, duration_seconds, last_observed_unix, last_error)
                 VALUES (?, ?, NULL, 'READY', NULL, NULL, NULL, ?, NULL)
                 ON CONFLICT(renderer_location) DO UPDATE SET
                    queue_entry_id = excluded.queue_entry_id,
                    next_queue_entry_id = excluded.next_queue_entry_id,
                    transport_state = excluded.transport_state,
                    current_track_uri = excluded.current_track_uri,
                    position_seconds = excluded.position_seconds,
                    duration_seconds = excluded.duration_seconds,
                    last_observed_unix = excluded.last_observed_unix,
                    last_error = excluded.last_error",
                params![renderer_location, queue_entry_id, now],
            )
            .map_err(db_error)?;
        Ok(())
    }

    fn advance_queue_after_completion(&self, renderer_location: &str) -> io::Result<Option<i64>> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let current_entry_id = transaction
            .query_row(
                "SELECT current_entry_id FROM playback_queues WHERE renderer_location = ?",
                [renderer_location],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
            .map_err(db_error)?
            .flatten();
        let Some(current_entry_id) = current_entry_id else {
            transaction.commit().map_err(db_error)?;
            return Ok(None);
        };

        let now = now_unix_timestamp();
        transaction
            .execute(
                "UPDATE queue_entries
                 SET entry_status = 'completed',
                     completed_unix = COALESCE(completed_unix, ?)
                 WHERE id = ?",
                params![now, current_entry_id],
            )
            .map_err(db_error)?;

        let next_entry_id = transaction
            .query_row(
                "SELECT id
                 FROM queue_entries
                 WHERE renderer_location = ?
                   AND position > (
                       SELECT position FROM queue_entries WHERE id = ?
                   )
                 ORDER BY position ASC, id ASC
                 LIMIT 1",
                params![renderer_location, current_entry_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?;

        transaction
            .execute(
                "UPDATE playback_queues
                 SET current_entry_id = ?, status = ?, updated_unix = ?, version = version + 1
                 WHERE renderer_location = ?",
                params![
                    next_entry_id,
                    if next_entry_id.is_some() {
                        "ready"
                    } else {
                        "completed"
                    },
                    now,
                    renderer_location
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE playback_sessions
                 SET queue_entry_id = ?, next_queue_entry_id = NULL, transport_state = ?, current_track_uri = NULL,
                     position_seconds = NULL, duration_seconds = NULL, last_observed_unix = ?, last_error = NULL
                 WHERE renderer_location = ?",
                params![
                    next_entry_id,
                    if next_entry_id.is_some() {
                        "READY"
                    } else {
                        "COMPLETED"
                    },
                    now,
                    renderer_location
                ],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
        Ok(next_entry_id)
    }

    fn list_playing_queue_renderers(&self) -> io::Result<Vec<String>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT renderer_location
                 FROM playback_queues
                 WHERE status = 'playing'
                 ORDER BY updated_unix ASC, renderer_location ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;
        let mut locations = Vec::new();
        for row in rows {
            locations.push(row.map_err(db_error)?);
        }
        Ok(locations)
    }
}

impl ServiceState {
    fn debug_enabled(&self) -> bool {
        self.config.debug_mode
    }

    fn metrics(&self) -> Option<&metrics::Metrics> {
        self.metrics.get().map(|arc| arc.as_ref())
    }

    fn install_metrics(&self, metrics: Arc<metrics::Metrics>) {
        let _ = self.metrics.set(metrics);
    }

    fn debug_log(&self, event: &str, details: impl AsRef<str>) {
        if self.debug_enabled() {
            eprintln!(
                "[musicd-debug][{}][{}] {}",
                now_unix_timestamp(),
                event,
                details.as_ref()
            );
        }
    }

    fn load(config: AppConfig) -> io::Result<Self> {
        let database = Database::open(&config.config_path)?;
        let persisted_library = database.load_library(config.library_path.clone())?;
        let state = Self {
            config,
            database,
            library: Mutex::new(persisted_library),
            renderer_backends: RendererBackends::default(),
            metrics: OnceLock::new(),
        };

        match scan_library(&state.config.library_path, &state.config.config_path) {
            Ok(library) => state.replace_library(library)?,
            Err(error) if state.track_count() > 0 => {
                eprintln!("library scan failed, continuing with persisted index: {error}");
            }
            Err(error) => return Err(error),
        }

        state.debug_log(
            "service-load",
            format!(
                "tracks={} default_renderer={}",
                state.track_count(),
                state
                    .config
                    .default_renderer_location
                    .as_deref()
                    .unwrap_or("<none>")
            ),
        );

        Ok(state)
    }

    fn track_count(&self) -> usize {
        self.library
            .lock()
            .map(|library| library.tracks.len())
            .unwrap_or(0)
    }

    fn tracks_snapshot(&self) -> Vec<LibraryTrack> {
        self.library
            .lock()
            .map(|library| library.tracks.clone())
            .unwrap_or_default()
    }

    fn albums_snapshot(&self) -> Vec<AlbumSummary> {
        let mut albums = self.database.load_albums().unwrap_or_default();
        apply_album_artwork_overrides(
            &mut albums,
            &self
                .database
                .list_album_artwork_overrides()
                .unwrap_or_default(),
        );
        albums
    }

    fn artists_snapshot(&self) -> Vec<ArtistSummary> {
        let mut artists = self.database.load_artists().unwrap_or_default();
        let mut albums = self.database.load_albums().unwrap_or_default();
        apply_album_artwork_overrides(
            &mut albums,
            &self
                .database
                .list_album_artwork_overrides()
                .unwrap_or_default(),
        );
        hydrate_artist_artwork_urls(&mut artists, &albums);
        artists
    }

    fn find_track(&self, track_id: &str) -> Option<LibraryTrack> {
        self.library.lock().ok().and_then(|library| {
            library
                .tracks
                .iter()
                .find(|track| track.id == track_id)
                .cloned()
        })
    }

    fn find_album(&self, album_id: &str) -> Option<AlbumSummary> {
        self.albums_snapshot()
            .into_iter()
            .find(|album| album.id == album_id)
    }

    fn find_artist(&self, artist_id: &str) -> Option<ArtistSummary> {
        self.artists_snapshot()
            .into_iter()
            .find(|artist| artist.id == artist_id)
    }

    fn tracks_for_album(&self, album_id: &str) -> Vec<LibraryTrack> {
        self.library
            .lock()
            .map(|library| {
                let mut tracks = library
                    .tracks
                    .iter()
                    .filter(|track| track.album_id == album_id)
                    .cloned()
                    .collect::<Vec<_>>();
                tracks.sort_by(compare_track_album_order);
                tracks
            })
            .unwrap_or_default()
    }

    fn first_track_for_album(&self, album_id: &str) -> Option<LibraryTrack> {
        self.tracks_for_album(album_id).into_iter().next()
    }

    fn albums_for_artist(&self, artist_id: &str) -> Vec<AlbumSummary> {
        self.albums_snapshot()
            .into_iter()
            .filter(|album| album.artist_id == artist_id)
            .collect()
    }

    fn queue_snapshot(&self, renderer_location: &str) -> Option<PlaybackQueue> {
        self.database.load_queue(renderer_location).ok().flatten()
    }

    fn playback_session(&self, renderer_location: &str) -> Option<PlaybackSession> {
        self.database
            .load_playback_session(renderer_location)
            .ok()
            .flatten()
    }

    fn rescan(&self) -> io::Result<usize> {
        let library = scan_library(&self.config.library_path, &self.config.config_path)?;
        let track_count = library.tracks.len();
        self.replace_library(library)?;
        Ok(track_count)
    }

    fn replace_library(&self, library: Library) -> io::Result<()> {
        self.database.save_library(&library)?;
        let mut state = self
            .library
            .lock()
            .map_err(|_| io::Error::other("library state lock poisoned"))?;
        *state = library;
        Ok(())
    }

    fn renderer_snapshot(&self) -> Vec<RendererRecord> {
        self.database.list_renderers().unwrap_or_default()
    }

    fn enriched_renderer_snapshot(&self) -> Vec<RendererRecord> {
        self.renderer_snapshot()
            .into_iter()
            .filter_map(|renderer| {
                let renderer = self
                    .enrich_renderer_record_if_needed(&renderer)
                    .unwrap_or(renderer);
                renderer_is_viable(&renderer).then_some(renderer)
            })
            .collect()
    }

    fn enriched_renderer_record(&self, renderer_location: &str) -> Option<RendererRecord> {
        self.database
            .load_renderer(renderer_location)
            .ok()
            .flatten()
            .and_then(|renderer| {
                let renderer = self
                    .enrich_renderer_record_if_needed(&renderer)
                    .unwrap_or(renderer);
                renderer_is_viable(&renderer).then_some(renderer)
            })
    }

    fn enrich_renderer_record_if_needed(
        &self,
        renderer: &RendererRecord,
    ) -> io::Result<RendererRecord> {
        if !renderer_needs_refresh(renderer) {
            return Ok(renderer.clone());
        }
        let resolved = self.resolve_renderer(&renderer.location)?;
        Ok(resolved)
    }

    fn preferred_renderer_location(&self, requested: Option<&str>) -> String {
        requested
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                self.database
                    .last_selected_renderer_location()
                    .ok()
                    .flatten()
            })
            .or_else(|| self.config.default_renderer_location.clone())
            .unwrap_or_default()
    }

    fn remember_renderer_location(&self, location: &str) -> io::Result<()> {
        if let Some(existing) = self.database.load_renderer(location)? {
            if !renderer_name_looks_like_location(&existing.name, location) {
                self.database
                    .set_last_selected_renderer_location(location)?;
                return Ok(());
            }
        }

        if matches!(renderer_kind_for_location(location), RendererKind::Upnp) {
            if let Ok(details) = inspect_renderer(location) {
                return self.remember_renderer_details(
                    location,
                    &details.friendly_name,
                    details.manufacturer.as_deref(),
                    details.model_name.as_deref(),
                    Some(&details.av_transport_control_url),
                    Some(&details.capabilities),
                    None,
                );
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "location did not resolve to a playable UPnP media renderer",
            ));
        }

        if matches!(
            renderer_kind_for_location(location),
            RendererKind::AndroidLocal
        ) {
            let renderer = RendererRecord {
                location: location.to_string(),
                name: "This phone".to_string(),
                manufacturer: Some("Android".to_string()),
                model_name: None,
                av_transport_control_url: None,
                capabilities: android_local_renderer_capabilities(),
                last_checked_unix: now_unix_timestamp(),
                last_reachable_unix: Some(now_unix_timestamp()),
                last_error: None,
                last_seen_unix: now_unix_timestamp(),
            };
            self.database.upsert_renderer(&renderer)?;
            self.database.set_last_selected_renderer_location(location)?;
            return Ok(());
        }

        let renderer = RendererRecord {
            location: location.to_string(),
            name: renderer_location_host(location)
                .unwrap_or(location)
                .to_string(),
            manufacturer: None,
            model_name: None,
            av_transport_control_url: None,
            capabilities: RendererCapabilities::default(),
            last_checked_unix: 0,
            last_reachable_unix: None,
            last_error: None,
            last_seen_unix: 0,
        };
        self.database.upsert_renderer(&renderer)?;
        self.database.set_last_selected_renderer_location(location)
    }

    fn remember_renderer_details(
        &self,
        location: &str,
        name: &str,
        manufacturer: Option<&str>,
        model_name: Option<&str>,
        av_transport_control_url: Option<&str>,
        capabilities: Option<&RendererCapabilities>,
        last_error: Option<&str>,
    ) -> io::Result<()> {
        let existing = self.database.load_renderer(location)?;
        let now = now_unix_timestamp();
        let last_reachable_unix = if last_error.is_none() {
            Some(now)
        } else {
            existing
                .as_ref()
                .and_then(|renderer| renderer.last_reachable_unix)
        };
        let last_seen_unix = last_reachable_unix.unwrap_or(0);
        let renderer = RendererRecord {
            location: location.to_string(),
            name: normalized_renderer_name(location, name, model_name),
            manufacturer: manufacturer.map(ToString::to_string),
            model_name: model_name.map(ToString::to_string),
            av_transport_control_url: av_transport_control_url.map(ToString::to_string),
            capabilities: capabilities
                .cloned()
                .or_else(|| {
                    existing
                        .as_ref()
                        .map(|renderer| renderer.capabilities.clone())
                })
                .unwrap_or_default(),
            last_checked_unix: now,
            last_reachable_unix,
            last_error: last_error.map(ToString::to_string),
            last_seen_unix,
        };
        self.database.upsert_renderer(&renderer)?;
        self.database.set_last_selected_renderer_location(location)
    }

    fn remember_renderer_record(&self, renderer: &RendererRecord) -> io::Result<()> {
        self.database.upsert_renderer(renderer)?;
        self.database
            .set_last_selected_renderer_location(&renderer.location)
    }

    fn mark_renderer_reachable(&self, renderer: &RendererRecord) -> io::Result<()> {
        let mut updated = renderer.clone();
        let now = now_unix_timestamp();
        updated.last_checked_unix = now;
        updated.last_reachable_unix = Some(now);
        updated.last_seen_unix = now;
        updated.last_error = None;
        self.database.upsert_renderer(&updated)
    }

    fn mark_renderer_unreachable(
        &self,
        renderer_location: &str,
        error: &io::Error,
    ) -> io::Result<()> {
        let mut renderer =
            self.database
                .load_renderer(renderer_location)?
                .unwrap_or(RendererRecord {
                    location: renderer_location.to_string(),
                    name: renderer_location_host(renderer_location)
                        .unwrap_or(renderer_location)
                        .to_string(),
                    manufacturer: None,
                    model_name: None,
                    av_transport_control_url: None,
                    capabilities: RendererCapabilities::default(),
                    last_checked_unix: 0,
                    last_reachable_unix: None,
                    last_error: None,
                    last_seen_unix: 0,
                });
        renderer.last_checked_unix = now_unix_timestamp();
        renderer.last_error = Some(error.to_string());
        self.database.upsert_renderer(&renderer)
    }

    fn track_artwork_path(&self, track: &LibraryTrack) -> Option<PathBuf> {
        track
            .artwork
            .as_ref()
            .map(|artwork| artwork_cache_path(&self.config.config_path, &artwork.cache_key))
    }

    fn album_artwork_path(&self, album_id: &str) -> Option<PathBuf> {
        self.find_album(album_id).and_then(|album| {
            album
                .artwork
                .as_ref()
                .map(|artwork| artwork_cache_path(&self.config.config_path, &artwork.cache_key))
        })
    }

    fn artwork_url_for_track(&self, track: &LibraryTrack) -> Option<String> {
        self.relative_artwork_url_for_track(track).map(|artwork_url| {
            format!(
                "{}/{}",
                self.config.resolved_base_url().trim_end_matches('/'),
                artwork_url.trim_start_matches('/')
            )
        })
    }

    fn relative_artwork_url_for_track(&self, track: &LibraryTrack) -> Option<String> {
        if track.artwork.is_some() {
            return Some(format!("/artwork/track/{}", track.id));
        }

        self.find_album(&track.album_id)
            .filter(|album| album.artwork.is_some())
            .map(|_| format!("/artwork/album/{}", track.album_id))
    }

    fn search_album_artwork_candidates(
        &self,
        album_id: &str,
    ) -> io::Result<Vec<AlbumArtworkSearchCandidate>> {
        let album = self
            .find_album(album_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "album not found"))?;
        let client = musicbrainz_client()?;
        search_musicbrainz_album_artwork(&client, &album.artist, &album.title)
    }

    fn apply_album_artwork_candidate(
        &self,
        album_id: &str,
        release_id: &str,
    ) -> io::Result<AlbumSummary> {
        let album = self
            .find_album(album_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "album not found"))?;
        let client = musicbrainz_client()?;
        let candidate = fetch_musicbrainz_cover_art_for_release(&client, release_id)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no front artwork found"))?;
        let downloaded = download_artwork_candidate(&client, &candidate.image_url)?;
        let cache_key = stable_track_id(&format!("mb-release:{}:{}", album.id, release_id));
        let extension = image_extension_for_mime(&downloaded.mime_type).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported remote artwork MIME type",
            )
        })?;
        let destination = self
            .config
            .config_path
            .join("artwork")
            .join(format!("{cache_key}.{extension}"));
        fs::write(&destination, downloaded.bytes)?;

        let override_record = AlbumArtworkOverride {
            album_id: album.id.clone(),
            cache_key: format!("{cache_key}.{extension}"),
            source: candidate.source,
            mime_type: downloaded.mime_type,
            musicbrainz_release_id: Some(release_id.to_string()),
            applied_unix: now_unix_timestamp(),
        };
        self.database
            .upsert_album_artwork_override(&override_record)?;
        self.find_album(album_id)
            .ok_or_else(|| io::Error::other("updated album could not be reloaded"))
    }

    fn renderer_backend(&self, renderer_location: &str) -> io::Result<&dyn RendererBackend> {
        self.renderer_backends
            .backend_for_location(renderer_location)
    }

    fn resolve_renderer(&self, renderer_location: &str) -> io::Result<RendererRecord> {
        let cached = self.database.load_renderer(renderer_location)?;
        let renderer = match self
            .renderer_backend(renderer_location)?
            .resolve_renderer(cached.as_ref(), renderer_location)
        {
            Ok(renderer) => renderer,
            Err(error) => {
                let _ = self.mark_renderer_unreachable(renderer_location, &error);
                return Err(error);
            }
        };
        if cached.as_ref() != Some(&renderer) {
            let _ = self.remember_renderer_record(&renderer);
        }
        Ok(renderer)
    }

    fn replace_queue_with_track(
        &self,
        renderer_location: &str,
        track: &LibraryTrack,
    ) -> io::Result<PlaybackQueue> {
        self.database.replace_queue(
            renderer_location,
            &format!("Track: {}", track.title),
            &[QueueMutationEntry {
                track_id: track.id.clone(),
                album_id: Some(track.album_id.clone()),
                source_kind: "track".to_string(),
                source_ref: Some(track.id.clone()),
            }],
        )
    }

    fn replace_queue_with_album(
        &self,
        renderer_location: &str,
        album: &AlbumSummary,
    ) -> io::Result<PlaybackQueue> {
        let tracks = self.tracks_for_album(&album.id);
        let entries = tracks
            .into_iter()
            .map(|track| QueueMutationEntry {
                track_id: track.id,
                album_id: Some(album.id.clone()),
                source_kind: "album".to_string(),
                source_ref: Some(album.id.clone()),
            })
            .collect::<Vec<_>>();
        self.database
            .replace_queue(renderer_location, &album.title, &entries)
    }

    fn append_track_to_queue(
        &self,
        renderer_location: &str,
        track: &LibraryTrack,
    ) -> io::Result<PlaybackQueue> {
        self.database.append_queue_entries(
            renderer_location,
            &format!("Track: {}", track.title),
            &[QueueMutationEntry {
                track_id: track.id.clone(),
                album_id: Some(track.album_id.clone()),
                source_kind: "track".to_string(),
                source_ref: Some(track.id.clone()),
            }],
        )
    }

    fn append_album_to_queue(
        &self,
        renderer_location: &str,
        album: &AlbumSummary,
    ) -> io::Result<PlaybackQueue> {
        let tracks = self.tracks_for_album(&album.id);
        let entries = tracks
            .into_iter()
            .map(|track| QueueMutationEntry {
                track_id: track.id,
                album_id: Some(album.id.clone()),
                source_kind: "album".to_string(),
                source_ref: Some(album.id.clone()),
            })
            .collect::<Vec<_>>();
        self.database
            .append_queue_entries(renderer_location, &album.title, &entries)
    }

    fn play_next_track(
        &self,
        renderer_location: &str,
        track: &LibraryTrack,
    ) -> io::Result<PlaybackQueue> {
        self.database.insert_queue_entries_after_current(
            renderer_location,
            &format!("Track: {}", track.title),
            &[QueueMutationEntry {
                track_id: track.id.clone(),
                album_id: Some(track.album_id.clone()),
                source_kind: "track".to_string(),
                source_ref: Some(track.id.clone()),
            }],
        )
    }

    fn play_next_album(
        &self,
        renderer_location: &str,
        album: &AlbumSummary,
    ) -> io::Result<PlaybackQueue> {
        let tracks = self.tracks_for_album(&album.id);
        let entries = tracks
            .into_iter()
            .map(|track| QueueMutationEntry {
                track_id: track.id,
                album_id: Some(album.id.clone()),
                source_kind: "album".to_string(),
                source_ref: Some(album.id.clone()),
            })
            .collect::<Vec<_>>();
        self.database
            .insert_queue_entries_after_current(renderer_location, &album.title, &entries)
    }

    fn stream_resource_for_track(&self, track: &LibraryTrack) -> StreamResource {
        StreamResource {
            stream_url: format!(
                "{}/stream/track/{}",
                self.config.resolved_base_url().trim_end_matches('/'),
                track.id
            ),
            mime_type: track.mime_type.clone(),
            title: track.title.clone(),
            album_art_url: self.artwork_url_for_track(track),
        }
    }

    fn clear_queue(&self, renderer_location: &str) -> io::Result<()> {
        self.database.clear_queue(renderer_location)
    }

    fn move_queue_entry_up(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<PlaybackQueue> {
        self.database
            .move_queue_entry(renderer_location, queue_entry_id, -1)
    }

    fn move_queue_entry_down(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<PlaybackQueue> {
        self.database
            .move_queue_entry(renderer_location, queue_entry_id, 1)
    }

    fn remove_pending_queue_entry(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<PlaybackQueue> {
        self.database
            .remove_queue_entry(renderer_location, queue_entry_id)
    }

    fn refresh_transport_state(&self, renderer_location: &str) -> io::Result<TransportSnapshot> {
        let renderer = self.resolve_renderer(renderer_location)?;
        let snapshot = match self
            .renderer_backend(renderer_location)?
            .transport_snapshot(&renderer)
        {
            Ok(snapshot) => {
                let _ = self.mark_renderer_reachable(&renderer);
                snapshot
            }
            Err(error) => {
                let _ = self.mark_renderer_unreachable(renderer_location, &error);
                return Err(error);
            }
        };
        self.database.record_transport_snapshot(
            renderer_location,
            &snapshot.transport_info.transport_state,
            snapshot.position_info.track_uri.as_deref(),
            snapshot.position_info.rel_time_seconds,
            snapshot.position_info.track_duration_seconds,
        )?;
        Ok(snapshot)
    }

    fn wait_for_transport_state(
        &self,
        renderer_location: &str,
        stable_states: &[&str],
        attempts: usize,
        delay: Duration,
    ) -> io::Result<TransportSnapshot> {
        let mut last_snapshot = self.refresh_transport_state(renderer_location)?;
        self.debug_log(
            "transport-wait",
            format!(
                "renderer={} initial_state={} stable={:?}",
                renderer_location, last_snapshot.transport_info.transport_state, stable_states
            ),
        );
        for _ in 0..attempts {
            if stable_states.contains(&last_snapshot.transport_info.transport_state.as_str()) {
                return Ok(last_snapshot);
            }
            thread::sleep(delay);
            last_snapshot = self.refresh_transport_state(renderer_location)?;
            self.debug_log(
                "transport-wait",
                format!(
                    "renderer={} observed_state={}",
                    renderer_location, last_snapshot.transport_info.transport_state
                ),
            );
        }
        Ok(last_snapshot)
    }

    fn resume_renderer(&self, renderer_location: &str) -> io::Result<String> {
        let _ = self.remember_renderer_location(renderer_location);
        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::AndroidLocal
        ) {
            if let Some(queue) = self.queue_snapshot(renderer_location) {
                if queue.current_entry_id.is_some() {
                    let session = self.playback_session(renderer_location);
                    if matches!(
                        session
                            .as_ref()
                            .map(|session| session.transport_state.as_str()),
                        Some("PAUSED_PLAYBACK")
                    ) {
                        self.database
                            .set_queue_status(renderer_location, "playing", "PLAYING")?;
                        return Ok("Playback resumed.".to_string());
                    }
                    let (track, _, renderer_name, _) =
                        self.start_current_queue_entry(renderer_location)?;
                    return Ok(format!(
                        "Now playing '{}' on {}.",
                        track.title, renderer_name
                    ));
                }
            }
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "queue is empty",
            ));
        }
        let queue = self.queue_snapshot(renderer_location);
        let session = self.playback_session(renderer_location);
        self.debug_log(
            "resume-request",
            format!(
                "renderer={} queue_current={:?} session_state={}",
                renderer_location,
                queue.as_ref().and_then(|queue| queue.current_entry_id),
                session
                    .as_ref()
                    .map(|session| session.transport_state.as_str())
                    .unwrap_or("<none>")
            ),
        );

        if let Some(queue) = queue.as_ref() {
            if session
                .as_ref()
                .map(|session| {
                    matches!(
                        session.transport_state.as_str(),
                        "STOPPED" | "NO_MEDIA_PRESENT" | "READY" | "COMPLETED"
                    )
                })
                .unwrap_or(true)
                && queue.current_entry_id.is_some()
            {
                let (track, _, renderer_name, _) =
                    self.start_current_queue_entry(renderer_location)?;
                return Ok(format!(
                    "Now playing '{}' on {}.",
                    track.title, renderer_name
                ));
            }
        }

        let renderer = self.resolve_renderer(renderer_location)?;
        if let Err(error) = self.renderer_backend(renderer_location)?.play(&renderer) {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        let snapshot = self.refresh_transport_state(renderer_location)?;
        self.database.set_queue_status(
            renderer_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
            &snapshot.transport_info.transport_state,
        )?;
        Ok("Playback resumed.".to_string())
    }

    fn pause_renderer(&self, renderer_location: &str) -> io::Result<String> {
        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::AndroidLocal
        ) {
            self.database
                .mark_next_queue_entry_preloaded(renderer_location, None)?;
            self.database
                .set_queue_status(renderer_location, "paused", "PAUSED_PLAYBACK")?;
            return Ok("Playback paused.".to_string());
        }
        let renderer = self.resolve_renderer(renderer_location)?;
        self.debug_log("pause-request", format!("renderer={renderer_location}"));
        if let Err(error) = self.renderer_backend(renderer_location)?.pause(&renderer) {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        let snapshot = self.wait_for_transport_state(
            renderer_location,
            &["PAUSED_PLAYBACK", "STOPPED", "NO_MEDIA_PRESENT"],
            6,
            Duration::from_millis(250),
        )?;
        self.database
            .mark_next_queue_entry_preloaded(renderer_location, None)?;
        self.database.set_queue_status(
            renderer_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
            &snapshot.transport_info.transport_state,
        )?;
        self.debug_log(
            "pause-settled",
            format!(
                "renderer={} state={} position={:?} duration={:?}",
                renderer_location,
                snapshot.transport_info.transport_state,
                snapshot.position_info.rel_time_seconds,
                snapshot.position_info.track_duration_seconds
            ),
        );
        if snapshot.transport_info.transport_state == "PAUSED_PLAYBACK" {
            Ok("Playback paused.".to_string())
        } else {
            Ok(format!(
                "Pause requested. Renderer now reports {}.",
                snapshot.transport_info.transport_state
            ))
        }
    }

    fn stop_renderer(&self, renderer_location: &str) -> io::Result<String> {
        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::AndroidLocal
        ) {
            self.database
                .mark_next_queue_entry_preloaded(renderer_location, None)?;
            self.database
                .set_queue_status(renderer_location, "stopped", "STOPPED")?;
            return Ok("Playback stopped.".to_string());
        }
        let renderer = self.resolve_renderer(renderer_location)?;
        self.debug_log("stop-request", format!("renderer={renderer_location}"));
        if let Err(error) = self.renderer_backend(renderer_location)?.stop(&renderer) {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        let snapshot = self.refresh_transport_state(renderer_location)?;
        self.database
            .mark_next_queue_entry_preloaded(renderer_location, None)?;
        self.database.set_queue_status(
            renderer_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
            &snapshot.transport_info.transport_state,
        )?;
        Ok("Playback stopped.".to_string())
    }

    fn skip_to_next(&self, renderer_location: &str) -> io::Result<String> {
        self.debug_log("next-request", format!("renderer={renderer_location}"));
        if let Some(queue) = self.queue_snapshot(renderer_location) {
            if let Some(current_entry_id) = queue.current_entry_id {
                if let Some(next_entry) = next_queue_entry_after(&queue, current_entry_id) {
                    self.database
                        .select_queue_entry(renderer_location, next_entry.id)?;
                    let (track, _, renderer_name, _) =
                        self.start_current_queue_entry(renderer_location)?;
                    return Ok(format!(
                        "Skipped to '{}' on {}.",
                        track.title, renderer_name
                    ));
                }
            }
        }

        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::AndroidLocal
        ) {
            return Ok("No later track in the local queue.".to_string());
        }

        let renderer = self.resolve_renderer(renderer_location)?;
        if let Err(error) = self.renderer_backend(renderer_location)?.next(&renderer) {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        let snapshot = self.refresh_transport_state(renderer_location)?;
        self.database.set_queue_status(
            renderer_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
            &snapshot.transport_info.transport_state,
        )?;
        Ok("Skipped to the next track.".to_string())
    }

    fn skip_to_previous(&self, renderer_location: &str) -> io::Result<String> {
        self.debug_log("previous-request", format!("renderer={renderer_location}"));
        if let Some(queue) = self.queue_snapshot(renderer_location) {
            if let Some(current_entry_id) = queue.current_entry_id {
                if let Some(previous_entry) = previous_queue_entry_before(&queue, current_entry_id)
                {
                    self.database
                        .select_queue_entry(renderer_location, previous_entry.id)?;
                    let (track, _, renderer_name, _) =
                        self.start_current_queue_entry(renderer_location)?;
                    return Ok(format!(
                        "Went back to '{}' on {}.",
                        track.title, renderer_name
                    ));
                }
            }
        }

        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::AndroidLocal
        ) {
            return Ok("No earlier track in the local queue.".to_string());
        }

        let renderer = self.resolve_renderer(renderer_location)?;
        if let Err(error) = self
            .renderer_backend(renderer_location)?
            .previous(&renderer)
        {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        let snapshot = self.refresh_transport_state(renderer_location)?;
        self.database.set_queue_status(
            renderer_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
            &snapshot.transport_info.transport_state,
        )?;
        Ok("Moved to the previous track.".to_string())
    }

    fn preload_next_queue_entry(
        &self,
        renderer_location: &str,
        renderer: &RendererRecord,
        queue: &PlaybackQueue,
        current_entry_id: i64,
    ) -> io::Result<()> {
        let Some(next_entry) = next_queue_entry_after(queue, current_entry_id) else {
            self.database
                .mark_next_queue_entry_preloaded(renderer_location, None)?;
            return Ok(());
        };

        let session = self.playback_session(renderer_location);
        if session
            .as_ref()
            .and_then(|session| session.next_queue_entry_id)
            == Some(next_entry.id)
        {
            return Ok(());
        }

        let track = self.find_track(&next_entry.track_id).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "queued next track not found")
        })?;
        let resource = self.stream_resource_for_track(&track);
        if renderer.capabilities.supports_set_next_av_transport_uri() == Some(false) {
            self.database
                .mark_next_queue_entry_preloaded(renderer_location, None)?;
            return Ok(());
        }
        if let Err(error) = self
            .renderer_backend(renderer_location)?
            .preload_next(renderer, &resource)
        {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        self.database
            .mark_next_queue_entry_preloaded(renderer_location, Some(next_entry.id))?;
        self.debug_log(
            "preload-next",
            format!(
                "renderer={} current_entry={} next_entry={} next_track={}",
                renderer_location, current_entry_id, next_entry.id, track.title
            ),
        );
        Ok(())
    }

    fn adopt_renderer_advanced_entry(
        &self,
        renderer_location: &str,
        queue: &PlaybackQueue,
        snapshot: &TransportSnapshot,
    ) -> io::Result<bool> {
        let Some(current_entry_id) = queue.current_entry_id else {
            return Ok(false);
        };
        let Some(next_entry) = next_queue_entry_after(queue, current_entry_id) else {
            return Ok(false);
        };
        let Some(track_uri) = snapshot.position_info.track_uri.as_deref() else {
            return Ok(false);
        };

        let next_track = self.find_track(&next_entry.track_id).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "queued next track not found")
        })?;
        let expected_stream_url = self.stream_resource_for_track(&next_track).stream_url;
        if !should_adopt_preloaded_next_entry(queue, snapshot, Some(&expected_stream_url)) {
            return Ok(false);
        }

        self.database.adopt_next_queue_entry_as_current(
            renderer_location,
            next_entry.id,
            &next_track.id,
            track_uri,
            next_track.duration_seconds,
        )?;
        self.debug_log(
            "renderer-advanced",
            format!(
                "renderer={} adopted_entry={} track={} uri={}",
                renderer_location, next_entry.id, next_track.title, track_uri
            ),
        );
        Ok(true)
    }

    fn start_current_queue_entry(
        &self,
        renderer_location: &str,
    ) -> io::Result<(LibraryTrack, i64, String, String)> {
        let queue = self
            .database
            .load_queue(renderer_location)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue not found"))?;
        let current_entry = queue
            .entries
            .iter()
            .find(|entry| Some(entry.id) == queue.current_entry_id)
            .or_else(|| queue.entries.first())
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue is empty"))?;
        let track = self
            .find_track(&current_entry.track_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queued track not found"))?;
        let resource = self.stream_resource_for_track(&track);
        let stream_url = resource.stream_url.clone();
        self.debug_log(
            "queue-start",
            format!(
                "renderer={} entry={} track={} uri={}",
                renderer_location, current_entry.id, track.title, stream_url
            ),
        );
        let renderer = self.resolve_renderer(renderer_location)?;

        match self
            .renderer_backend(renderer_location)?
            .play_stream(&renderer, &resource)
        {
            Ok(()) => {
                let _ = self.mark_renderer_reachable(&renderer);
                self.database.mark_queue_play_started(
                    renderer_location,
                    current_entry.id,
                    &track.id,
                    &stream_url,
                    track.duration_seconds,
                )?;
                if let Err(error) = self.preload_next_queue_entry(
                    renderer_location,
                    &renderer,
                    &queue,
                    current_entry.id,
                ) {
                    eprintln!("next-track preload failed for {renderer_location}: {error}");
                }
                Ok((
                    track,
                    current_entry.id,
                    renderer.name.clone(),
                    renderer.location.clone(),
                ))
            }
            Err(error) => {
                let _ = self.mark_renderer_unreachable(renderer_location, &error);
                let _ = self.database.mark_queue_play_error(
                    renderer_location,
                    Some(current_entry.id),
                    &error.to_string(),
                );
                Err(error)
            }
        }
    }

    fn poll_active_queues(&self) -> io::Result<()> {
        for renderer_location in self.database.list_playing_queue_renderers()? {
            if matches!(
                renderer_kind_for_location(&renderer_location),
                RendererKind::AndroidLocal
            ) {
                continue;
            }
            self.debug_log("queue-poll", format!("renderer={renderer_location}"));
            if let Err(error) = self.poll_renderer_queue(&renderer_location) {
                eprintln!("queue poll failed for {renderer_location}: {error}");
            }
        }
        Ok(())
    }

    fn poll_renderer_queue(&self, renderer_location: &str) -> io::Result<()> {
        let mut queue = match self.queue_snapshot(renderer_location) {
            Some(queue) => queue,
            None => return Ok(()),
        };
        let session = self.playback_session(renderer_location);
        let previous_queue_status = queue.status.clone();
        let renderer = self.resolve_renderer(renderer_location)?;
        let snapshot = match self
            .renderer_backend(renderer_location)?
            .transport_snapshot(&renderer)
        {
            Ok(snapshot) => {
                let _ = self.mark_renderer_reachable(&renderer);
                snapshot
            }
            Err(error) => {
                let _ = self.mark_renderer_unreachable(renderer_location, &error);
                let _ = self
                    .database
                    .record_transport_poll_error(renderer_location, &error.to_string());
                return Err(error);
            }
        };

        self.database.record_transport_snapshot(
            renderer_location,
            &snapshot.transport_info.transport_state,
            snapshot.position_info.track_uri.as_deref(),
            snapshot.position_info.rel_time_seconds,
            snapshot.position_info.track_duration_seconds,
        )?;
        self.database.sync_queue_status(
            renderer_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
        )?;
        if self.debug_enabled() {
            let previous_state = session
                .as_ref()
                .map(|session| session.transport_state.as_str())
                .unwrap_or("<none>");
            if previous_state != snapshot.transport_info.transport_state
                || previous_queue_status
                    != queue_status_for_transport(&snapshot.transport_info.transport_state)
            {
                self.debug_log(
                    "transport-transition",
                    format!(
                        "renderer={} session_state={} -> {} queue_status={} -> {} position={:?} duration={:?}",
                        renderer_location,
                        previous_state,
                        snapshot.transport_info.transport_state,
                        previous_queue_status,
                        queue_status_for_transport(&snapshot.transport_info.transport_state),
                        snapshot.position_info.rel_time_seconds,
                        snapshot.position_info.track_duration_seconds
                    ),
                );
            }
        }

        if self.adopt_renderer_advanced_entry(renderer_location, &queue, &snapshot)? {
            if let Some(updated_queue) = self.queue_snapshot(renderer_location) {
                queue = updated_queue;
            }
        }

        if let Some(current_entry_id) = queue.current_entry_id.filter(|_| {
            matches!(
                snapshot.transport_info.transport_state.as_str(),
                "PLAYING" | "TRANSITIONING"
            )
        }) {
            if let Err(error) = self.preload_next_queue_entry(
                renderer_location,
                &renderer,
                &queue,
                current_entry_id,
            ) {
                eprintln!("next-track preload refresh failed for {renderer_location}: {error}");
            }
        }

        if matches!(
            snapshot.transport_info.transport_state.as_str(),
            "STOPPED" | "NO_MEDIA_PRESENT"
        ) && matches!(
            session
                .as_ref()
                .map(|session| session.transport_state.as_str()),
            Some("PAUSED_PLAYBACK")
        ) {
            self.debug_log(
                "auto-advance-suppressed",
                format!(
                    "renderer={} stopped_after_pause position={:?} duration={:?}",
                    renderer_location,
                    snapshot.position_info.rel_time_seconds,
                    snapshot.position_info.track_duration_seconds
                ),
            );
        }

        if should_auto_advance(&queue, session.as_ref(), &snapshot, self) {
            self.debug_log(
                "auto-advance",
                format!(
                    "renderer={} current_entry={:?} position={:?} duration={:?}",
                    renderer_location,
                    queue.current_entry_id,
                    snapshot.position_info.rel_time_seconds,
                    snapshot.position_info.track_duration_seconds
                ),
            );
            let next_entry_id = self
                .database
                .advance_queue_after_completion(renderer_location)?;
            if next_entry_id.is_some() {
                let _ = self.start_current_queue_entry(renderer_location)?;
            }
        }

        Ok(())
    }
}

fn scan_library(root: &Path, config_path: &Path) -> io::Result<Library> {
    if !root.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("library path does not exist: {}", root.display()),
        ));
    }

    let artwork_cache_dir = config_path.join("artwork");
    fs::create_dir_all(&artwork_cache_dir)?;
    let mut tracks = Vec::new();
    scan_dir(root, root, &artwork_cache_dir, &mut tracks)?;
    tracks.sort_by(compare_library_tracks);

    Ok(Library {
        scan_root: root.to_path_buf(),
        tracks,
    })
}

fn build_album_summaries(tracks: &[LibraryTrack]) -> Vec<AlbumSummary> {
    let mut grouped = HashMap::<String, Vec<LibraryTrack>>::new();
    for track in tracks {
        grouped
            .entry(track.album_id.clone())
            .or_default()
            .push(track.clone());
    }

    let mut albums = grouped
        .into_iter()
        .filter_map(|(album_id, mut album_tracks)| {
            album_tracks.sort_by(compare_track_album_order);
            let first_track = album_tracks.first()?.clone();
            let album_artwork_url = format!("/artwork/album/{album_id}");
            let artwork_track_id = album_tracks
                .iter()
                .find(|track| track.artwork.is_some())
                .map(|track| track.id.clone());
            let artwork = album_tracks.iter().find_map(|track| track.artwork.clone());
            Some(AlbumSummary {
                id: album_id,
                artist_id: stable_artist_id(&first_track.artist),
                title: first_track.album.clone(),
                artist: first_track.artist.clone(),
                track_count: album_tracks.len(),
                artwork_track_id: artwork_track_id.clone(),
                artwork: artwork.clone(),
                artwork_url: artwork.map(|_| album_artwork_url),
                first_track_id: first_track.id.clone(),
            })
        })
        .collect::<Vec<_>>();

    albums.sort_by(compare_albums);
    albums
}

fn apply_album_artwork_overrides(albums: &mut [AlbumSummary], overrides: &[AlbumArtworkOverride]) {
    let override_records = overrides
        .iter()
        .map(|override_record| {
            (
                override_record.album_id.clone(),
                (
                    TrackArtwork {
                        cache_key: override_record.cache_key.clone(),
                        source: override_record.source.clone(),
                        mime_type: override_record.mime_type.clone(),
                    },
                    format!("/artwork/album/{}", override_record.album_id),
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    for album in albums {
        if let Some((override_artwork, override_url)) = override_records.get(&album.id) {
            album.artwork = Some(override_artwork.clone());
            album.artwork_url = Some(override_url.clone());
        }
    }
}

fn hydrate_artist_artwork_urls(artists: &mut [ArtistSummary], albums: &[AlbumSummary]) {
    let artwork_by_artist = albums
        .iter()
        .filter_map(|album| {
            album
                .artwork_url
                .as_ref()
                .map(|artwork_url| (album.artist_id.clone(), artwork_url.clone()))
        })
        .collect::<HashMap<_, _>>();

    for artist in artists {
        if let Some(artwork_url) = artwork_by_artist.get(&artist.id) {
            artist.artwork_url = Some(artwork_url.clone());
        }
    }
}

fn build_artist_summaries_from_albums(
    tracks: &[LibraryTrack],
    albums: &[AlbumSummary],
) -> Vec<ArtistSummary> {
    let mut track_counts = HashMap::<String, usize>::new();
    for track in tracks {
        *track_counts
            .entry(stable_artist_id(&track.artist))
            .or_default() += 1;
    }

    let mut grouped = HashMap::<String, Vec<AlbumSummary>>::new();
    for album in albums {
        grouped
            .entry(stable_artist_id(&album.artist))
            .or_default()
            .push(album.clone());
    }

    let mut artists = grouped
        .into_iter()
        .filter_map(|(artist_id, mut artist_albums)| {
            artist_albums.sort_by(compare_albums);
            let first_album = artist_albums.first()?.clone();
            let artwork_track_id = artist_albums
                .iter()
                .find_map(|album| album.artwork_track_id.clone());
            let artwork_url = artist_albums
                .iter()
                .find_map(|album| album.artwork_url.clone());
            Some(ArtistSummary {
                id: artist_id.clone(),
                name: first_album.artist.clone(),
                album_count: artist_albums.len(),
                track_count: track_counts.get(&artist_id).copied().unwrap_or(0),
                artwork_track_id,
                artwork_url,
                first_album_id: first_album.id,
            })
        })
        .collect::<Vec<_>>();

    artists.sort_by(compare_artists);
    artists
}

#[allow(dead_code)]
fn build_artist_summaries(tracks: &[LibraryTrack]) -> Vec<ArtistSummary> {
    let albums = build_album_summaries(tracks);
    build_artist_summaries_from_albums(tracks, &albums)
}

fn compare_library_tracks(left: &LibraryTrack, right: &LibraryTrack) -> std::cmp::Ordering {
    (
        left.artist.as_str(),
        left.album.as_str(),
        numeric_sort_key(left.disc_number),
        numeric_sort_key(left.track_number),
        left.title.as_str(),
        left.relative_path.as_str(),
    )
        .cmp(&(
            right.artist.as_str(),
            right.album.as_str(),
            numeric_sort_key(right.disc_number),
            numeric_sort_key(right.track_number),
            right.title.as_str(),
            right.relative_path.as_str(),
        ))
}

fn compare_track_album_order(left: &LibraryTrack, right: &LibraryTrack) -> std::cmp::Ordering {
    (
        numeric_sort_key(left.disc_number),
        numeric_sort_key(left.track_number),
        left.title.as_str(),
        left.relative_path.as_str(),
    )
        .cmp(&(
            numeric_sort_key(right.disc_number),
            numeric_sort_key(right.track_number),
            right.title.as_str(),
            right.relative_path.as_str(),
        ))
}

fn compare_albums(left: &AlbumSummary, right: &AlbumSummary) -> std::cmp::Ordering {
    (left.artist.as_str(), left.title.as_str(), left.id.as_str()).cmp(&(
        right.artist.as_str(),
        right.title.as_str(),
        right.id.as_str(),
    ))
}

fn compare_artists(left: &ArtistSummary, right: &ArtistSummary) -> std::cmp::Ordering {
    (left.name.as_str(), left.id.as_str()).cmp(&(right.name.as_str(), right.id.as_str()))
}

fn numeric_sort_key(value: Option<u32>) -> (bool, u32) {
    (value.is_none(), value.unwrap_or(u32::MAX))
}

fn scan_dir(
    root: &Path,
    dir: &Path,
    artwork_cache_dir: &Path,
    tracks: &mut Vec<LibraryTrack>,
) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata()?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if should_skip_entry(&file_name) {
            continue;
        }

        if metadata.is_dir() {
            scan_dir(root, &path, artwork_cache_dir, tracks)?;
            continue;
        }

        if !is_supported_audio_file(&path) {
            continue;
        }

        let relative_components = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .components()
            .filter_map(component_to_string)
            .collect::<Vec<_>>();
        let relative_path = relative_components.join("/");
        let parsed_tags = read_lofty_track_tags(&path);
        let title = parsed_tags
            .title
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| inferred_title(&path));
        let (fallback_artist, fallback_album) = infer_artist_and_album(&relative_components);
        let artist = parsed_tags
            .artist
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(fallback_artist);
        let album = parsed_tags
            .album
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(fallback_album);
        let (fallback_disc_number, fallback_track_number) =
            infer_disc_and_track_numbers(&relative_components);
        let disc_number = parsed_tags.disc_number.or(fallback_disc_number);
        let track_number = parsed_tags.track_number.or(fallback_track_number);
        let mime_type = infer_mime_type(&path).to_string();
        let id = stable_track_id(&relative_path);
        let album_id = stable_album_id(&artist, &album);
        let artwork =
            resolve_track_artwork(root, &path, &relative_components, &id, artwork_cache_dir);

        tracks.push(LibraryTrack {
            id,
            album_id,
            title,
            artist,
            album,
            disc_number,
            track_number,
            duration_seconds: parsed_tags.duration_seconds,
            relative_path,
            path,
            mime_type,
            file_size: metadata.len(),
            artwork,
        });
    }

    Ok(())
}

#[derive(Debug)]
struct ArtworkCandidate {
    cache_key: String,
    source: String,
    mime_type: String,
    extension: &'static str,
    data: ArtworkData,
}

#[derive(Debug)]
enum ArtworkData {
    Bytes(Vec<u8>),
    File(PathBuf),
}

struct DownloadedArtwork {
    bytes: Vec<u8>,
    mime_type: String,
}

fn resolve_track_artwork(
    root: &Path,
    track_path: &Path,
    relative_components: &[String],
    track_id: &str,
    artwork_cache_dir: &Path,
) -> Option<TrackArtwork> {
    let candidate = read_embedded_artwork(track_path, track_id)
        .or_else(|| find_sidecar_artwork(root, track_path, relative_components));
    let Some(candidate) = candidate else {
        return None;
    };

    let destination =
        artwork_cache_dir.join(format!("{}.{}", candidate.cache_key, candidate.extension));
    if persist_artwork_candidate(&candidate, &destination).is_err() {
        return None;
    }

    Some(TrackArtwork {
        cache_key: format!("{}.{}", candidate.cache_key, candidate.extension),
        source: candidate.source,
        mime_type: candidate.mime_type,
    })
}

fn read_embedded_artwork(track_path: &Path, track_id: &str) -> Option<ArtworkCandidate> {
    let tagged_file = read_from_path(track_path).ok()?;
    let (picture, tag_label) = tagged_file
        .tags()
        .iter()
        .find_map(|tag| {
            tag.get_picture_type(PictureType::CoverFront)
                .map(|picture| (picture, format!("{:?}", tag.tag_type())))
        })
        .or_else(|| {
            tagged_file.tags().iter().find_map(|tag| {
                tag.pictures()
                    .first()
                    .map(|picture| (picture, format!("{:?}", tag.tag_type())))
            })
        })
        .or_else(|| {
            tagged_file
                .primary_tag()
                .or_else(|| tagged_file.first_tag())
                .and_then(|tag| {
                    tag.get_picture_type(PictureType::CoverFront)
                        .or_else(|| tag.pictures().first())
                        .map(|picture| (picture, format!("{:?}", tag.tag_type())))
                })
        })?;
    let mime_type = picture
        .mime_type()
        .map(|value| value.as_str().to_string())
        .or_else(|| infer_image_mime_from_bytes(picture.data()).map(ToString::to_string))?;
    let extension = image_extension_for_mime(&mime_type)?;

    Some(ArtworkCandidate {
        cache_key: stable_track_id(&format!("embedded:{track_id}")),
        source: format!("Embedded artwork ({:?}, {})", picture.pic_type(), tag_label),
        mime_type,
        extension,
        data: ArtworkData::Bytes(picture.data().to_vec()),
    })
}

fn find_sidecar_artwork(
    root: &Path,
    track_path: &Path,
    relative_components: &[String],
) -> Option<ArtworkCandidate> {
    let search_dirs = artwork_search_dirs(track_path, relative_components);
    for directory in search_dirs {
        let mut entries = fs::read_dir(&directory)
            .ok()?
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        entries.sort_by_key(|entry| entry.path());
        let mut best_match: Option<(usize, PathBuf, String)> = None;

        for entry in entries {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                continue;
            }

            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy().to_string();
            let Some(priority) = artwork_name_priority(&file_name) else {
                continue;
            };
            let should_replace = best_match
                .as_ref()
                .map(|(best_priority, best_path, _)| {
                    priority < *best_priority || (priority == *best_priority && path < *best_path)
                })
                .unwrap_or(true);
            if should_replace {
                best_match = Some((priority, path, file_name));
            }
        }

        if let Some((priority, path, _)) = best_match {
            let mime_type = infer_image_mime_from_path(&path)?;
            let extension = image_extension_for_mime(mime_type)?;
            let relative_source = path
                .strip_prefix(root)
                .ok()
                .map(|value| value.display().to_string())
                .unwrap_or_else(|| path.display().to_string());
            return Some(ArtworkCandidate {
                cache_key: stable_track_id(&format!("sidecar:{relative_source}:{priority}")),
                source: format!("Sidecar file: {relative_source}"),
                mime_type: mime_type.to_string(),
                extension,
                data: ArtworkData::File(path),
            });
        }
    }

    None
}

fn artwork_search_dirs(track_path: &Path, relative_components: &[String]) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(directory) = track_path.parent() {
        directories.push(directory.to_path_buf());
        if relative_components.len() > 2 {
            let parent_name = relative_components
                .get(relative_components.len().saturating_sub(2))
                .map(String::as_str)
                .unwrap_or_default();
            if looks_like_disc_folder(parent_name) {
                if let Some(parent) = directory.parent() {
                    if parent != directory {
                        directories.push(parent.to_path_buf());
                    }
                }
            }
        }
    }
    directories
}

fn artwork_name_priority(file_name: &str) -> Option<usize> {
    let normalized = file_name.trim().to_ascii_lowercase();
    let stem = Path::new(&normalized)
        .file_stem()
        .and_then(|value| value.to_str())?;

    let stem_priority = match stem {
        "cover" => 0,
        "folder" => 1,
        "front" => 2,
        "album" => 3,
        "artwork" => 4,
        _ => return None,
    };

    let extension_priority = match Path::new(&normalized)
        .extension()
        .and_then(|value| value.to_str())?
    {
        "jpg" => 0,
        "jpeg" => 1,
        "png" => 2,
        "webp" => 3,
        _ => return None,
    };

    Some((stem_priority * 10) + extension_priority)
}

fn infer_image_mime_from_path(path: &Path) -> Option<&'static str> {
    match file_extension(path).as_deref()? {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn infer_image_mime_from_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

fn image_extension_for_mime(mime_type: &str) -> Option<&'static str> {
    match mime_type {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

fn persist_artwork_candidate(candidate: &ArtworkCandidate, destination: &Path) -> io::Result<()> {
    match &candidate.data {
        ArtworkData::Bytes(bytes) => fs::write(destination, bytes),
        ArtworkData::File(source) => {
            fs::copy(source, destination)?;
            Ok(())
        }
    }
}

fn artwork_cache_path(config_path: &Path, cache_key: &str) -> PathBuf {
    config_path.join("artwork").join(cache_key)
}

fn musicbrainz_client() -> io::Result<Client> {
    Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(io::Error::other)
}

fn musicbrainz_user_agent() -> String {
    format!(
        "musicd/{} (self-hosted local music library app)",
        env!("CARGO_PKG_VERSION")
    )
}

fn search_musicbrainz_album_artwork(
    client: &Client,
    artist: &str,
    album: &str,
) -> io::Result<Vec<AlbumArtworkSearchCandidate>> {
    let query = format!(
        "release:\"{}\" AND artist:\"{}\"",
        lucene_escape_phrase(album),
        lucene_escape_phrase(artist),
    );
    let response = client
        .get("https://musicbrainz.org/ws/2/release")
        .query(&[("query", query.as_str()), ("fmt", "json"), ("limit", "8")])
        .header(USER_AGENT, musicbrainz_user_agent())
        .header(ACCEPT, "application/json")
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(io::Error::other)?;

    let search: MusicBrainzSearchResponse = response.json().map_err(io::Error::other)?;
    let mut seen_release_ids = HashSet::new();
    let mut candidates = Vec::new();

    for release in search.releases {
        if !seen_release_ids.insert(release.id.clone()) {
            continue;
        }
        if let Some(cover) = fetch_musicbrainz_cover_art_for_release(client, &release.id)? {
            candidates.push(AlbumArtworkSearchCandidate {
                release_id: release.id,
                release_group_id: release.release_group.map(|group| group.id),
                title: release.title,
                artist: artist_credit_name(&release.artist_credit),
                date: release.date,
                country: release.country,
                score: release.score.unwrap_or_default(),
                thumbnail_url: cover.thumbnail_url,
                image_url: cover.image_url,
                source: cover.source,
            });
        }
        if candidates.len() >= 3 {
            break;
        }
    }

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.date.cmp(&right.date))
            .then_with(|| left.country.cmp(&right.country))
    });
    Ok(candidates)
}

fn fetch_musicbrainz_cover_art_for_release(
    client: &Client,
    release_id: &str,
) -> io::Result<Option<AlbumArtworkSearchCandidate>> {
    let response = client
        .get(format!("https://coverartarchive.org/release/{release_id}/"))
        .header(USER_AGENT, musicbrainz_user_agent())
        .header(ACCEPT, "application/json")
        .send()
        .map_err(io::Error::other)?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    let response = response.error_for_status().map_err(io::Error::other)?;
    let archive: CoverArtArchiveResponse = response.json().map_err(io::Error::other)?;
    let Some(image) = archive
        .images
        .into_iter()
        .find(|image| image.front || image.approved)
    else {
        return Ok(None);
    };

    let thumbnail_url = image
        .thumbnails
        .get("250")
        .or_else(|| image.thumbnails.get("small"))
        .cloned()
        .unwrap_or_else(|| image.image.clone());

    Ok(Some(AlbumArtworkSearchCandidate {
        release_id: release_id.to_string(),
        release_group_id: None,
        title: String::new(),
        artist: String::new(),
        date: None,
        country: None,
        score: 0,
        thumbnail_url,
        image_url: image.image,
        source: archive
            .release
            .unwrap_or_else(|| format!("MusicBrainz release {release_id}")),
    }))
}

fn download_artwork_candidate(client: &Client, image_url: &str) -> io::Result<DownloadedArtwork> {
    let response = client
        .get(image_url)
        .header(USER_AGENT, musicbrainz_user_agent())
        .header(ACCEPT, "image/*")
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(io::Error::other)?;

    let mime_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| value.starts_with("image/"))
        .map(ToString::to_string)
        .or_else(|| infer_image_mime_from_url(image_url).map(ToString::to_string));
    let bytes = response.bytes().map_err(io::Error::other)?.to_vec();
    let mime_type = mime_type
        .or_else(|| infer_image_mime_from_bytes(&bytes).map(ToString::to_string))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown artwork MIME type"))?;

    Ok(DownloadedArtwork { bytes, mime_type })
}

fn infer_image_mime_from_url(url: &str) -> Option<&'static str> {
    let clean = url.split('?').next().unwrap_or(url);
    infer_image_mime_from_path(Path::new(clean))
}

fn artist_credit_name(credits: &[MusicBrainzArtistCredit]) -> String {
    credits
        .iter()
        .filter_map(|credit| credit.name.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn lucene_escape_phrase(value: &str) -> String {
    value.replace('\\', r#"\\"#).replace('"', r#"\""#)
}

fn component_to_string(component: Component<'_>) -> Option<String> {
    component.as_os_str().to_str().map(ToString::to_string)
}

fn should_skip_entry(file_name: &str) -> bool {
    file_name.starts_with('.') || file_name == "@eaDir"
}

fn is_supported_audio_file(path: &Path) -> bool {
    matches!(
        file_extension(path).as_deref(),
        Some("flac" | "wav" | "aiff" | "aif" | "alac" | "m4a" | "aac" | "mp3" | "ogg" | "dsf")
    )
}

fn file_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

fn stable_track_id(relative_path: &str) -> String {
    let mut hash = 1469598103934665603_u64;
    for byte in relative_path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{hash:016x}")
}

fn stable_album_id(artist: &str, album: &str) -> String {
    stable_track_id(&format!(
        "album:{}:{}",
        artist.trim().to_ascii_lowercase(),
        album.trim().to_ascii_lowercase()
    ))
}

fn stable_artist_id(artist: &str) -> String {
    stable_track_id(&format!("artist:{}", artist.trim().to_ascii_lowercase()))
}

fn normalized_renderer_name(location: &str, name: &str, model_name: Option<&str>) -> String {
    let trimmed_name = name.trim();
    let trimmed_model = model_name.map(str::trim).filter(|value| !value.is_empty());

    if renderer_name_looks_like_location(trimmed_name, location) {
        return trimmed_model
            .map(ToString::to_string)
            .or_else(|| renderer_location_host(location).map(ToString::to_string))
            .unwrap_or_else(|| location.to_string());
    }

    if trimmed_name.is_empty() {
        return trimmed_model
            .map(ToString::to_string)
            .or_else(|| renderer_location_host(location).map(ToString::to_string))
            .unwrap_or_else(|| location.to_string());
    }

    trimmed_name.to_string()
}

fn renderer_needs_refresh(renderer: &RendererRecord) -> bool {
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

fn renderer_is_viable(renderer: &RendererRecord) -> bool {
    match renderer_kind_for_location(&renderer.location) {
        RendererKind::Upnp => renderer.av_transport_control_url.is_some(),
        RendererKind::Sonos => true,
        RendererKind::AndroidLocal => true,
    }
}

fn renderer_actions_json(actions: &Option<Vec<String>>) -> Option<String> {
    actions
        .as_ref()
        .and_then(|actions| serde_json::to_string(actions).ok())
}

fn parse_renderer_actions_json(value: Option<String>) -> Option<Vec<String>> {
    value.and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
}

fn renderer_name_looks_like_location(name: &str, location: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed == location {
        return true;
    }
    if trimmed.parse::<IpAddr>().is_ok() {
        return true;
    }

    renderer_location_host(location)
        .map(|host| {
            trimmed.eq_ignore_ascii_case(host)
                || host.parse::<IpAddr>().is_ok() && trimmed.eq_ignore_ascii_case(host)
        })
        .unwrap_or(false)
}

fn renderer_location_host(location: &str) -> Option<&str> {
    let remainder = location
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(location);
    let authority = remainder.split('/').next()?.trim();
    if authority.is_empty() {
        return None;
    }
    let host = authority
        .strip_prefix('[')
        .and_then(|value| value.split_once(']').map(|(host, _)| host))
        .unwrap_or_else(|| authority.split(':').next().unwrap_or(authority))
        .trim();
    if host.is_empty() { None } else { Some(host) }
}

fn infer_disc_and_track_numbers(relative_components: &[String]) -> (Option<u32>, Option<u32>) {
    let directories = relative_components
        .iter()
        .take(relative_components.len().saturating_sub(1))
        .collect::<Vec<_>>();
    let disc_number = directories.iter().rev().find_map(|value| {
        if looks_like_disc_folder(value) {
            trailing_number(value)
        } else {
            None
        }
    });
    let track_number = relative_components
        .last()
        .and_then(|value| Path::new(value).file_stem().and_then(|stem| stem.to_str()))
        .and_then(leading_number);

    (disc_number, track_number)
}

fn leading_number(value: &str) -> Option<u32> {
    let digits = value
        .chars()
        .skip_while(|character| character.is_whitespace())
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok()
    }
}

fn trailing_number(value: &str) -> Option<u32> {
    let digits = value
        .chars()
        .rev()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok()
    }
}

fn now_unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> io::Result<()> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut statement = connection.prepare(&pragma).map_err(db_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(db_error)?;

    for row in rows {
        if row.map_err(db_error)? == column {
            return Ok(());
        }
    }

    let alter = format!("ALTER TABLE {table} ADD COLUMN {column} {definition}");
    connection.execute(&alter, []).map_err(db_error)?;
    Ok(())
}

fn table_is_empty(connection: &Connection, table: &str) -> io::Result<bool> {
    let query = format!("SELECT 1 FROM {table} LIMIT 1");
    connection
        .query_row(&query, [], |_| Ok(()))
        .optional()
        .map(|row| row.is_none())
        .map_err(db_error)
}

fn db_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(format!("sqlite error: {error}"))
}

fn load_tracks_from_connection(connection: &Connection) -> io::Result<Vec<LibraryTrack>> {
    let mut statement = connection
        .prepare(
            "SELECT id, album_id, title, artist, album, disc_number, track_number,
                    duration_seconds, relative_path, path, mime_type, file_size,
                    artwork_cache_key, artwork_source, artwork_mime_type
             FROM tracks
             ORDER BY artist, album, COALESCE(disc_number, 0), COALESCE(track_number, 0), title, relative_path",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            let artist: String = row.get(3)?;
            let album: String = row.get(4)?;
            Ok(LibraryTrack {
                id: row.get(0)?,
                album_id: row
                    .get::<_, Option<String>>(1)?
                    .unwrap_or_else(|| stable_album_id(&artist, &album)),
                title: row.get(2)?,
                artist,
                album,
                disc_number: row.get(5)?,
                track_number: row.get(6)?,
                duration_seconds: row.get(7)?,
                relative_path: row.get(8)?,
                path: PathBuf::from(row.get::<_, String>(9)?),
                mime_type: row.get(10)?,
                file_size: row.get(11)?,
                artwork: match (
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                ) {
                    (Some(cache_key), Some(source), Some(mime_type)) => Some(TrackArtwork {
                        cache_key,
                        source,
                        mime_type,
                    }),
                    _ => None,
                },
            })
        })
        .map_err(db_error)?;

    let mut tracks = Vec::new();
    for row in rows {
        tracks.push(row.map_err(db_error)?);
    }
    Ok(tracks)
}

fn load_albums_from_connection(connection: &Connection) -> io::Result<Vec<AlbumSummary>> {
    let mut statement = connection
        .prepare(
            "SELECT id, artist_id, title, artist_name, track_count, artwork_track_id,
                    artwork_cache_key, artwork_source, artwork_mime_type, first_track_id
             FROM albums
             ORDER BY artist_name ASC, title ASC, id ASC",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            let artwork_track_id = row.get::<_, Option<String>>(5)?;
            let artwork = match (
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ) {
                (Some(cache_key), Some(source), Some(mime_type)) => Some(TrackArtwork {
                    cache_key,
                    source,
                    mime_type,
                }),
                _ => None,
            };
            Ok(AlbumSummary {
                id: row.get(0)?,
                artist_id: row.get(1)?,
                title: row.get(2)?,
                artist: row.get(3)?,
                track_count: row.get(4)?,
                artwork_track_id: artwork_track_id.clone(),
                artwork: artwork.clone(),
                artwork_url: if artwork.is_some() {
                    Some(format!("/artwork/album/{}", row.get::<_, String>(0)?))
                } else {
                    None
                },
                first_track_id: row.get(9)?,
            })
        })
        .map_err(db_error)?;

    let mut albums = Vec::new();
    for row in rows {
        albums.push(row.map_err(db_error)?);
    }
    Ok(albums)
}

fn rebuild_normalized_library_tables(connection: &Connection) -> io::Result<()> {
    let tracks = load_tracks_from_connection(connection)?;
    let albums = build_album_summaries(&tracks);
    let artists = build_artist_summaries_from_albums(&tracks, &albums);
    let transaction = connection.unchecked_transaction().map_err(db_error)?;
    transaction
        .execute("DELETE FROM albums", [])
        .map_err(db_error)?;
    transaction
        .execute("DELETE FROM artists", [])
        .map_err(db_error)?;

    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO albums
                 (id, artist_id, title, artist_name, track_count, artwork_track_id,
                  artwork_cache_key, artwork_source, artwork_mime_type, first_track_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .map_err(db_error)?;
        for album in &albums {
            let artwork_cache_key = album
                .artwork
                .as_ref()
                .map(|artwork| artwork.cache_key.clone());
            let artwork_source = album.artwork.as_ref().map(|artwork| artwork.source.clone());
            let artwork_mime_type = album
                .artwork
                .as_ref()
                .map(|artwork| artwork.mime_type.clone());
            statement
                .execute(params![
                    album.id,
                    album.artist_id,
                    album.title,
                    album.artist,
                    album.track_count,
                    album.artwork_track_id,
                    artwork_cache_key,
                    artwork_source,
                    artwork_mime_type,
                    album.first_track_id,
                ])
                .map_err(db_error)?;
        }
    }

    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO artists
                 (id, name, album_count, track_count, artwork_track_id, first_album_id)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .map_err(db_error)?;
        for artist in &artists {
            statement
                .execute(params![
                    artist.id,
                    artist.name,
                    artist.album_count,
                    artist.track_count,
                    artist.artwork_track_id,
                    artist.first_album_id,
                ])
                .map_err(db_error)?;
        }
    }

    transaction.commit().map_err(db_error)?;
    Ok(())
}

fn inspect_embedded_metadata(path: &Path) -> io::Result<EmbeddedMetadata> {
    if let Ok(metadata) = inspect_with_lofty(path) {
        return Ok(metadata);
    }

    match file_extension(path).as_deref() {
        Some("flac") => inspect_flac_metadata(path),
        Some("mp3") => inspect_mp3_metadata(path),
        Some("m4a" | "alac" | "aac") => Ok(EmbeddedMetadata {
            format_name: "MP4-family file".to_string(),
            fields: Vec::new(),
            notes: vec!["Embedded tag parsing for this format is not implemented yet.".to_string()],
        }),
        Some("ogg") => Ok(EmbeddedMetadata {
            format_name: "Ogg container".to_string(),
            fields: Vec::new(),
            notes: vec!["Embedded tag parsing for Ogg/Vorbis is not implemented yet.".to_string()],
        }),
        Some("wav" | "aiff" | "aif" | "dsf") => Ok(EmbeddedMetadata {
            format_name: "Audio file".to_string(),
            fields: Vec::new(),
            notes: vec![
                "No embedded metadata parser is implemented for this format yet.".to_string(),
            ],
        }),
        _ => Ok(EmbeddedMetadata {
            format_name: "Unknown".to_string(),
            fields: Vec::new(),
            notes: vec!["Unknown file type.".to_string()],
        }),
    }
}

fn read_lofty_track_tags(path: &Path) -> ParsedTrackTags {
    let tagged_file = match read_from_path(path) {
        Ok(tagged_file) => tagged_file,
        Err(_) => return ParsedTrackTags::default(),
    };

    let Some(tag) = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
    else {
        return ParsedTrackTags::default();
    };

    ParsedTrackTags {
        title: tag.title().map(|value| value.into_owned()),
        artist: tag.artist().map(|value| value.into_owned()),
        album: tag.album().map(|value| value.into_owned()),
        disc_number: tag.disk(),
        track_number: tag.track(),
        duration_seconds: {
            let seconds = tagged_file.properties().duration().as_secs();
            if seconds == 0 { None } else { Some(seconds) }
        },
    }
}

fn inspect_with_lofty(path: &Path) -> io::Result<EmbeddedMetadata> {
    let tagged_file = read_from_path(path).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("lofty failed to read tags: {error}"),
        )
    })?;

    let mut fields = Vec::new();
    let mut notes = Vec::new();
    let tag_types = tagged_file
        .tags()
        .iter()
        .map(|tag| format!("{:?}", tag.tag_type()))
        .collect::<Vec<_>>();
    notes.push(format!("Lofty file type: {:?}", tagged_file.file_type()));
    if tag_types.is_empty() {
        notes.push("Lofty did not find any readable tags in this file.".to_string());
    } else {
        notes.push(format!("Readable tag types: {}", tag_types.join(", ")));
    }

    if let Some(tag) = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
    {
        fields.push(("TAG_TYPE".to_string(), format!("{:?}", tag.tag_type())));
        push_optional_field(
            &mut fields,
            "TITLE",
            tag.title().map(|value| value.into_owned()),
        );
        push_optional_field(
            &mut fields,
            "ARTIST",
            tag.artist().map(|value| value.into_owned()),
        );
        push_optional_field(
            &mut fields,
            "ALBUM",
            tag.album().map(|value| value.into_owned()),
        );
        push_optional_field(
            &mut fields,
            "GENRE",
            tag.genre().map(|value| value.into_owned()),
        );
        push_optional_field(
            &mut fields,
            "TRACKNUMBER",
            tag.track().map(|value| value.to_string()),
        );
        push_optional_field(
            &mut fields,
            "TRACKTOTAL",
            tag.track_total().map(|value| value.to_string()),
        );
        push_optional_field(
            &mut fields,
            "DISCNUMBER",
            tag.disk().map(|value| value.to_string()),
        );
        push_optional_field(
            &mut fields,
            "DISCTOTAL",
            tag.disk_total().map(|value| value.to_string()),
        );
        push_optional_field(
            &mut fields,
            "COMMENT",
            tag.comment().map(|value| value.into_owned()),
        );
    }

    let properties = tagged_file.properties();
    push_optional_field(
        &mut fields,
        "DURATION_SECONDS",
        Some(properties.duration().as_secs().to_string()),
    );
    push_optional_field(
        &mut fields,
        "CHANNELS",
        properties.channels().map(|value| value.to_string()),
    );
    push_optional_field(
        &mut fields,
        "SAMPLE_RATE",
        properties.sample_rate().map(|value| value.to_string()),
    );
    push_optional_field(
        &mut fields,
        "AUDIO_BITRATE_KBPS",
        properties.audio_bitrate().map(|value| value.to_string()),
    );
    push_optional_field(
        &mut fields,
        "OVERALL_BITRATE_KBPS",
        properties.overall_bitrate().map(|value| value.to_string()),
    );
    push_optional_field(
        &mut fields,
        "BIT_DEPTH",
        properties.bit_depth().map(|value| value.to_string()),
    );

    Ok(EmbeddedMetadata {
        format_name: "Lofty parsed metadata".to_string(),
        fields,
        notes,
    })
}

fn push_optional_field(fields: &mut Vec<(String, String)>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            fields.push((key.to_string(), trimmed.to_string()));
        }
    }
}

fn inspect_flac_metadata(path: &Path) -> io::Result<EmbeddedMetadata> {
    let mut file = File::open(path)?;
    let mut signature = [0_u8; 4];
    file.read_exact(&mut signature)?;
    if &signature != b"fLaC" {
        return Ok(EmbeddedMetadata {
            format_name: "FLAC".to_string(),
            fields: Vec::new(),
            notes: vec!["File does not begin with the FLAC signature.".to_string()],
        });
    }

    let mut notes = Vec::new();
    let mut fields = Vec::new();
    loop {
        let mut header = [0_u8; 4];
        if file.read_exact(&mut header).is_err() {
            break;
        }
        let is_last = header[0] & 0x80 != 0;
        let block_type = header[0] & 0x7f;
        let block_len =
            ((header[1] as usize) << 16) | ((header[2] as usize) << 8) | header[3] as usize;
        let mut block = vec![0_u8; block_len];
        file.read_exact(&mut block)?;

        if block_type == 4 {
            let (parsed_fields, parsed_notes) = parse_vorbis_comment_block(&block);
            fields.extend(parsed_fields);
            notes.extend(parsed_notes);
        }

        if is_last {
            break;
        }
    }

    if fields.is_empty() {
        notes.push("No Vorbis comment fields were parsed from this FLAC file.".to_string());
    }

    Ok(EmbeddedMetadata {
        format_name: "FLAC Vorbis comments".to_string(),
        fields,
        notes,
    })
}

fn parse_vorbis_comment_block(block: &[u8]) -> (Vec<(String, String)>, Vec<String>) {
    let mut offset = 0;
    let mut notes = Vec::new();
    let mut fields = Vec::new();

    let Some(vendor_len) = read_le_u32(block, &mut offset) else {
        return (
            fields,
            vec!["Vorbis comment block ended before vendor length.".to_string()],
        );
    };
    if offset + vendor_len as usize > block.len() {
        return (
            fields,
            vec!["Vorbis comment vendor string length was invalid.".to_string()],
        );
    }
    let vendor = String::from_utf8_lossy(&block[offset..offset + vendor_len as usize]).to_string();
    offset += vendor_len as usize;
    fields.push(("VENDOR".to_string(), vendor));

    let Some(comment_count) = read_le_u32(block, &mut offset) else {
        notes.push("Vorbis comment block ended before the comment count.".to_string());
        return (fields, notes);
    };

    for _ in 0..comment_count {
        let Some(comment_len) = read_le_u32(block, &mut offset) else {
            notes.push("Vorbis comment block ended before a comment length.".to_string());
            break;
        };
        let comment_len = comment_len as usize;
        if offset + comment_len > block.len() {
            notes.push("Vorbis comment block contained an invalid comment length.".to_string());
            break;
        }
        let comment = String::from_utf8_lossy(&block[offset..offset + comment_len]).to_string();
        offset += comment_len;
        if let Some((key, value)) = comment.split_once('=') {
            fields.push((key.to_ascii_uppercase(), value.to_string()));
        } else {
            notes.push(format!("Unstructured Vorbis comment: {comment}"));
        }
    }

    (fields, notes)
}

fn inspect_mp3_metadata(path: &Path) -> io::Result<EmbeddedMetadata> {
    let mut file = File::open(path)?;
    let mut notes = Vec::new();
    let mut fields = Vec::new();

    let mut header = [0_u8; 10];
    let read = file.read(&mut header)?;
    if read >= 10 && &header[..3] == b"ID3" {
        let size = decode_synchsafe_u32(&header[6..10]);
        notes.push(format!(
            "ID3v2.{}.{} tag detected at file start ({} bytes before audio frames).",
            header[3], header[4], size
        ));
    } else {
        notes.push("No ID3v2 header detected at the start of the file.".to_string());
    }

    let file_len = file.metadata()?.len();
    if file_len >= 128 {
        file.seek(SeekFrom::End(-128))?;
        let mut trailer = [0_u8; 128];
        file.read_exact(&mut trailer)?;
        if &trailer[..3] == b"TAG" {
            fields.push(("TITLE".to_string(), decode_id3v1_text(&trailer[3..33])));
            fields.push(("ARTIST".to_string(), decode_id3v1_text(&trailer[33..63])));
            fields.push(("ALBUM".to_string(), decode_id3v1_text(&trailer[63..93])));
            fields.push(("YEAR".to_string(), decode_id3v1_text(&trailer[93..97])));
            let comment = decode_id3v1_text(&trailer[97..127]);
            if !comment.is_empty() {
                fields.push(("COMMENT".to_string(), comment));
            }
            if trailer[125] == 0 && trailer[126] != 0 {
                fields.push(("TRACKNUMBER".to_string(), trailer[126].to_string()));
            }
        } else {
            notes.push("No ID3v1 trailer detected at the end of the file.".to_string());
        }
    }

    Ok(EmbeddedMetadata {
        format_name: "MP3 tags".to_string(),
        fields,
        notes,
    })
}

fn read_le_u32(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    if *offset + 4 > bytes.len() {
        return None;
    }
    let value = u32::from_le_bytes([
        bytes[*offset],
        bytes[*offset + 1],
        bytes[*offset + 2],
        bytes[*offset + 3],
    ]);
    *offset += 4;
    Some(value)
}

fn decode_synchsafe_u32(bytes: &[u8]) -> u32 {
    ((bytes[0] as u32) << 21)
        | ((bytes[1] as u32) << 14)
        | ((bytes[2] as u32) << 7)
        | (bytes[3] as u32)
}

fn decode_id3v1_text(bytes: &[u8]) -> String {
    let trimmed = bytes
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&trimmed).trim().to_string()
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

fn infer_artist_and_album(relative_components: &[String]) -> (String, String) {
    let directories = relative_components
        .iter()
        .take(relative_components.len().saturating_sub(1))
        .cloned()
        .collect::<Vec<_>>();

    match directories.as_slice() {
        [] => ("Unknown Artist".to_string(), "Unknown Album".to_string()),
        [album] => ("Unknown Artist".to_string(), cleanup_track_label(album)),
        [artist, album] => (cleanup_track_label(artist), cleanup_track_label(album)),
        _ => {
            let artist = directories
                .first()
                .map(|value| cleanup_track_label(value))
                .unwrap_or_else(|| "Unknown Artist".to_string());
            let album_index = if directories
                .last()
                .map(|value| looks_like_disc_folder(value))
                .unwrap_or(false)
            {
                directories.len().saturating_sub(2)
            } else {
                directories.len().saturating_sub(1)
            };
            let album = directories
                .get(album_index)
                .map(|value| cleanup_track_label(value))
                .unwrap_or_else(|| "Unknown Album".to_string());
            (artist, album)
        }
    }
}

fn looks_like_disc_folder(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase().replace('_', " ");
    normalized.starts_with("disc ")
        || normalized.starts_with("disk ")
        || normalized.starts_with("cd ")
        || normalized == "disc1"
        || normalized == "disc 1"
        || normalized == "cd1"
        || normalized == "cd 1"
}

fn cleanup_track_label(value: &str) -> String {
    let trimmed = value.trim();
    let trimmed =
        trimmed.trim_start_matches(|character: char| character == '_' || character == '-');
    let without_prefix = if let Some((prefix, rest)) = trimmed.split_once(' ') {
        if prefix
            .chars()
            .all(|character| character.is_ascii_digit() || character == '.')
        {
            rest.trim_start_matches(['-', '_', '.']).trim()
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    without_prefix.replace('_', " ")
}

fn infer_mime_type(path: &Path) -> &'static str {
    match file_extension(path).as_deref().unwrap_or("") {
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "aiff" | "aif" => "audio/aiff",
        "alac" | "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "dsf" => "audio/dsd",
        _ => "application/octet-stream",
    }
}

fn format_track_position(disc_number: Option<u32>, track_number: Option<u32>) -> String {
    match (disc_number, track_number) {
        (Some(disc), Some(track)) => format!("Disc {disc} • Track {track}"),
        (None, Some(track)) => format!("Track {track}"),
        (Some(disc), None) => format!("Disc {disc}"),
        (None, None) => "Unknown position".to_string(),
    }
}

fn option_u32_json(value: Option<u32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn option_u64_json(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn option_i64_json(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn option_bool_json(value: Option<bool>) -> String {
    value.map(bool_json).unwrap_or_else(|| "null".to_string())
}

fn bool_json(value: bool) -> String {
    if value {
        "true".to_string()
    } else {
        "false".to_string()
    }
}

fn string_list_json(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!(r#""{}""#, json_escape(value)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn option_json_fragment(value: Option<&str>) -> String {
    value.unwrap_or("null").to_string()
}

fn option_string_json(value: Option<&str>) -> String {
    value
        .map(|value| format!(r#""{}""#, json_escape(value)))
        .unwrap_or_else(|| "null".to_string())
}

fn format_duration_seconds(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn queue_status_for_transport(transport_state: &str) -> &'static str {
    match transport_state {
        "PLAYING" | "TRANSITIONING" => "playing",
        "PAUSED_PLAYBACK" => "paused",
        "STOPPED" | "NO_MEDIA_PRESENT" => "stopped",
        "READY" => "ready",
        "COMPLETED" => "completed",
        "ERROR" => "error",
        _ => "ready",
    }
}

fn next_queue_entry_after(queue: &PlaybackQueue, current_entry_id: i64) -> Option<&QueueEntry> {
    let current_position = queue
        .entries
        .iter()
        .find(|entry| entry.id == current_entry_id)
        .map(|entry| entry.position)?;

    queue
        .entries
        .iter()
        .find(|entry| entry.position > current_position)
}

fn previous_queue_entry_before(
    queue: &PlaybackQueue,
    current_entry_id: i64,
) -> Option<&QueueEntry> {
    let current_position = queue
        .entries
        .iter()
        .find(|entry| entry.id == current_entry_id)
        .map(|entry| entry.position)?;

    queue
        .entries
        .iter()
        .rev()
        .find(|entry| entry.position < current_position)
}

fn should_adopt_preloaded_next_entry(
    queue: &PlaybackQueue,
    snapshot: &TransportSnapshot,
    expected_next_track_uri: Option<&str>,
) -> bool {
    let Some(current_entry_id) = queue.current_entry_id else {
        return false;
    };
    if next_queue_entry_after(queue, current_entry_id).is_none() {
        return false;
    }
    let Some(track_uri) = snapshot.position_info.track_uri.as_deref() else {
        return false;
    };
    expected_next_track_uri.is_some_and(|expected_uri| track_uri == expected_uri)
}

fn should_auto_advance(
    queue: &PlaybackQueue,
    session: Option<&PlaybackSession>,
    snapshot: &TransportSnapshot,
    state: &ServiceState,
) -> bool {
    if !matches!(
        snapshot.transport_info.transport_state.as_str(),
        "STOPPED" | "NO_MEDIA_PRESENT"
    ) {
        return false;
    }

    let session = match session {
        Some(session) => session,
        None => return false,
    };
    if session.transport_state == "PAUSED_PLAYBACK" {
        return false;
    }
    let current_entry_id = match queue.current_entry_id {
        Some(current_entry_id) => current_entry_id,
        None => return false,
    };
    let current_entry = match queue
        .entries
        .iter()
        .find(|entry| entry.id == current_entry_id)
    {
        Some(current_entry) => current_entry,
        None => return false,
    };
    if current_entry.entry_status != "playing" {
        return false;
    }

    let track = match state.find_track(&current_entry.track_id) {
        Some(track) => track,
        None => return false,
    };
    let expected_duration = snapshot
        .position_info
        .track_duration_seconds
        .or(track.duration_seconds)
        .or(session.duration_seconds);
    let observed_position = session
        .position_seconds
        .into_iter()
        .chain(snapshot.position_info.rel_time_seconds)
        .max()
        .unwrap_or(0);

    expected_duration
        .map(|duration| observed_position.saturating_add(2) >= duration)
        .unwrap_or(false)
}

fn parse_range_header(value: &str, total_len: u64) -> Option<(u64, u64)> {
    let bytes = value.strip_prefix("bytes=")?;
    let (start_text, end_text) = bytes.split_once('-')?;

    if start_text.is_empty() {
        let suffix_len = end_text.parse::<u64>().ok()?;
        if suffix_len == 0 {
            return None;
        }
        let start = total_len.saturating_sub(suffix_len);
        return Some((start, total_len.saturating_sub(1)));
    }

    let start = start_text.parse::<u64>().ok()?;
    let end = if end_text.is_empty() {
        total_len.saturating_sub(1)
    } else {
        end_text.parse::<u64>().ok()?
    };

    if start > end || end >= total_len {
        return None;
    }

    Some((start, end))
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::{
        Database, LibraryTrack, OnceLock, PlaybackQueue, PlaybackSession, QueueEntry,
        QueueMutationEntry, RendererBackends, RendererKind, ServiceState, artwork_name_priority,
        build_artist_summaries, cleanup_track_label, compare_track_album_order, decode_id3v1_text,
        infer_artist_and_album, infer_disc_and_track_numbers, infer_image_mime_from_bytes,
        next_queue_entry_after, parse_query_string, parse_range_header, parse_request_form,
        parse_vorbis_comment_block, previous_queue_entry_before, queue_status_for_transport,
        renderer_is_viable, renderer_kind_for_location, should_adopt_preloaded_next_entry,
        should_auto_advance, should_skip_entry, stable_album_id, stable_artist_id,
        stable_track_id,
    };
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

        let library = super::Library {
            scan_root: PathBuf::from("/music"),
            tracks: vec![first.clone(), second.clone(), third.clone()],
        };

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
            library: std::sync::Mutex::new(super::Library {
                scan_root: PathBuf::from("/music"),
                tracks,
            }),
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
