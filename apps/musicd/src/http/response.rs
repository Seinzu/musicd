use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::metrics;
use crate::util::{infer_mime_type, json_escape, url_encode};

use super::request::parse_range_header;

pub(crate) type ResponseWriter = BufWriter<TcpStream>;

const TELEMETRY_COPY_BUFFER_BYTES: usize = 64 * 1024;
const SLOW_FILE_READ: Duration = Duration::from_millis(250);
const SLOW_SOCKET_WRITE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileTransferStage {
    FileRead,
    SocketWrite,
    SocketFlush,
}

impl FileTransferStage {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::FileRead => "file_read",
            Self::SocketWrite => "socket_write",
            Self::SocketFlush => "socket_flush",
        }
    }

    fn slow_threshold(self) -> Duration {
        match self {
            Self::FileRead => SLOW_FILE_READ,
            Self::SocketWrite | Self::SocketFlush => SLOW_SOCKET_WRITE,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FileTransferStall {
    pub(crate) stage: FileTransferStage,
    pub(crate) duration: Duration,
    pub(crate) bytes_transferred: u64,
    pub(crate) operation_bytes: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FileTransferTelemetry {
    pub(crate) expected_bytes: u64,
    pub(crate) bytes_transferred: u64,
    pub(crate) read_operations: u64,
    pub(crate) write_operations: u64,
    pub(crate) total_read_duration: Duration,
    pub(crate) total_write_duration: Duration,
    pub(crate) max_read_duration: Duration,
    pub(crate) max_write_duration: Duration,
    pub(crate) flush_duration: Duration,
    pub(crate) elapsed: Duration,
}

struct TransferMeasurement<'a, F> {
    telemetry: &'a mut FileTransferTelemetry,
    on_stall: &'a mut F,
}

impl<F> TransferMeasurement<'_, F>
where
    F: FnMut(FileTransferStall),
{
    fn observe(&mut self, stage: FileTransferStage, duration: Duration, operation_bytes: usize) {
        match stage {
            FileTransferStage::FileRead => {
                self.telemetry.read_operations += 1;
                self.telemetry.total_read_duration += duration;
                self.telemetry.max_read_duration = self.telemetry.max_read_duration.max(duration);
            }
            FileTransferStage::SocketWrite => {
                self.telemetry.write_operations += 1;
                self.telemetry.total_write_duration += duration;
                self.telemetry.max_write_duration = self.telemetry.max_write_duration.max(duration);
            }
            FileTransferStage::SocketFlush => {
                self.telemetry.flush_duration += duration;
            }
        }

        if duration >= stage.slow_threshold() {
            (self.on_stall)(FileTransferStall {
                stage,
                duration,
                bytes_transferred: self.telemetry.bytes_transferred,
                operation_bytes,
            });
        }
    }
}

pub(crate) fn respond_with_file(
    writer: &mut ResponseWriter,
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

pub(crate) fn respond_with_file_telemetry<F>(
    writer: &mut ResponseWriter,
    file_path: &Path,
    range_header: Option<String>,
    mut on_stall: F,
) -> (io::Result<()>, FileTransferTelemetry)
where
    F: FnMut(FileTransferStall),
{
    let started = Instant::now();
    let mut telemetry = FileTransferTelemetry::default();
    let result = respond_with_file_telemetry_inner(
        writer,
        file_path,
        range_header,
        &mut telemetry,
        &mut on_stall,
    );
    telemetry.elapsed = started.elapsed();
    (result, telemetry)
}

fn respond_with_file_telemetry_inner<F>(
    writer: &mut ResponseWriter,
    file_path: &Path,
    range_header: Option<String>,
    telemetry: &mut FileTransferTelemetry,
    on_stall: &mut F,
) -> io::Result<()>
where
    F: FnMut(FileTransferStall),
{
    let mut file = match File::open(file_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return respond_not_found(writer, false);
        }
        Err(error) => return Err(error),
    };

    let total_len = file.metadata()?.len();
    let mime_type = infer_mime_type(file_path);
    let response_range = range_header
        .as_deref()
        .and_then(|value| parse_range_header(value, total_len));
    let (start, end, status, content_range) = match response_range {
        Some((start, end)) => (
            start,
            end,
            "206 Partial Content",
            Some(format!("bytes {start}-{end}/{total_len}")),
        ),
        None => (0, total_len.saturating_sub(1), "200 OK", None),
    };
    let content_len = if total_len == 0 { 0 } else { end - start + 1 };
    telemetry.expected_bytes = content_len;

    let mut headers = vec![
        ("Content-Type".to_string(), mime_type.to_string()),
        ("Accept-Ranges".to_string(), "bytes".to_string()),
        ("Content-Length".to_string(), content_len.to_string()),
    ];
    if let Some(content_range) = content_range {
        headers.push(("Content-Range".to_string(), content_range));
    }
    write_response_owned(writer, status, &headers, None)?;
    if content_len == 0 {
        return Ok(());
    }

    file.seek(SeekFrom::Start(start))?;
    let mut measurement = TransferMeasurement {
        telemetry,
        on_stall,
    };
    copy_exact_bytes_with_telemetry(&mut file, writer, content_len, &mut measurement)
}

fn copy_exact_bytes(
    reader: &mut File,
    writer: &mut ResponseWriter,
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

fn copy_exact_bytes_with_telemetry<R, W, F>(
    reader: &mut R,
    writer: &mut W,
    mut bytes_left: u64,
    measurement: &mut TransferMeasurement<'_, F>,
) -> io::Result<()>
where
    R: Read,
    W: Write,
    F: FnMut(FileTransferStall),
{
    let mut buffer = [0_u8; TELEMETRY_COPY_BUFFER_BYTES];
    while bytes_left > 0 {
        let to_read = usize::try_from(bytes_left.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let read_started = Instant::now();
        let read_result = reader.read(&mut buffer[..to_read]);
        let read_duration = read_started.elapsed();
        let operation_bytes = read_result.as_ref().copied().unwrap_or(0);
        measurement.observe(FileTransferStage::FileRead, read_duration, operation_bytes);
        let read = read_result?;
        if read == 0 {
            break;
        }

        let write_started = Instant::now();
        let write_result = writer.write_all(&buffer[..read]);
        let write_duration = write_started.elapsed();
        measurement.observe(FileTransferStage::SocketWrite, write_duration, read);
        write_result?;

        measurement.telemetry.bytes_transferred += read as u64;
        bytes_left -= read as u64;
    }

    let flush_started = Instant::now();
    let flush_result = writer.flush();
    measurement.observe(FileTransferStage::SocketFlush, flush_started.elapsed(), 0);
    flush_result
}

#[cfg(test)]
mod tests {
    use super::{FileTransferTelemetry, TransferMeasurement, copy_exact_bytes_with_telemetry};
    use std::io::Cursor;

    #[test]
    fn measured_copy_separates_read_and_write_operations() {
        let input = vec![7_u8; 96 * 1024];
        let mut reader = Cursor::new(input);
        let mut output = Vec::new();
        let mut telemetry = FileTransferTelemetry {
            expected_bytes: 70_000,
            ..FileTransferTelemetry::default()
        };
        let mut stalls = Vec::new();
        let mut observer = |stall| stalls.push(stall);
        let mut measurement = TransferMeasurement {
            telemetry: &mut telemetry,
            on_stall: &mut observer,
        };

        copy_exact_bytes_with_telemetry(&mut reader, &mut output, 70_000, &mut measurement)
            .expect("measured copy should succeed");

        assert_eq!(output.len(), 70_000);
        assert_eq!(telemetry.bytes_transferred, 70_000);
        assert_eq!(telemetry.read_operations, 2);
        assert_eq!(telemetry.write_operations, 2);
        assert!(stalls.is_empty());
    }
}

pub(crate) fn respond_text(
    writer: &mut ResponseWriter,
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

/// Serve a static asset whose URL carries a version query string. The
/// `Cache-Control: ... immutable` directive lets browsers skip even
/// conditional revalidation; cache busts via the version-bumped URL.
pub(crate) fn respond_asset(
    writer: &mut ResponseWriter,
    content_type: &str,
    body: &[u8],
    head_only: bool,
) -> io::Result<()> {
    write_response_owned(
        writer,
        "200 OK",
        &[
            ("Content-Type".to_string(), content_type.to_string()),
            ("Content-Length".to_string(), body.len().to_string()),
            (
                "Cache-Control".to_string(),
                "public, max-age=31536000, immutable".to_string(),
            ),
        ],
        if head_only { None } else { Some(body) },
    )
}

pub(crate) fn respond_json(
    writer: &mut ResponseWriter,
    status: &str,
    body: &str,
) -> io::Result<()> {
    respond_text(
        writer,
        status,
        "application/json; charset=utf-8",
        body.as_bytes(),
        false,
    )
}

pub(crate) fn api_error(writer: &mut ResponseWriter, status: &str, error: &str) -> io::Result<()> {
    respond_json(
        writer,
        status,
        &format!(r#"{{"ok":false,"error":"{}"}}"#, json_escape(error)),
    )
}

pub(crate) fn is_expected_client_disconnect(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::UnexpectedEof
    )
}

pub(crate) fn respond_not_found(writer: &mut ResponseWriter, head_only: bool) -> io::Result<()> {
    respond_text(
        writer,
        "404 Not Found",
        "text/plain; charset=utf-8",
        b"not found",
        head_only,
    )
}

pub(crate) fn respond_method_not_allowed(writer: &mut ResponseWriter) -> io::Result<()> {
    respond_text(
        writer,
        "405 Method Not Allowed",
        "text/plain; charset=utf-8",
        b"method not allowed",
        false,
    )
}

pub(crate) fn redirect_home(
    writer: &mut ResponseWriter,
    renderer_location: Option<&str>,
    message: Option<&str>,
    error: Option<&str>,
) -> io::Result<()> {
    redirect_to_path(writer, "/", renderer_location, message, error)
}

pub(crate) fn redirect_to_path(
    writer: &mut ResponseWriter,
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

pub(crate) fn redirect_album(
    writer: &mut ResponseWriter,
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

pub(crate) fn write_response_owned(
    writer: &mut ResponseWriter,
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
