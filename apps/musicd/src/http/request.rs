use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read};
use std::net::TcpStream;

use crate::util::percent_decode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HttpRequest {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) path: String,
    pub(crate) query: HashMap<String, String>,
    pub(crate) form: HashMap<String, String>,
    pub(crate) range_header: Option<String>,
    pub(crate) content_type: Option<String>,
    pub(crate) body: Vec<u8>,
}

pub(crate) fn read_http_request(
    reader: &mut BufReader<TcpStream>,
) -> io::Result<Option<HttpRequest>> {
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

pub(crate) fn parse_request_form(content_type: Option<&str>, body: &[u8]) -> HashMap<String, String> {
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
