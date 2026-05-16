use std::io;

use rusqlite::{OptionalExtension, params};

use crate::ids::stable_track_id;
use crate::types::{AlbumRecommendation, RecommendationImportItem};
use crate::util::now_unix_timestamp;

use super::{Database, db_error};

impl Database {
    pub(crate) fn list_album_recommendations(
        &self,
        seed_album_id: Option<&str>,
    ) -> io::Result<Vec<AlbumRecommendation>> {
        let connection = self.connection()?;
        let sql = recommendation_select_sql(seed_album_id.is_some());
        let mut statement = connection.prepare(sql).map_err(db_error)?;
        let rows = if let Some(seed_album_id) = seed_album_id {
            statement
                .query_map([seed_album_id], recommendation_from_row)
                .map_err(db_error)?
        } else {
            statement
                .query_map([], recommendation_from_row)
                .map_err(db_error)?
        };

        let mut recommendations = Vec::new();
        for row in rows {
            recommendations.push(row.map_err(db_error)?);
        }
        Ok(recommendations)
    }

    pub(crate) fn upsert_album_recommendations(
        &self,
        default_source: &str,
        default_batch_id: Option<&str>,
        items: &[RecommendationImportItem],
    ) -> io::Result<usize> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        let now = now_unix_timestamp();
        let mut imported = 0usize;
        for item in items {
            let source = normalized_text(Some(item.source.as_deref().unwrap_or(default_source)))
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "llm-import".to_string());
            let seed_album_id = normalized_text(Some(&item.seed_album_id)).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "seed_album_id is required")
            })?;
            let suggested_artist =
                normalized_text(Some(&item.suggested_artist)).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "suggested_artist is required")
                })?;
            let suggested_title =
                normalized_text(Some(&item.suggested_title)).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "suggested_title is required")
                })?;
            let status = normalized_text(item.status.as_deref())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "suggested".to_string());
            let batch_id = normalized_text(item.batch_id.as_deref())
                .or_else(|| normalized_text(default_batch_id));
            let recommendation_key = normalized_text(item.recommendation_key.as_deref())
                .unwrap_or_else(|| {
                    recommendation_key(
                        &source,
                        &seed_album_id,
                        item.suggested_musicbrainz_release_group_id.as_deref(),
                        item.suggested_musicbrainz_release_id.as_deref(),
                        &suggested_artist,
                        &suggested_title,
                    )
                });
            let existing_created_unix = transaction
                .query_row(
                    "SELECT created_unix FROM album_recommendations WHERE recommendation_key = ?",
                    [&recommendation_key],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(db_error)?;
            transaction
                .execute(
                    "INSERT INTO album_recommendations
                     (recommendation_key, source, batch_id, seed_album_id,
                      seed_musicbrainz_release_id, suggested_artist, suggested_title,
                      suggested_musicbrainz_release_id, suggested_musicbrainz_release_group_id,
                      confidence, rationale, external_url, tidal_url, artwork_url, status,
                      created_unix, updated_unix)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT(recommendation_key) DO UPDATE SET
                       source = excluded.source,
                       batch_id = excluded.batch_id,
                       seed_album_id = excluded.seed_album_id,
                       seed_musicbrainz_release_id = excluded.seed_musicbrainz_release_id,
                       suggested_artist = excluded.suggested_artist,
                       suggested_title = excluded.suggested_title,
                       suggested_musicbrainz_release_id = excluded.suggested_musicbrainz_release_id,
                       suggested_musicbrainz_release_group_id = excluded.suggested_musicbrainz_release_group_id,
                       confidence = excluded.confidence,
                       rationale = excluded.rationale,
                       external_url = excluded.external_url,
                       tidal_url = excluded.tidal_url,
                       artwork_url = excluded.artwork_url,
                       status = excluded.status,
                       updated_unix = excluded.updated_unix",
                    params![
                        recommendation_key,
                        source,
                        batch_id,
                        seed_album_id,
                        normalized_text(item.seed_musicbrainz_release_id.as_deref()),
                        suggested_artist,
                        suggested_title,
                        normalized_text(item.suggested_musicbrainz_release_id.as_deref()),
                        normalized_text(item.suggested_musicbrainz_release_group_id.as_deref()),
                        item.confidence,
                        normalized_text(item.rationale.as_deref()),
                        normalized_text(item.external_url.as_deref()),
                        normalized_text(item.tidal_url.as_deref()),
                        normalized_text(item.artwork_url.as_deref()),
                        status,
                        existing_created_unix.unwrap_or(now),
                        now,
                    ],
                )
                .map_err(db_error)?;
            imported += 1;
        }
        transaction.commit().map_err(db_error)?;
        Ok(imported)
    }

    pub(crate) fn delete_album_recommendations(&self) -> io::Result<usize> {
        let connection = self.connection()?;
        connection
            .execute("DELETE FROM album_recommendations", [])
            .map_err(db_error)
    }
}

fn recommendation_select_sql(filtered: bool) -> &'static str {
    if filtered {
        "SELECT recommendation_key, source, batch_id, seed_album_id,
                seed_musicbrainz_release_id, suggested_artist, suggested_title,
                suggested_musicbrainz_release_id, suggested_musicbrainz_release_group_id,
                confidence, rationale, external_url, tidal_url, artwork_url, status,
                created_unix, updated_unix
         FROM album_recommendations
         WHERE seed_album_id = ?
         ORDER BY status ASC, confidence DESC, updated_unix DESC, suggested_artist ASC, suggested_title ASC"
    } else {
        "SELECT recommendation_key, source, batch_id, seed_album_id,
                seed_musicbrainz_release_id, suggested_artist, suggested_title,
                suggested_musicbrainz_release_id, suggested_musicbrainz_release_group_id,
                confidence, rationale, external_url, tidal_url, artwork_url, status,
                created_unix, updated_unix
         FROM album_recommendations
         ORDER BY seed_album_id ASC, status ASC, confidence DESC, updated_unix DESC"
    }
}

fn recommendation_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AlbumRecommendation> {
    Ok(AlbumRecommendation {
        recommendation_key: row.get(0)?,
        source: row.get(1)?,
        batch_id: row.get(2)?,
        seed_album_id: row.get(3)?,
        seed_musicbrainz_release_id: row.get(4)?,
        suggested_artist: row.get(5)?,
        suggested_title: row.get(6)?,
        suggested_musicbrainz_release_id: row.get(7)?,
        suggested_musicbrainz_release_group_id: row.get(8)?,
        confidence: row.get(9)?,
        rationale: row.get(10)?,
        external_url: row.get(11)?,
        tidal_url: row.get(12)?,
        artwork_url: row.get(13)?,
        status: row.get(14)?,
        created_unix: row.get(15)?,
        updated_unix: row.get(16)?,
    })
}

fn normalized_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn recommendation_key(
    source: &str,
    seed_album_id: &str,
    suggested_release_group_id: Option<&str>,
    suggested_release_id: Option<&str>,
    suggested_artist: &str,
    suggested_title: &str,
) -> String {
    let target = suggested_release_group_id
        .or(suggested_release_id)
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            format!(
                "{}:{}",
                suggested_artist.trim().to_ascii_lowercase(),
                suggested_title.trim().to_ascii_lowercase()
            )
        });
    stable_track_id(&format!(
        "album-recommendation:{source}:{seed_album_id}:{target}"
    ))
}
