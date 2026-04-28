use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use musicd_core::AppConfig;
use musicd_upnp::{StreamResource, discover_renderers, inspect_renderer, play_stream};

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

    serve_single_file(path, bind_address)
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
        if let Err(error) = serve_single_file(server_path, &listener_address) {
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

fn serve_single_file(file_path: Arc<PathBuf>, bind_address: &str) -> io::Result<()> {
    let listener = TcpListener::bind(bind_address)?;
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let path = Arc::clone(&file_path);
                thread::spawn(move || {
                    if let Err(error) = handle_client(stream, path) {
                        eprintln!("request failed: {error}");
                    }
                });
            }
            Err(error) => eprintln!("accept failed: {error}"),
        }
    }
    Ok(())
}

fn handle_client(stream: TcpStream, file_path: Arc<PathBuf>) -> io::Result<()> {
    let peer = stream.peer_addr().ok();
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

    if let Some(peer) = peer {
        eprintln!("{peer} -> {method} {path}");
    } else {
        eprintln!("unknown-peer -> {method} {path}");
    }

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

    match (method, path) {
        ("GET", "/stream/current") | ("HEAD", "/stream/current") => respond_with_file(
            &mut writer,
            file_path.as_path(),
            method == "HEAD",
            range_header,
        ),
        ("GET", "/health") | ("HEAD", "/health") => {
            let body = b"ok";
            write_response(
                &mut writer,
                "200 OK",
                &[
                    ("Content-Type", "text/plain; charset=utf-8"),
                    ("Content-Length", "2"),
                ],
                if method == "HEAD" { None } else { Some(body) },
            )
        }
        _ => write_response(
            &mut writer,
            "404 Not Found",
            &[
                ("Content-Type", "text/plain; charset=utf-8"),
                ("Content-Length", "9"),
            ],
            if method == "HEAD" {
                None
            } else {
                Some(b"not found")
            },
        ),
    }
}

fn respond_with_file(
    writer: &mut TcpStream,
    file_path: &Path,
    head_only: bool,
    range_header: Option<String>,
) -> io::Result<()> {
    let mut file = File::open(file_path)?;
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

            write_response(
                writer,
                "206 Partial Content",
                &[
                    ("Content-Type", mime_type),
                    ("Accept-Ranges", "bytes"),
                    ("Content-Length", &content_length_text),
                    ("Content-Range", &content_range_text),
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
            write_response(
                writer,
                "200 OK",
                &[
                    ("Content-Type", mime_type),
                    ("Accept-Ranges", "bytes"),
                    ("Content-Length", &content_length_text),
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

fn write_response(
    writer: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
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

fn print_status() {
    let config = AppConfig::from_env();
    println!("musicd phase 1 scaffold");
    println!();
    println!("Library path: {}", config.library_path.display());
    println!("Bind address: {}", config.bind_address);
    println!("HTTP base URL: {}", config.base_url);
    println!();
    println!("Commands:");
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
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("musicd track")
        .to_string()
}

fn infer_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
    {
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

#[cfg(test)]
mod tests {
    use super::parse_range_header;

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
}
