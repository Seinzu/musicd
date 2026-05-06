use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::ids::stable_track_id;
use crate::types::TrackArtwork;

mod mime;
mod musicbrainz;
mod sidecar;

pub(crate) use mime::{image_extension_for_mime, infer_image_mime_from_bytes};
pub(crate) use musicbrainz::{
    download_artwork_candidate, fetch_musicbrainz_cover_art_for_release, musicbrainz_client,
    search_musicbrainz_album_artwork,
};
#[cfg(test)]
pub(crate) use sidecar::artwork_name_priority;

#[derive(Debug)]
pub(crate) struct ArtworkCandidate {
    pub(crate) cache_key: String,
    pub(crate) source: String,
    pub(crate) mime_type: String,
    pub(crate) extension: &'static str,
    pub(crate) data: ArtworkData,
}

#[derive(Debug)]
pub(crate) enum ArtworkData {
    Bytes(Vec<u8>),
    File(PathBuf),
}

#[derive(Debug)]
pub(crate) struct EmbeddedPicture {
    pub(crate) bytes: Vec<u8>,
    pub(crate) mime_type: String,
    pub(crate) pic_type: String,
    pub(crate) tag_label: String,
}

pub(crate) struct DownloadedArtwork {
    pub(crate) bytes: Vec<u8>,
    pub(crate) mime_type: String,
}

/// Resolve cover art for a track using a pre-extracted embedded picture (so we
/// don't reopen the file) plus a sidecar lookup. The cache key is derived from
/// `album_id` for embedded art so all tracks on an album share a single cached
/// blob on disk; callers should dedupe their resolve calls per album.
pub(crate) fn resolve_track_artwork(
    root: &Path,
    track_path: &Path,
    relative_components: &[String],
    album_id: &str,
    embedded: Option<EmbeddedPicture>,
    artwork_cache_dir: &Path,
) -> Option<TrackArtwork> {
    let candidate = embedded
        .and_then(|picture| candidate_from_embedded(picture, album_id))
        .or_else(|| sidecar::find_sidecar_artwork(root, track_path, relative_components));
    let candidate = candidate?;

    let destination =
        artwork_cache_dir.join(format!("{}.{}", candidate.cache_key, candidate.extension));
    if persist_artwork_candidate(&candidate, &destination).is_err() {
        return None;
    }

    Some(TrackArtwork {
        cache_key: format!("{}.{}", candidate.cache_key, candidate.extension),
        source: candidate.source,
        mime_type: candidate.mime_type,
    })
}

fn candidate_from_embedded(picture: EmbeddedPicture, album_id: &str) -> Option<ArtworkCandidate> {
    let extension = image_extension_for_mime(&picture.mime_type)?;
    Some(ArtworkCandidate {
        cache_key: stable_track_id(&format!("embedded:{album_id}")),
        source: format!("Embedded artwork ({}, {})", picture.pic_type, picture.tag_label),
        mime_type: picture.mime_type,
        extension,
        data: ArtworkData::Bytes(picture.bytes),
    })
}

fn persist_artwork_candidate(candidate: &ArtworkCandidate, destination: &Path) -> io::Result<()> {
    match &candidate.data {
        ArtworkData::Bytes(bytes) => fs::write(destination, bytes),
        ArtworkData::File(source) => {
            fs::copy(source, destination)?;
            Ok(())
        }
    }
}

pub(crate) fn artwork_cache_path(config_path: &Path, cache_key: &str) -> PathBuf {
    config_path.join("artwork").join(cache_key)
}
