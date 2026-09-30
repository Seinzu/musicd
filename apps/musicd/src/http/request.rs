use std::collections::HashMap;
use std::fmt;
use std::io::{self, BufRead, Read};
use std::net::IpAddr;

use crate::util::percent_decode;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HttpRequest {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) path: String,
    pub(crate) query: HashMap<String, String>,
    pub(crate) form: HashMap<String, String>,
    pub(crate) range_header: Option<String>,
    pub(crate) content_type: Option<String>,
    pub(crate) authorization: Option<String>,
    pub(crate) cookie: Option<String>,
    pub(crate) peer: Option<IpAddr>,
    pub(crate) body: Vec<u8>,
}

/// Longest request line or header line accepted, including the line ending.
pub(crate) const MAX_LINE_BYTES: usize = 64 * 1024;
/// Most header lines accepted in one request.
pub(crate) const MAX_HEADER_COUNT: usize = 100;
/// Largest request body accepted. Recommendation imports are the biggest
/// bodies clients send, and they stay well under this.
pub(crate) const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// A request the server refuses to read any further, with the status to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RequestRejected {
    pub(crate) status: &'static str,
    pub(crate) message: &'static str,
}

impl fmt::Display for RequestRejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.status, self.message)
    }
}

impl std::error::Error for RequestRejected {}

fn reject(status: &'static str, message: &'static str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        RequestRejected { status, message },
    )
}

pub(crate) fn request_rejection(error: &io::Error) -> Option<RequestRejected> {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<RequestRejected>())
        .copied()
}

pub(crate) fn read_http_request<R: BufRead>(reader: &mut R) -> io::Result<Option<HttpRequest>> {
    let Some(request_line) = read_bounded_line(reader, "414 URI Too Long")? else {
        return Ok(None);
    };

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query) = split_target_and_query(&target);

    let mut range_header = None;
    let mut content_type = None;
    let mut authorization = None;
    let mut cookie = None;
    let mut content_length = 0_usize;
    let mut header_count = 0_usize;
    loop {
        let Some(line) = read_bounded_line(reader, "431 Request Header Fields Too Large")? else {
            break;
        };
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        header_count += 1;
        if header_count > MAX_HEADER_COUNT {
            return Err(reject(
                "431 Request Header Fields Too Large",
                "too many request headers",
            ));
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            let value = value.trim();
            if name.eq_ignore_ascii_case("Range") {
                range_header = Some(value.to_string());
            } else if name.eq_ignore_ascii_case("Content-Type") {
                content_type = Some(value.to_string());
            } else if name.eq_ignore_ascii_case("Authorization") {
                authorization = Some(value.to_string());
            } else if name.eq_ignore_ascii_case("Cookie") {
                cookie = Some(value.to_string());
            } else if name.eq_ignore_ascii_case("Content-Length") {
                content_length = value
                    .parse::<usize>()
                    .map_err(|_| reject("400 Bad Request", "invalid Content-Length"))?;
            }
        }
    }

    if content_length > MAX_BODY_BYTES {
        return Err(reject("413 Content Too Large", "request body is too large"));
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
        authorization,
        cookie,
        // The caller knows the socket and fills this in.
        peer: None,
        body,
    }))
}

/// Reads one line of at most `MAX_LINE_BYTES`, or `None` at end of stream.
fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    too_long_status: &'static str,
) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    reader
        .by_ref()
        .take(MAX_LINE_BYTES as u64 + 1)
        .read_until(b'\n', &mut line)?;
    if line.is_empty() {
        return Ok(None);
    }
    if line.len() > MAX_LINE_BYTES {
        return Err(reject(too_long_status, "request line is too long"));
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|_| reject("400 Bad Request", "request line is not valid UTF-8"))
}

pub(crate) fn split_target_and_query(target: &str) -> (String, HashMap<String, String>) {
    match target.split_once('?') {
        Some((path, query)) => (path.to_string(), parse_query_string(query)),
        None => (target.to_string(), HashMap::new()),
    }
}

pub(crate) fn parse_query_string(query: &str) -> HashMap<String, String> {
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

pub(crate) fn parse_request_form(
    content_type: Option<&str>,
    body: &[u8],
) -> HashMap<String, String> {
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

pub(crate) fn request_value<'a>(request: &'a HttpRequest, key: &str) -> Option<&'a str> {
    request
        .form
        .get(key)
        .or_else(|| request.query.get(key))
        .map(String::as_str)
}

pub(crate) fn parse_range_header(value: &str, total_len: u64) -> Option<(u64, u64)> {
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

#[cfg(test)]
mod tests {
    use super::{
        MAX_BODY_BYTES, MAX_HEADER_COUNT, MAX_LINE_BYTES, read_http_request, request_rejection,
    };
    use std::io::Cursor;

    fn rejection_status(raw: Vec<u8>) -> &'static str {
        let error = read_http_request(&mut Cursor::new(raw)).unwrap_err();
        request_rejection(&error)
            .expect("request should be rejected")
            .status
    }

    #[test]
    fn reads_body_within_limits() {
        let raw = b"POST /api/play HTTP/1.1\r\nHost: 192.168.1.20:8787\r\nAuthorization: Bearer mdt_abc\r\nCookie: musicd_session=s1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 11\r\n\r\ntrack_id=a1";
        let request = read_http_request(&mut Cursor::new(raw.to_vec()))
            .unwrap()
            .unwrap();
        assert_eq!(request.form.get("track_id").map(String::as_str), Some("a1"));
        assert_eq!(request.authorization.as_deref(), Some("Bearer mdt_abc"));
        assert_eq!(request.cookie.as_deref(), Some("musicd_session=s1"));
        assert_eq!(request.peer, None);
    }

    #[test]
    fn rejects_oversized_content_length_before_allocating() {
        let raw = b"POST /api/play HTTP/1.1\r\nContent-Length: 1000000000000\r\n\r\n".to_vec();
        assert_eq!(rejection_status(raw), "413 Content Too Large");
        let raw = format!(
            "POST /api/play HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY_BYTES + 1
        );
        assert_eq!(rejection_status(raw.into_bytes()), "413 Content Too Large");
    }

    #[test]
    fn rejects_invalid_content_length() {
        let raw = b"POST /api/play HTTP/1.1\r\nContent-Length: lots\r\n\r\n".to_vec();
        assert_eq!(rejection_status(raw), "400 Bad Request");
    }

    #[test]
    fn rejects_overlong_lines_and_too_many_headers() {
        let raw = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(MAX_LINE_BYTES));
        assert_eq!(rejection_status(raw.into_bytes()), "414 URI Too Long");

        let raw = format!(
            "GET / HTTP/1.1\r\nX-Big: {}\r\n\r\n",
            "a".repeat(MAX_LINE_BYTES)
        );
        assert_eq!(
            rejection_status(raw.into_bytes()),
            "431 Request Header Fields Too Large"
        );

        let headers = "X-Filler: 1\r\n".repeat(MAX_HEADER_COUNT + 1);
        let raw = format!("GET / HTTP/1.1\r\n{headers}\r\n");
        assert_eq!(
            rejection_status(raw.into_bytes()),
            "431 Request Header Fields Too Large"
        );
    }
}
