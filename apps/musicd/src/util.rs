use std::fmt;
use std::path::{Component, Path};
use std::time::{SystemTime, UNIX_EPOCH};

/// HTML-escaping `Display` wrapper. Use with `write!` to escape into an
/// existing buffer in a single pass, with no intermediate `String`.
pub(crate) struct EscapeHtml<'a>(pub(crate) &'a str);

impl fmt::Display for EscapeHtml<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0.as_bytes();
        let mut last_flushed = 0;
        for (i, &byte) in bytes.iter().enumerate() {
            let escape = match byte {
                b'&' => "&amp;",
                b'<' => "&lt;",
                b'>' => "&gt;",
                b'"' => "&quot;",
                b'\'' => "&#39;",
                _ => continue,
            };
            if last_flushed < i {
                f.write_str(&self.0[last_flushed..i])?;
            }
            f.write_str(escape)?;
            last_flushed = i + 1;
        }
        if last_flushed < bytes.len() {
            f.write_str(&self.0[last_flushed..])?;
        }
        Ok(())
    }
}

pub(crate) fn html_escape(value: &str) -> String {
    use std::fmt::Write;
    let mut output = String::with_capacity(value.len());
    write!(output, "{}", EscapeHtml(value)).expect("formatting into String never fails");
    output
}

pub(crate) fn json_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

pub(crate) fn percent_decode(value: &str) -> String {
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

pub(crate) fn url_encode(value: &str) -> String {
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

pub(crate) fn option_u32_json(value: Option<u32>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

pub(crate) fn option_u64_json(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

pub(crate) fn option_i64_json(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

pub(crate) fn option_bool_json(value: Option<bool>) -> String {
    value.map(bool_json).unwrap_or_else(|| "null".to_string())
}

pub(crate) fn bool_json(value: bool) -> String {
    if value {
        "true".to_string()
    } else {
        "false".to_string()
    }
}

pub(crate) fn string_list_json(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!(r#""{}""#, json_escape(value)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub(crate) fn option_json_fragment(value: Option<&str>) -> String {
    value.unwrap_or("null").to_string()
}

pub(crate) fn option_string_json(value: Option<&str>) -> String {
    value
        .map(|value| format!(r#""{}""#, json_escape(value)))
        .unwrap_or_else(|| "null".to_string())
}

pub(crate) fn format_track_position(disc_number: Option<u32>, track_number: Option<u32>) -> String {
    match (disc_number, track_number) {
        (Some(disc), Some(track)) => format!("Disc {disc} • Track {track}"),
        (None, Some(track)) => format!("Track {track}"),
        (Some(disc), None) => format!("Disc {disc}"),
        (None, None) => "Unknown position".to_string(),
    }
}

pub(crate) fn format_duration_seconds(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub(crate) fn infer_mime_type(path: &Path) -> &'static str {
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

pub(crate) fn file_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

pub(crate) fn component_to_string(component: Component<'_>) -> Option<String> {
    component.as_os_str().to_str().map(ToString::to_string)
}

pub(crate) fn is_supported_audio_file(path: &Path) -> bool {
    matches!(
        file_extension(path).as_deref(),
        Some("flac" | "wav" | "aiff" | "aif" | "alac" | "m4a" | "aac" | "mp3" | "ogg" | "dsf")
    )
}

pub(crate) fn should_skip_entry(file_name: &str) -> bool {
    file_name.starts_with('.') || file_name == "@eaDir"
}

pub(crate) fn looks_like_disc_folder(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase().replace('_', " ");
    normalized.starts_with("disc ")
        || normalized.starts_with("disk ")
        || normalized.starts_with("cd ")
        || normalized == "disc1"
        || normalized == "disc 1"
        || normalized == "cd1"
        || normalized == "cd 1"
}

pub(crate) fn inferred_title(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("musicd track");
    cleanup_track_label(stem)
}

pub(crate) fn cleanup_track_label(value: &str) -> String {
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

pub(crate) fn infer_artist_and_album(relative_components: &[String]) -> (String, String) {
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

pub(crate) fn infer_disc_and_track_numbers(
    relative_components: &[String],
) -> (Option<u32>, Option<u32>) {
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

pub(crate) fn leading_number(value: &str) -> Option<u32> {
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

pub(crate) fn trailing_number(value: &str) -> Option<u32> {
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

pub(crate) fn now_unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
