use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::Path;

use crate::metrics;
use crate::util::{infer_mime_type, json_escape, url_encode};

use super::request::parse_range_header;

pub(crate) type ResponseWriter = BufWriter<TcpStream>;

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
