use std::collections::HashMap;
use std::io;

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
