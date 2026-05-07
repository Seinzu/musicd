use std::collections::HashMap;
use std::io;

use crate::library::inspect_embedded_metadata;
use crate::types::{AlbumRecommendation, RecommendationImportRequest, RecommendationSeed};

use super::ServiceState;

impl ServiceState {
    pub(crate) fn recommendation_seeds(&self) -> Vec<RecommendationSeed> {
        let overrides_by_album = self
            .database
            .list_album_artwork_overrides()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|override_record| {
                override_record
                    .musicbrainz_release_id
                    .map(|release_id| (override_record.album_id, release_id))
            })
            .collect::<HashMap<_, _>>();

        self.albums_snapshot()
            .iter()
            .filter_map(|album| {
                let tracks = self.tracks_for_album(&album.id);
                let first_track = tracks.first()?;
                let tag_metadata = inspect_embedded_metadata(&first_track.path).ok();
                let tag_release_id = tag_metadata
                    .as_ref()
                    .and_then(|metadata| metadata_value(&metadata.fields, RELEASE_ID_KEYS));
                let release_id = tag_release_id
                    .clone()
                    .or_else(|| overrides_by_album.get(&album.id).cloned())?;
                let release_group_id = tag_metadata
                    .as_ref()
                    .and_then(|metadata| metadata_value(&metadata.fields, RELEASE_GROUP_ID_KEYS));
                let release_date = tag_metadata
                    .as_ref()
                    .and_then(|metadata| metadata_value(&metadata.fields, RELEASE_DATE_KEYS));
                let genres = tag_metadata
                    .as_ref()
                    .map(|metadata| metadata_values(&metadata.fields, GENRE_KEYS))
                    .unwrap_or_default();
                Some(RecommendationSeed {
                    album_id: album.id.clone(),
                    title: album.title.clone(),
                    artist: album.artist.clone(),
                    track_count: album.track_count,
                    first_track_id: album.first_track_id.clone(),
                    artwork_url: album.artwork_url.clone(),
                    musicbrainz_release_id: release_id,
                    musicbrainz_release_group_id: release_group_id,
                    source: if tag_release_id.is_some() {
                        "embedded_tags".to_string()
                    } else {
                        "artwork_override".to_string()
                    },
                    release_date,
                    genres,
                })
            })
            .collect()
    }

    pub(crate) fn album_recommendations(
        &self,
        seed_album_id: Option<&str>,
    ) -> Vec<AlbumRecommendation> {
        self.database
            .list_album_recommendations(seed_album_id)
            .unwrap_or_default()
    }

    pub(crate) fn import_album_recommendations(
        &self,
        request: &RecommendationImportRequest,
    ) -> io::Result<usize> {
        let source = request.source.as_deref().unwrap_or("llm-import");
        self.database.upsert_album_recommendations(
            source,
            request.batch_id.as_deref(),
            &request.recommendations,
        )
    }
}

const RELEASE_ID_KEYS: &[&str] = &[
    "MUSICBRAINZ_ALBUMID",
    "MUSICBRAINZ_RELEASEID",
    "MUSICBRAINZ RELEASE ID",
    "MUSICBRAINZ_ALBUMID",
    "MB_RELEASEID",
];

const RELEASE_GROUP_ID_KEYS: &[&str] = &[
    "MUSICBRAINZ_RELEASEGROUPID",
    "MUSICBRAINZ_RELEASEGROUP_ID",
    "MUSICBRAINZ RELEASE GROUP ID",
    "MUSICBRAINZ_RELEASE_GROUP_ID",
    "MB_RELEASEGROUPID",
];

const RELEASE_DATE_KEYS: &[&str] = &[
    "DATE",
    "YEAR",
    "ORIGINALDATE",
    "ORIGINALYEAR",
    "RELEASEDATE",
    "RELEASE_DATE",
];

const GENRE_KEYS: &[&str] = &["GENRE", "STYLE"];

fn metadata_value(fields: &[(String, String)], keys: &[&str]) -> Option<String> {
    fields
        .iter()
        .find(|(key, value)| key_matches(key, keys) && !value.trim().is_empty())
        .map(|(_, value)| value.trim().to_string())
}

fn metadata_values(fields: &[(String, String)], keys: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    for (_, value) in fields
        .iter()
        .filter(|(key, value)| key_matches(key, keys) && !value.trim().is_empty())
    {
        for part in value.split([';', ',']) {
            let part = part.trim();
            if !part.is_empty() && !values.iter().any(|existing| existing == part) {
                values.push(part.to_string());
            }
        }
    }
    values
}

fn key_matches(key: &str, candidates: &[&str]) -> bool {
    let normalized = normalized_key(key);
    candidates
        .iter()
        .any(|candidate| normalized == normalized_key(candidate))
}

fn normalized_key(key: &str) -> String {
    key.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}
