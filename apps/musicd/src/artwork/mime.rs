use std::path::Path;

use crate::util::file_extension;

pub(crate) fn infer_image_mime_from_path(path: &Path) -> Option<&'static str> {
    match file_extension(path).as_deref()? {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

pub(crate) fn infer_image_mime_from_bytes(bytes: &[u8]) -> Option<&'static str> {
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

pub(crate) fn infer_image_mime_from_url(url: &str) -> Option<&'static str> {
    let clean = url.split('?').next().unwrap_or(url);
    infer_image_mime_from_path(Path::new(clean))
}

pub(crate) fn image_extension_for_mime(mime_type: &str) -> Option<&'static str> {
    match mime_type {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}
