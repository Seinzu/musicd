use std::io::{self, BufReader};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Instant;

use crate::metrics;
use crate::service::ServiceState;

use super::request::{HttpRequest, read_http_request};
use super::response::{
    is_expected_client_disconnect, respond_not_found, respond_text, respond_with_file,
};

#[derive(Debug, Clone)]
pub(crate) enum ServerMode {
    SingleFile(Arc<PathBuf>),
    Service(Arc<ServiceState>),
}

pub(crate) fn serve_tcp(bind_address: &str, mode: ServerMode) -> io::Result<()> {
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
            super::router::handle_service_request(&mut writer, &request, Arc::clone(state))
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
