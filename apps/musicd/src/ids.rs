use std::net::IpAddr;

pub(crate) fn stable_track_id(relative_path: &str) -> String {
    let mut hash = 1469598103934665603_u64;
    for byte in relative_path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{hash:016x}")
}

pub(crate) fn stable_album_id(artist: &str, album: &str) -> String {
    stable_track_id(&format!(
        "album:{}:{}",
        artist.trim().to_ascii_lowercase(),
        album.trim().to_ascii_lowercase()
    ))
}

pub(crate) fn stable_album_id_from_release(mbid: &str) -> String {
    stable_track_id(&format!("mb_release:{}", mbid.trim()))
}

pub(crate) fn stable_album_id_from_folder(folder_path: &str) -> String {
    stable_track_id(&format!(
        "folder:{}",
        folder_path.trim().to_ascii_lowercase()
    ))
}

pub(crate) fn stable_artist_id(artist: &str) -> String {
    stable_track_id(&format!("artist:{}", artist.trim().to_ascii_lowercase()))
}

pub(crate) fn normalized_renderer_name(
    location: &str,
    name: &str,
    model_name: Option<&str>,
) -> String {
    let trimmed_name = name.trim();
    let trimmed_model = model_name.map(str::trim).filter(|value| !value.is_empty());

    if renderer_name_looks_like_location(trimmed_name, location) {
        return trimmed_model
            .map(ToString::to_string)
            .or_else(|| renderer_location_host(location).map(ToString::to_string))
            .unwrap_or_else(|| location.to_string());
    }

    if trimmed_name.is_empty() {
        return trimmed_model
            .map(ToString::to_string)
            .or_else(|| renderer_location_host(location).map(ToString::to_string))
            .unwrap_or_else(|| location.to_string());
    }

    trimmed_name.to_string()
}

pub(crate) fn renderer_name_looks_like_location(name: &str, location: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed == location {
        return true;
    }
    if trimmed.parse::<IpAddr>().is_ok() {
        return true;
    }

    renderer_location_host(location)
        .map(|host| {
            trimmed.eq_ignore_ascii_case(host)
                || host.parse::<IpAddr>().is_ok() && trimmed.eq_ignore_ascii_case(host)
        })
        .unwrap_or(false)
}

pub(crate) fn renderer_location_host(location: &str) -> Option<&str> {
    let remainder = location
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(location);
    let authority = remainder.split('/').next()?.trim();
    if authority.is_empty() {
        return None;
    }
    let host = authority
        .strip_prefix('[')
        .and_then(|value| value.split_once(']').map(|(host, _)| host))
        .unwrap_or_else(|| authority.split(':').next().unwrap_or(authority))
        .trim();
    if host.is_empty() { None } else { Some(host) }
}
