use std::io::{self, BufReader, BufWriter, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::metrics;
use crate::service::ServiceState;

use super::ResponseWriter;
use super::guard::check_request;
use super::request::{HttpRequest, read_http_request, request_rejection};
use super::response::{
    is_expected_client_disconnect, respond_not_found, respond_text, respond_with_file,
};

const RESPONSE_BUFFER_BYTES: usize = 64 * 1024;
/// How long a client may take to send each read of its request. Responses
/// are not limited, since renderers stop reading streams while paused.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Most connections handled at once. Each one holds a thread, and streams and
/// event feeds stay open, so this is well above normal use.
const MAX_CONNECTIONS: usize = 512;

#[derive(Debug, Clone)]
pub(crate) enum ServerMode {
    SingleFile(Arc<PathBuf>),
    Service(Arc<ServiceState>),
}

pub(crate) fn serve_tcp(bind_address: &str, mode: ServerMode) -> io::Result<()> {
    let listener = TcpListener::bind(bind_address)?;
    let open_connections = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                let Some(slot) = ConnectionSlot::acquire(&open_connections) else {
                    let _ = stream.write_all(
                        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                };
                let mode = mode.clone();
                thread::spawn(move || {
                    let _slot = slot;
                    if let Err(error) = handle_client(stream, mode) {
                        if !is_expected_client_disconnect(&error) && !is_read_timeout(&error) {
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

/// Counts an open connection until dropped.
struct ConnectionSlot(Arc<AtomicUsize>);

impl ConnectionSlot {
    fn acquire(open_connections: &Arc<AtomicUsize>) -> Option<Self> {
        let previous = open_connections.fetch_add(1, Ordering::SeqCst);
        if previous >= MAX_CONNECTIONS {
            open_connections.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Self(Arc::clone(open_connections)))
    }
}

impl Drop for ConnectionSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn is_read_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

fn handle_client(stream: TcpStream, mode: ServerMode) -> io::Result<()> {
    let peer = stream.peer_addr().ok();
    stream.set_read_timeout(Some(REQUEST_READ_TIMEOUT))?;
    let mut writer: ResponseWriter =
        BufWriter::with_capacity(RESPONSE_BUFFER_BYTES, stream.try_clone()?);
    let mut reader = BufReader::new(stream);

    let mut request = match read_http_request(&mut reader) {
        Ok(Some(request)) => request,
        Ok(None) => return Ok(()),
        Err(error) => {
            let Some(rejection) = request_rejection(&error) else {
                return Err(error);
            };
            eprintln!(
                "{} -> rejected request: {rejection}",
                peer.map(|peer| peer.to_string())
                    .unwrap_or_else(|| "unknown-peer".to_string())
            );
            let _ = respond_text(
                &mut writer,
                rejection.status,
                "text/plain; charset=utf-8",
                rejection.message.as_bytes(),
                false,
            );
            let _ = writer.flush();
            return Ok(());
        }
    };

    request.peer = peer.map(|peer| peer.ip());

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
        ServerMode::Service(state) => match check_request(&request, &state.config) {
            Ok(()) => {
                super::router::handle_service_request(&mut writer, &request, Arc::clone(state))
            }
            Err(rejection) => {
                eprintln!(
                    "rejected {} {}: {}",
                    request.method,
                    request.path,
                    rejection.message()
                );
                respond_text(
                    &mut writer,
                    "403 Forbidden",
                    "text/plain; charset=utf-8",
                    rejection.message().as_bytes(),
                    request.method == "HEAD",
                )
            }
        },
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

    let _ = writer.flush();
    result
}

fn handle_single_file_request(
    writer: &mut ResponseWriter,
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
