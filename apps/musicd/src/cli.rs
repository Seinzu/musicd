use std::env;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use musicd_core::AppConfig;
use musicd_upnp::{StreamResource, discover_renderers, inspect_renderer, play_stream};

use crate::http::{ServerMode, serve_tcp};
use crate::metrics;
use crate::service::{ServiceState, spawn_queue_worker};
use crate::util::{infer_mime_type, inferred_title};

pub(crate) fn run() -> io::Result<()> {
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
