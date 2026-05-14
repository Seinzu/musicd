use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io;

use crate::types::{
    AlbumRecommendation, AlbumSummary, RecommendationImportRequest, RecommendationSeed,
};
use crate::util::now_unix_timestamp;

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
                let tag_release_id = album.metadata.musicbrainz_release_id.clone();
                let release_id = tag_release_id
                    .clone()
                    .or_else(|| overrides_by_album.get(&album.id).cloned())?;
                Some(RecommendationSeed {
                    album_id: album.id.clone(),
                    title: album.title.clone(),
                    artist: album.artist.clone(),
                    track_count: album.track_count,
                    first_track_id: album.first_track_id.clone(),
                    artwork_url: album.artwork_url.clone(),
                    musicbrainz_release_id: release_id,
                    musicbrainz_release_group_id: album
                        .metadata
                        .musicbrainz_release_group_id
                        .clone(),
                    source: if tag_release_id.is_some() {
                        "embedded_tags".to_string()
                    } else {
                        "artwork_override".to_string()
                    },
                    release_date: album
                        .metadata
                        .release_date
                        .clone()
                        .or_else(|| album.metadata.original_release_date.clone()),
                    genres: album.metadata.genres.clone(),
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

    pub(crate) fn album_recommendations_for_display(
        &self,
        seed_album_id: Option<&str>,
        status: Option<&str>,
        exclude_library: bool,
        randomize: bool,
        limit: Option<usize>,
    ) -> Vec<AlbumRecommendation> {
        let mut recommendations = self.album_recommendations(seed_album_id);
        if let Some(status) = status.map(str::trim).filter(|value| !value.is_empty()) {
            recommendations.retain(|recommendation| recommendation.status == status);
        }
        if exclude_library {
            let library_index = RecommendationLibraryIndex::new(&self.albums_snapshot());
            recommendations.retain(|recommendation| !library_index.contains(recommendation));
        }
        if randomize {
            let seed = now_unix_timestamp();
            recommendations.sort_by_key(|recommendation| {
                random_recommendation_key(&recommendation.recommendation_key, seed)
            });
        }
        if let Some(limit) = limit {
            recommendations.truncate(limit);
        }
        recommendations
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

    pub(crate) fn delete_album_recommendations(&self) -> io::Result<usize> {
        self.database.delete_album_recommendations()
    }
}

struct RecommendationLibraryIndex {
    album_ids: HashSet<String>,
    release_ids: HashSet<String>,
    release_group_ids: HashSet<String>,
    normalized_artist_titles: HashSet<(String, String)>,
    relaxed_artist_titles: HashSet<(String, String)>,
}

impl RecommendationLibraryIndex {
    fn new(albums: &[AlbumSummary]) -> Self {
        Self {
            album_ids: albums.iter().map(|album| album.id.clone()).collect(),
            release_ids: albums
                .iter()
                .filter_map(|album| album.metadata.musicbrainz_release_id.clone())
                .collect(),
            release_group_ids: albums
                .iter()
                .filter_map(|album| album.metadata.musicbrainz_release_group_id.clone())
                .collect(),
            normalized_artist_titles: albums
                .iter()
                .map(|album| {
                    (
                        normalize_recommendation_match_text(&album.artist),
                        normalize_recommendation_match_text(&album.title),
                    )
                })
                .collect(),
            relaxed_artist_titles: albums
                .iter()
                .map(|album| {
                    (
                        normalize_recommendation_match_text(&album.artist),
                        relaxed_album_title_match_text(&album.title),
                    )
                })
                .collect(),
        }
    }

    fn contains(&self, recommendation: &AlbumRecommendation) -> bool {
        if recommendation
            .suggested_musicbrainz_release_id
            .as_ref()
            .is_some_and(|release_id| self.release_ids.contains(release_id))
        {
            return true;
        }
        if recommendation
            .suggested_musicbrainz_release_group_id
            .as_ref()
            .is_some_and(|release_group_id| self.release_group_ids.contains(release_group_id))
        {
            return true;
        }
        if recommendation
            .artwork_url
            .as_deref()
            .and_then(album_id_from_artwork_url)
            .is_some_and(|album_id| self.album_ids.contains(album_id))
        {
            return true;
        }

        let normalized_artist =
            normalize_recommendation_match_text(&recommendation.suggested_artist);
        let normalized_title = normalize_recommendation_match_text(&recommendation.suggested_title);
        if self
            .normalized_artist_titles
            .contains(&(normalized_artist.clone(), normalized_title))
        {
            return true;
        }

        self.relaxed_artist_titles.contains(&(
            normalized_artist,
            relaxed_album_title_match_text(&recommendation.suggested_title),
        ))
    }
}

fn random_recommendation_key(recommendation_key: &str, seed: i64) -> u64 {
    let mut hasher = DefaultHasher::new();
    recommendation_key.hash(&mut hasher);
    seed.hash(&mut hasher);
    hasher.finish()
}

fn album_id_from_artwork_url(url: &str) -> Option<&str> {
    if !url.contains("/artwork/album/") {
        return None;
    }
    url.split('?')
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|album_id| !album_id.is_empty())
}

fn normalize_recommendation_match_text(value: &str) -> String {
    let mut normalized = String::new();
    for ch in value.chars() {
        if ch.is_alphanumeric() {
            normalized.extend(ch.to_lowercase());
        } else {
            normalized.push(' ');
        }
    }
    let mut words = normalized
        .split_whitespace()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if matches!(words.first().map(String::as_str), Some("the" | "a" | "an")) {
        words.remove(0);
    }
    words.join(" ")
}

fn relaxed_album_title_match_text(value: &str) -> String {
    let without_bracketed_text = strip_bracketed_text(value);
    normalize_recommendation_match_text(&without_bracketed_text)
        .split_whitespace()
        .filter(|word| !is_edition_word(word))
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_bracketed_text(value: &str) -> String {
    let mut result = String::new();
    let mut depth = 0usize;
    for ch in value.chars() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => result.push(ch),
            _ => {}
        }
    }
    result
}

fn is_edition_word(word: &str) -> bool {
    matches!(
        word,
        "anniversary"
            | "bonus"
            | "deluxe"
            | "edition"
            | "expanded"
            | "explicit"
            | "mono"
            | "remaster"
            | "remastered"
            | "special"
            | "stereo"
            | "version"
    )
}

#[cfg(test)]
mod tests {
    use super::{normalize_recommendation_match_text, relaxed_album_title_match_text};

    #[test]
    fn normalizes_recommendation_artist_text_for_matching() {
        assert_eq!(
            normalize_recommendation_match_text("The Radio Dept."),
            "radio dept"
        );
        assert_eq!(normalize_recommendation_match_text("R.E.M."), "r e m");
    }

    #[test]
    fn relaxes_album_title_edition_text_for_matching() {
        assert_eq!(
            relaxed_album_title_match_text("In Rainbows (Deluxe Edition)"),
            "in rainbows"
        );
        assert_eq!(
            relaxed_album_title_match_text("Kid A [20th Anniversary Remastered]"),
            "kid a"
        );
    }
}
