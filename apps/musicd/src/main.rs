use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::read_from_path;
use lofty::tag::Accessor;
use musicd_core::AppConfig;
use musicd_upnp::{StreamResource, discover_renderers, inspect_renderer, play_stream};
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, Clone, PartialEq, Eq)]
struct LibraryTrack {
    id: String,
    title: String,
    artist: String,
    album: String,
    relative_path: String,
    path: PathBuf,
    mime_type: String,
    file_size: u64,
    artwork: Option<TrackArtwork>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackArtwork {
    cache_key: String,
    source: String,
    mime_type: String,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RendererRecord {
    location: String,
    name: String,
    manufacturer: Option<String>,
    model_name: Option<String>,
    av_transport_control_url: Option<String>,
    last_seen_unix: i64,
}

#[derive(Debug, Clone)]
struct Database {
    path: PathBuf,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpRequest {
    method: String,
    target: String,
    path: String,
    query: HashMap<String, String>,
    range_header: Option<String>,
}

#[derive(Debug, Clone)]
enum ServerMode {
    SingleFile(Arc<PathBuf>),
    Service(Arc<ServiceState>),
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
    let track_count = state.track_count();

    println!("musicd service");
    println!("Library path: {}", config.library_path.display());
    println!("Config path: {}", config.config_path.display());
    println!("Bind address: {}", config.bind_address);
    println!("HTTP base URL: {}", config.base_url);
    println!("Indexed tracks: {track_count}");
    if let Some(renderer) = &config.default_renderer_location {
        println!("Default renderer: {renderer}");
    }
    println!(
        "Open {}/ in a browser to browse and play music.",
        config.base_url
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
                        eprintln!("request failed: {error}");
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

    match mode {
        ServerMode::SingleFile(path) => handle_single_file_request(&mut writer, &request, path),
        ServerMode::Service(state) => handle_service_request(&mut writer, &request, state),
    }
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
        ("GET", "/play") => handle_play_request(writer, request, &state),
        ("GET", "/rescan") => handle_rescan_request(writer, request, &state),
        ("HEAD", "/play") | ("HEAD", "/rescan") => respond_method_not_allowed(writer),
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

    let stream_url = format!(
        "{}/stream/track/{}",
        state.config.base_url.trim_end_matches('/'),
        track.id
    );
    let resource = StreamResource {
        stream_url,
        mime_type: track.mime_type.clone(),
        title: track.title.clone(),
    };

    match play_stream(&renderer_location, &resource) {
        Ok(renderer) => {
            let _ = state.remember_renderer_details(
                &renderer.location,
                &renderer.friendly_name,
                renderer.manufacturer.as_deref(),
                renderer.model_name.as_deref(),
                Some(&renderer.av_transport_control_url),
            );
            redirect_home(
                writer,
                Some(&renderer_location),
                Some(&format!(
                    "Now playing '{}' on {}.",
                    track.title, renderer.friendly_name
                )),
                None,
            )
        }
        Err(error) => redirect_home(
            writer,
            Some(&renderer_location),
            None,
            Some(&format!("Playback failed: {error}")),
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
            }
        }
    }

    Ok(Some(HttpRequest {
        method,
        target,
        path,
        query,
        range_header,
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
        "/".to_string()
    } else {
        format!("/?{}", params.join("&"))
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
    let library_path = state.config.library_path.display().to_string();
    let renderer_location = state
        .preferred_renderer_location(request.query.get("renderer_location").map(String::as_str));
    let known_renderers = state.renderer_snapshot();
    let message = request.query.get("message").cloned().unwrap_or_default();
    let error = request.query.get("error").cloned().unwrap_or_default();

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
            "<tr data-search=\"{}\"><td><input type=\"radio\" form=\"playback_form\" name=\"track_id\" value=\"{}\"></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td><a href=\"/stream/track/{}\" target=\"_blank\" rel=\"noreferrer\">Preview</a> <span class=\"muted-sep\">|</span> <a href=\"/track/{}\" target=\"_blank\" rel=\"noreferrer\">Inspect</a></td></tr>",
            html_escape(&search_text),
            html_escape(&track.id),
            cover_html,
            html_escape(&track.title),
            html_escape(&track.artist),
            html_escape(&track.album),
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
  <title>musicd</title>
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
      label {{
        min-width: auto;
        width: 100%;
      }}
      input[type="text"], select {{
        min-width: 0;
        width: 100%;
      }}
    }}
  </style>
</head>
<body>
  <main>
    <header>
      <h1>musicd</h1>
      <p class="meta">Library path: {}</p>
      <p class="meta">Indexed tracks: {}</p>
      <p class="meta">Stream base URL: {}</p>
    </header>
    {}{}
    <section class="controls">
      <form id="playback_form" class="control-row" action="/play" method="get">
        <label for="renderer_location">Renderer LOCATION</label>
        <input id="renderer_location" name="renderer_location" type="text" value="{}" placeholder="http://192.168.1.55:49152/description.xml" oninput="syncRendererFields(this.value)">
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
        <input id="rescan_renderer_location" type="hidden" name="renderer_location" value="{}">
        <label for="track_filter">Search Tracks</label>
        <input id="track_filter" type="text" placeholder="Filter by title, artist, album, or path" oninput="filterTracks()">
        <button type="submit" class="secondary">Rescan Library</button>
      </form>
    </section>
    {}
    <section class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Play</th>
            <th>Cover</th>
            <th>Title</th>
            <th>Artist</th>
            <th>Album</th>
            <th>Preview</th>
          </tr>
        </thead>
        <tbody id="track_table">
          {}
        </tbody>
      </table>
    </section>
  </main>
  <script>
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
      }}
    }}

    function syncRendererFields(value) {{
      const hidden = document.getElementById('rescan_renderer_location');
      if (hidden) {{
        hidden.value = value;
      }}
    }}

    function filterTracks() {{
      const needle = document.getElementById('track_filter').value.trim().toLowerCase();
      const rows = document.querySelectorAll('#track_table tr');
      for (const row of rows) {{
        row.style.display = !needle || row.dataset.search.includes(needle) ? '' : 'none';
      }}
    }}
  </script>
</body>
</html>"#,
        html_escape(&library_path),
        tracks.len(),
        html_escape(&state.config.base_url),
        message_html,
        error_html,
        html_escape(&renderer_location),
        renderer_options,
        html_escape(&renderer_location),
        empty_state,
        rows,
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
    let artwork_url = track
        .artwork
        .as_ref()
        .map(|_| format!("/artwork/track/{}", track.id));

    let inferred_rows = [
        ("Track ID", track.id.clone()),
        ("Title", track.title.clone()),
        ("Artist", track.artist.clone()),
        ("Album", track.album.clone()),
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
  </style>
</head>
<body>
  <main>
    <header>
      <h1>{}</h1>
      <p>{} • {}</p>
      <div class="actions">
        <a href="/">Back to Library</a>
        <a class="secondary" href="/stream/track/{}" target="_blank" rel="noreferrer">Preview Stream</a>
        <a href="{}">Play On Renderer</a>
      </div>
    </header>
    <section>
      <h2>Inferred Library Metadata</h2>
      <table>{}</table>
    </section>
    {}
    <section>
      <h2>Embedded File Metadata</h2>
      <p>Parser: {}</p>
      <table>{}</table>
      {}
    </section>
  </main>
</body>
</html>"#,
        html_escape(&track.title),
        html_escape(&track.title),
        html_escape(&track.artist),
        html_escape(&track.album),
        html_escape(&track.id),
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
        r#"{{"id":"{}","title":"{}","artist":"{}","album":"{}","relative_path":"{}","absolute_path":"{}","mime_type":"{}","size":{},"artwork":{},"embedded_metadata":{{"parser":"{}","fields":[{}],"notes":[{}]}}}}"#,
        json_escape(&track.id),
        json_escape(&track.title),
        json_escape(&track.artist),
        json_escape(&track.album),
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
    let entries = tracks
        .into_iter()
        .map(|track| {
            let artwork_url = track
                .artwork
                .as_ref()
                .map(|_| format!("/artwork/track/{}", track.id))
                .unwrap_or_default();
            format!(
                r#"{{"id":"{}","title":"{}","artist":"{}","album":"{}","path":"{}","mime_type":"{}","size":{},"artwork_url":"{}"}}"#,
                json_escape(&track.id),
                json_escape(&track.title),
                json_escape(&track.artist),
                json_escape(&track.album),
                json_escape(&track.relative_path),
                json_escape(&track.mime_type),
                track.file_size,
                json_escape(&artwork_url),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{entries}]")
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
        .map(|renderer| {
            match inspect_renderer(&renderer.location) {
                Ok(details) => {
                    let _ = state.remember_renderer_details(
                        &details.location,
                        &details.friendly_name,
                        details.manufacturer.as_deref(),
                        details.model_name.as_deref(),
                        Some(&details.av_transport_control_url),
                    );
                    format!(
                        r#"{{"location":"{}","name":"{}","manufacturer":"{}","model":"{}","av_transport":"{}"}}"#,
                        json_escape(&details.location),
                        json_escape(&details.friendly_name),
                        json_escape(details.manufacturer.as_deref().unwrap_or("")),
                        json_escape(details.model_name.as_deref().unwrap_or("")),
                        json_escape(&details.av_transport_control_url),
                    )
                }
                Err(error) => {
                    let name = renderer.server.as_deref().unwrap_or("Unknown renderer");
                    let _ = state.remember_renderer_details(
                        &renderer.location,
                        name,
                        None,
                        None,
                        None,
                    );
                    format!(
                        r#"{{"location":"{}","name":"{}","error":"{}"}}"#,
                        json_escape(&renderer.location),
                        json_escape(name),
                        json_escape(&error.to_string()),
                    )
                }
            }
        })
        .collect::<Vec<_>>()
        .join(",");

    format!("[{entries}]")
}

impl Database {
    fn open(config_path: &Path) -> io::Result<Self> {
        fs::create_dir_all(config_path)?;
        let database = Self {
            path: config_path.join("musicd.db"),
        };
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
                    title TEXT NOT NULL,
                    artist TEXT NOT NULL,
                    album TEXT NOT NULL,
                    relative_path TEXT NOT NULL,
                    path TEXT NOT NULL,
                    mime_type TEXT NOT NULL,
                    file_size INTEGER NOT NULL,
                    artwork_cache_key TEXT,
                    artwork_source TEXT,
                    artwork_mime_type TEXT
                );

                CREATE TABLE IF NOT EXISTS renderers (
                    location TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    manufacturer TEXT,
                    model_name TEXT,
                    av_transport_control_url TEXT,
                    last_seen_unix INTEGER NOT NULL DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS app_state (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                "#,
            )
            .map_err(db_error)?;
        ensure_column(&connection, "tracks", "artwork_cache_key", "TEXT")?;
        ensure_column(&connection, "tracks", "artwork_source", "TEXT")?;
        ensure_column(&connection, "tracks", "artwork_mime_type", "TEXT")?;
        Ok(())
    }

    fn connection(&self) -> io::Result<Connection> {
        Connection::open(&self.path).map_err(db_error)
    }

    fn load_library(&self, scan_root: PathBuf) -> io::Result<Library> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, title, artist, album, relative_path, path, mime_type, file_size,
                        artwork_cache_key, artwork_source, artwork_mime_type
                 FROM tracks
                 ORDER BY artist, album, title, relative_path",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(LibraryTrack {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    artist: row.get(2)?,
                    album: row.get(3)?,
                    relative_path: row.get(4)?,
                    path: PathBuf::from(row.get::<_, String>(5)?),
                    mime_type: row.get(6)?,
                    file_size: row.get(7)?,
                    artwork: match (
                        row.get::<_, Option<String>>(8)?,
                        row.get::<_, Option<String>>(9)?,
                        row.get::<_, Option<String>>(10)?,
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
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        transaction
            .execute("DELETE FROM tracks", [])
            .map_err(db_error)?;
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO tracks
                     (id, title, artist, album, relative_path, path, mime_type, file_size,
                      artwork_cache_key, artwork_source, artwork_mime_type)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
                        track.title,
                        track.artist,
                        track.album,
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
        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    fn list_renderers(&self) -> io::Result<Vec<RendererRecord>> {
        let connection = self.connection()?;
        let selected = self.last_selected_renderer_location()?;
        let mut statement = connection
            .prepare(
                "SELECT location, name, manufacturer, model_name, av_transport_control_url, last_seen_unix
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
                    last_seen_unix: row.get(5)?,
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

    fn upsert_renderer(&self, renderer: &RendererRecord) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO renderers
                 (location, name, manufacturer, model_name, av_transport_control_url, last_seen_unix)
                 VALUES (?, ?, ?, ?, ?, ?)
                 ON CONFLICT(location) DO UPDATE SET
                    name = excluded.name,
                    manufacturer = COALESCE(excluded.manufacturer, renderers.manufacturer),
                    model_name = COALESCE(excluded.model_name, renderers.model_name),
                    av_transport_control_url = COALESCE(excluded.av_transport_control_url, renderers.av_transport_control_url),
                    last_seen_unix = excluded.last_seen_unix",
                params![
                    renderer.location,
                    renderer.name,
                    renderer.manufacturer,
                    renderer.model_name,
                    renderer.av_transport_control_url,
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
        connection
            .query_row(
                "SELECT value FROM app_state WHERE key = 'last_renderer_location'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)
    }
}

impl ServiceState {
    fn load(config: AppConfig) -> io::Result<Self> {
        let database = Database::open(&config.config_path)?;
        let persisted_library = database.load_library(config.library_path.clone())?;
        let state = Self {
            config,
            database,
            library: Mutex::new(persisted_library),
        };

        match scan_library(&state.config.library_path, &state.config.config_path) {
            Ok(library) => state.replace_library(library)?,
            Err(error) if state.track_count() > 0 => {
                eprintln!("library scan failed, continuing with persisted index: {error}");
            }
            Err(error) => return Err(error),
        }

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

    fn find_track(&self, track_id: &str) -> Option<LibraryTrack> {
        self.library.lock().ok().and_then(|library| {
            library
                .tracks
                .iter()
                .find(|track| track.id == track_id)
                .cloned()
        })
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
        let renderer = RendererRecord {
            location: location.to_string(),
            name: location.to_string(),
            manufacturer: None,
            model_name: None,
            av_transport_control_url: None,
            last_seen_unix: now_unix_timestamp(),
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
    ) -> io::Result<()> {
        let renderer = RendererRecord {
            location: location.to_string(),
            name: name.to_string(),
            manufacturer: manufacturer.map(ToString::to_string),
            model_name: model_name.map(ToString::to_string),
            av_transport_control_url: av_transport_control_url.map(ToString::to_string),
            last_seen_unix: now_unix_timestamp(),
        };
        self.database.upsert_renderer(&renderer)?;
        self.database.set_last_selected_renderer_location(location)
    }

    fn track_artwork_path(&self, track: &LibraryTrack) -> Option<PathBuf> {
        track
            .artwork
            .as_ref()
            .map(|artwork| artwork_cache_path(&self.config.config_path, &artwork.cache_key))
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
    tracks.sort_by(|left, right| {
        (
            left.artist.as_str(),
            left.album.as_str(),
            left.title.as_str(),
            left.relative_path.as_str(),
        )
            .cmp(&(
                right.artist.as_str(),
                right.album.as_str(),
                right.title.as_str(),
                right.relative_path.as_str(),
            ))
    });

    Ok(Library {
        scan_root: root.to_path_buf(),
        tracks,
    })
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
        let mime_type = infer_mime_type(&path).to_string();
        let id = stable_track_id(&relative_path);
        let artwork =
            resolve_track_artwork(root, &path, &relative_components, &id, artwork_cache_dir);

        tracks.push(LibraryTrack {
            id,
            title,
            artist,
            album,
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
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())?;
    let picture = tag
        .get_picture_type(PictureType::CoverFront)
        .or_else(|| tag.pictures().first())?;
    let mime_type = picture
        .mime_type()
        .map(|value| value.as_str().to_string())
        .or_else(|| infer_image_mime_from_bytes(picture.data()).map(ToString::to_string))?;
    let extension = image_extension_for_mime(&mime_type)?;

    Some(ArtworkCandidate {
        cache_key: stable_track_id(&format!("embedded:{track_id}")),
        source: format!("Embedded artwork ({:?})", picture.pic_type()),
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

fn db_error(error: rusqlite::Error) -> io::Error {
    io::Error::other(format!("sqlite error: {error}"))
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
    println!("HTTP base URL: {}", config.base_url);
    println!("Discovery timeout: {}ms", config.discovery_timeout_ms);
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
        artwork_name_priority, cleanup_track_label, decode_id3v1_text, infer_artist_and_album,
        infer_image_mime_from_bytes, parse_query_string, parse_range_header,
        parse_vorbis_comment_block, should_skip_entry, stable_track_id,
    };

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
}
