use std::collections::HashMap;
use std::path::PathBuf;

use musicd_upnp::RendererCapabilities;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryTrack {
    pub(crate) id: String,
    pub(crate) album_id: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) album: String,
    pub(crate) disc_number: Option<u32>,
    pub(crate) track_number: Option<u32>,
    pub(crate) duration_seconds: Option<u64>,
    pub(crate) relative_path: String,
    pub(crate) path: PathBuf,
    pub(crate) mime_type: String,
    pub(crate) file_size: u64,
    pub(crate) artwork: Option<TrackArtwork>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AlbumSummary {
    pub(crate) id: String,
    pub(crate) artist_id: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) track_count: usize,
    pub(crate) artwork_track_id: Option<String>,
    pub(crate) artwork: Option<TrackArtwork>,
    pub(crate) artwork_url: Option<String>,
    pub(crate) first_track_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArtistSummary {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) album_count: usize,
    pub(crate) track_count: usize,
    pub(crate) artwork_track_id: Option<String>,
    pub(crate) artwork_url: Option<String>,
    pub(crate) first_album_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrackArtwork {
    pub(crate) cache_key: String,
    pub(crate) source: String,
    pub(crate) mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AlbumArtworkOverride {
    pub(crate) album_id: String,
    pub(crate) cache_key: String,
    pub(crate) source: String,
    pub(crate) mime_type: String,
    pub(crate) musicbrainz_release_id: Option<String>,
    pub(crate) applied_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AlbumArtworkSearchCandidate {
    pub(crate) release_id: String,
    pub(crate) release_group_id: Option<String>,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) date: Option<String>,
    pub(crate) country: Option<String>,
    pub(crate) score: i32,
    pub(crate) thumbnail_url: String,
    pub(crate) image_url: String,
    pub(crate) source: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MusicBrainzSearchResponse {
    #[serde(default)]
    pub(crate) releases: Vec<MusicBrainzSearchRelease>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MusicBrainzSearchRelease {
    pub(crate) id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) date: Option<String>,
    #[serde(default)]
    pub(crate) country: Option<String>,
    #[serde(default)]
    pub(crate) score: Option<i32>,
    #[serde(rename = "artist-credit", default)]
    pub(crate) artist_credit: Vec<MusicBrainzArtistCredit>,
    #[serde(rename = "release-group", default)]
    pub(crate) release_group: Option<MusicBrainzReleaseGroupRef>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MusicBrainzArtistCredit {
    pub(crate) name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct MusicBrainzReleaseGroupRef {
    pub(crate) id: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CoverArtArchiveResponse {
    #[serde(default)]
    pub(crate) images: Vec<CoverArtArchiveImage>,
    #[serde(default)]
    pub(crate) release: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CoverArtArchiveImage {
    #[serde(default)]
    pub(crate) front: bool,
    #[serde(default)]
    pub(crate) approved: bool,
    pub(crate) image: String,
    #[serde(default)]
    pub(crate) thumbnails: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmbeddedMetadata {
    pub(crate) format_name: String,
    pub(crate) fields: Vec<(String, String)>,
    pub(crate) notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ParsedTrackTags {
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) album: Option<String>,
    pub(crate) disc_number: Option<u32>,
    pub(crate) track_number: Option<u32>,
    pub(crate) duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RendererRecord {
    pub(crate) location: String,
    pub(crate) name: String,
    pub(crate) manufacturer: Option<String>,
    pub(crate) model_name: Option<String>,
    pub(crate) av_transport_control_url: Option<String>,
    pub(crate) capabilities: RendererCapabilities,
    pub(crate) last_checked_unix: i64,
    pub(crate) last_reachable_unix: Option<i64>,
    pub(crate) last_error: Option<String>,
    pub(crate) last_seen_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaybackQueue {
    pub(crate) renderer_location: String,
    pub(crate) name: String,
    pub(crate) current_entry_id: Option<i64>,
    pub(crate) status: String,
    pub(crate) version: i64,
    pub(crate) updated_unix: i64,
    pub(crate) entries: Vec<QueueEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueueEntry {
    pub(crate) id: i64,
    pub(crate) position: i64,
    pub(crate) track_id: String,
    pub(crate) album_id: Option<String>,
    pub(crate) source_kind: String,
    pub(crate) source_ref: Option<String>,
    pub(crate) entry_status: String,
    pub(crate) started_unix: Option<i64>,
    pub(crate) completed_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlaybackSession {
    pub(crate) renderer_location: String,
    pub(crate) queue_entry_id: Option<i64>,
    pub(crate) next_queue_entry_id: Option<i64>,
    pub(crate) transport_state: String,
    pub(crate) current_track_uri: Option<String>,
    pub(crate) position_seconds: Option<u64>,
    pub(crate) duration_seconds: Option<u64>,
    pub(crate) last_observed_unix: i64,
    pub(crate) last_error: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrackPlayRecord {
    pub(crate) id: i64,
    pub(crate) track_id: String,
    pub(crate) renderer_location: String,
    pub(crate) queue_entry_id: Option<i64>,
    pub(crate) played_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueueMutationEntry {
    pub(crate) track_id: String,
    pub(crate) album_id: Option<String>,
    pub(crate) source_kind: String,
    pub(crate) source_ref: Option<String>,
}
