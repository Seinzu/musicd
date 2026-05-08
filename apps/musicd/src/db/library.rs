use std::io;
use std::path::PathBuf;

use rusqlite::{Connection, params};

use crate::ids::stable_album_id;
use crate::library::{Library, build_album_summaries, build_artist_summaries_from_albums};
use crate::types::{AlbumArtworkOverride, LibraryTrack, TrackArtwork, TrackMetadata};
#[cfg(test)]
use crate::types::{AlbumMetadata, AlbumSummary, ArtistSummary};

use super::{Database, db_error};

fn genres_json(genres: &[String]) -> Option<String> {
    if genres.is_empty() {
        None
    } else {
        serde_json::to_string(genres).ok()
    }
}

fn parse_genres_json(value: Option<String>) -> Vec<String> {
    value
        .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
        .unwrap_or_default()
}

impl Database {
    pub(crate) fn load_library(&self, scan_root: PathBuf) -> io::Result<Library> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, album_id, title, artist, album, disc_number, track_number,
                        duration_seconds, relative_path, path, mime_type, file_size,
                        artwork_cache_key, artwork_source, artwork_mime_type,
                        musicbrainz_release_id, musicbrainz_release_group_id,
                        musicbrainz_recording_id, musicbrainz_release_track_id,
                        release_date, original_release_date, release_country,
                        release_type, genres_json
                 FROM tracks
                 ORDER BY artist, album, COALESCE(disc_number, 0), COALESCE(track_number, 0), title, relative_path",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                let artist: String = row.get(3)?;
                let album: String = row.get(4)?;
                Ok(LibraryTrack {
                    id: row.get(0)?,
                    album_id: row
                        .get::<_, Option<String>>(1)?
                        .unwrap_or_else(|| stable_album_id(&artist, &album)),
                    title: row.get(2)?,
                    artist,
                    album,
                    disc_number: row.get(5)?,
                    track_number: row.get(6)?,
                    duration_seconds: row.get(7)?,
                    relative_path: row.get(8)?,
                    path: PathBuf::from(row.get::<_, String>(9)?),
                    mime_type: row.get(10)?,
                    file_size: row.get(11)?,
                    artwork: match (
                        row.get::<_, Option<String>>(12)?,
                        row.get::<_, Option<String>>(13)?,
                        row.get::<_, Option<String>>(14)?,
                    ) {
                        (Some(cache_key), Some(source), Some(mime_type)) => Some(TrackArtwork {
                            cache_key,
                            source,
                            mime_type,
                        }),
                        _ => None,
                    },
                    metadata: TrackMetadata {
                        musicbrainz_release_id: row.get(15)?,
                        musicbrainz_release_group_id: row.get(16)?,
                        musicbrainz_recording_id: row.get(17)?,
                        musicbrainz_release_track_id: row.get(18)?,
                        release_date: row.get(19)?,
                        original_release_date: row.get(20)?,
                        release_country: row.get(21)?,
                        release_type: row.get(22)?,
                        genres: parse_genres_json(row.get(23)?),
                    },
                })
            })
            .map_err(db_error)?;

        let mut tracks = Vec::new();
        for row in rows {
            tracks.push(row.map_err(db_error)?);
        }
        drop(statement);

        let overrides = Self::list_album_artwork_overrides_with(&connection)?;
        Ok(Library::build(scan_root, tracks, &overrides))
    }

    pub(crate) fn save_library(&self, library: &Library) -> io::Result<()> {
        let albums = build_album_summaries(&library.tracks);
        let artists = build_artist_summaries_from_albums(&library.tracks, &albums);
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(db_error)?;
        transaction
            .execute("DELETE FROM tracks", [])
            .map_err(db_error)?;
        transaction
            .execute("DELETE FROM albums", [])
            .map_err(db_error)?;
        transaction
            .execute("DELETE FROM artists", [])
            .map_err(db_error)?;
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO tracks
                     (id, album_id, title, artist, album, disc_number, track_number,
                      duration_seconds, relative_path, path, mime_type, file_size,
                      artwork_cache_key, artwork_source, artwork_mime_type,
                      musicbrainz_release_id, musicbrainz_release_group_id,
                      musicbrainz_recording_id, musicbrainz_release_track_id,
                      release_date, original_release_date, release_country,
                      release_type, genres_json)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .map_err(db_error)?;

            for track in library.tracks.iter() {
                let artwork_cache_key = track
                    .artwork
                    .as_ref()
                    .map(|artwork| artwork.cache_key.clone());
                let artwork_source = track.artwork.as_ref().map(|artwork| artwork.source.clone());
                let artwork_mime_type = track
                    .artwork
                    .as_ref()
                    .map(|artwork| artwork.mime_type.clone());
                let genres_json = genres_json(&track.metadata.genres);
                statement
                    .execute(params![
                        track.id,
                        track.album_id,
                        track.title,
                        track.artist,
                        track.album,
                        track.disc_number,
                        track.track_number,
                        track.duration_seconds,
                        track.relative_path,
                        track.path.display().to_string(),
                        track.mime_type,
                        track.file_size,
                        artwork_cache_key,
                        artwork_source,
                        artwork_mime_type,
                        track.metadata.musicbrainz_release_id,
                        track.metadata.musicbrainz_release_group_id,
                        track.metadata.musicbrainz_recording_id,
                        track.metadata.musicbrainz_release_track_id,
                        track.metadata.release_date,
                        track.metadata.original_release_date,
                        track.metadata.release_country,
                        track.metadata.release_type,
                        genres_json
                    ])
                    .map_err(db_error)?;
            }
        }
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO albums
                     (id, artist_id, title, artist_name, track_count, artwork_track_id,
                      artwork_cache_key, artwork_source, artwork_mime_type, first_track_id,
                      musicbrainz_release_id, musicbrainz_release_group_id,
                      release_date, original_release_date, release_country, release_type,
                      genres_json, metadata_source_track_id)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .map_err(db_error)?;

            for album in &albums {
                let artwork_cache_key = album
                    .artwork
                    .as_ref()
                    .map(|artwork| artwork.cache_key.clone());
                let artwork_source = album.artwork.as_ref().map(|artwork| artwork.source.clone());
                let artwork_mime_type = album
                    .artwork
                    .as_ref()
                    .map(|artwork| artwork.mime_type.clone());
                let genres_json = genres_json(&album.metadata.genres);
                statement
                    .execute(params![
                        album.id,
                        album.artist_id,
                        album.title,
                        album.artist,
                        album.track_count,
                        album.artwork_track_id,
                        artwork_cache_key,
                        artwork_source,
                        artwork_mime_type,
                        album.first_track_id,
                        album.metadata.musicbrainz_release_id,
                        album.metadata.musicbrainz_release_group_id,
                        album.metadata.release_date,
                        album.metadata.original_release_date,
                        album.metadata.release_country,
                        album.metadata.release_type,
                        genres_json,
                        album.metadata.source_track_id,
                    ])
                    .map_err(db_error)?;
            }
        }
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO artists
                     (id, name, album_count, track_count, artwork_track_id, first_album_id)
                     VALUES (?, ?, ?, ?, ?, ?)",
                )
                .map_err(db_error)?;

            for artist in &artists {
                statement
                    .execute(params![
                        artist.id,
                        artist.name,
                        artist.album_count,
                        artist.track_count,
                        artist.artwork_track_id,
                        artist.first_album_id,
                    ])
                    .map_err(db_error)?;
            }
        }
        transaction.commit().map_err(db_error)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn load_albums(&self) -> io::Result<Vec<AlbumSummary>> {
        let connection = self.connection()?;
        load_albums_from_connection(&connection)
    }

    #[cfg(test)]
    pub(crate) fn load_artists(&self) -> io::Result<Vec<ArtistSummary>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, album_count, track_count, artwork_track_id, first_album_id
                 FROM artists
                 ORDER BY name ASC, id ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                let artwork_track_id = row.get::<_, Option<String>>(4)?;
                Ok(ArtistSummary {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    album_count: row.get(2)?,
                    track_count: row.get(3)?,
                    artwork_track_id: artwork_track_id.clone(),
                    artwork_url: artwork_track_id
                        .map(|track_id| format!("/artwork/track/{track_id}")),
                    first_album_id: row.get(5)?,
                })
            })
            .map_err(db_error)?;

        let mut artists = Vec::new();
        for row in rows {
            artists.push(row.map_err(db_error)?);
        }
        Ok(artists)
    }

    pub(crate) fn list_album_artwork_overrides(&self) -> io::Result<Vec<AlbumArtworkOverride>> {
        let connection = self.connection()?;
        Self::list_album_artwork_overrides_with(&connection)
    }

    pub(super) fn list_album_artwork_overrides_with(
        connection: &Connection,
    ) -> io::Result<Vec<AlbumArtworkOverride>> {
        let mut statement = connection
            .prepare(
                "SELECT album_id, cache_key, source, mime_type, musicbrainz_release_id, applied_unix
                 FROM album_artwork_overrides",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(AlbumArtworkOverride {
                    album_id: row.get(0)?,
                    cache_key: row.get(1)?,
                    source: row.get(2)?,
                    mime_type: row.get(3)?,
                    musicbrainz_release_id: row.get(4)?,
                    applied_unix: row.get(5)?,
                })
            })
            .map_err(db_error)?;

        let mut overrides = Vec::new();
        for row in rows {
            overrides.push(row.map_err(db_error)?);
        }
        Ok(overrides)
    }

    pub(crate) fn upsert_album_artwork_override(
        &self,
        override_record: &AlbumArtworkOverride,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO album_artwork_overrides
                 (album_id, cache_key, source, mime_type, musicbrainz_release_id, applied_unix)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(album_id) DO UPDATE SET
                   cache_key = excluded.cache_key,
                   source = excluded.source,
                   mime_type = excluded.mime_type,
                   musicbrainz_release_id = excluded.musicbrainz_release_id,
                   applied_unix = excluded.applied_unix",
                params![
                    override_record.album_id,
                    override_record.cache_key,
                    override_record.source,
                    override_record.mime_type,
                    override_record.musicbrainz_release_id,
                    override_record.applied_unix,
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }
}

pub(super) fn load_tracks_from_connection(
    connection: &Connection,
) -> io::Result<Vec<LibraryTrack>> {
    let mut statement = connection
        .prepare(
            "SELECT id, album_id, title, artist, album, disc_number, track_number,
                    duration_seconds, relative_path, path, mime_type, file_size,
                    artwork_cache_key, artwork_source, artwork_mime_type,
                    musicbrainz_release_id, musicbrainz_release_group_id,
                    musicbrainz_recording_id, musicbrainz_release_track_id,
                    release_date, original_release_date, release_country,
                    release_type, genres_json
             FROM tracks
             ORDER BY artist, album, COALESCE(disc_number, 0), COALESCE(track_number, 0), title, relative_path",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            let artist: String = row.get(3)?;
            let album: String = row.get(4)?;
            Ok(LibraryTrack {
                id: row.get(0)?,
                album_id: row
                    .get::<_, Option<String>>(1)?
                    .unwrap_or_else(|| stable_album_id(&artist, &album)),
                title: row.get(2)?,
                artist,
                album,
                disc_number: row.get(5)?,
                track_number: row.get(6)?,
                duration_seconds: row.get(7)?,
                relative_path: row.get(8)?,
                path: PathBuf::from(row.get::<_, String>(9)?),
                mime_type: row.get(10)?,
                file_size: row.get(11)?,
                artwork: match (
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                ) {
                    (Some(cache_key), Some(source), Some(mime_type)) => Some(TrackArtwork {
                        cache_key,
                        source,
                        mime_type,
                    }),
                    _ => None,
                },
                metadata: TrackMetadata {
                    musicbrainz_release_id: row.get(15)?,
                    musicbrainz_release_group_id: row.get(16)?,
                    musicbrainz_recording_id: row.get(17)?,
                    musicbrainz_release_track_id: row.get(18)?,
                    release_date: row.get(19)?,
                    original_release_date: row.get(20)?,
                    release_country: row.get(21)?,
                    release_type: row.get(22)?,
                    genres: parse_genres_json(row.get(23)?),
                },
            })
        })
        .map_err(db_error)?;

    let mut tracks = Vec::new();
    for row in rows {
        tracks.push(row.map_err(db_error)?);
    }
    Ok(tracks)
}

#[cfg(test)]
fn load_albums_from_connection(connection: &Connection) -> io::Result<Vec<AlbumSummary>> {
    let mut statement = connection
        .prepare(
            "SELECT id, artist_id, title, artist_name, track_count, artwork_track_id,
                    artwork_cache_key, artwork_source, artwork_mime_type, first_track_id,
                    musicbrainz_release_id, musicbrainz_release_group_id,
                    release_date, original_release_date, release_country, release_type,
                    genres_json, metadata_source_track_id
             FROM albums
             ORDER BY artist_name ASC, title ASC, id ASC",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| {
            let artwork_track_id = row.get::<_, Option<String>>(5)?;
            let artwork = match (
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
            ) {
                (Some(cache_key), Some(source), Some(mime_type)) => Some(TrackArtwork {
                    cache_key,
                    source,
                    mime_type,
                }),
                _ => None,
            };
            Ok(AlbumSummary {
                id: row.get(0)?,
                artist_id: row.get(1)?,
                title: row.get(2)?,
                artist: row.get(3)?,
                track_count: row.get(4)?,
                artwork_track_id: artwork_track_id.clone(),
                artwork: artwork.clone(),
                artwork_url: if artwork.is_some() {
                    Some(format!("/artwork/album/{}", row.get::<_, String>(0)?))
                } else {
                    None
                },
                first_track_id: row.get(9)?,
                metadata: AlbumMetadata {
                    musicbrainz_release_id: row.get(10)?,
                    musicbrainz_release_group_id: row.get(11)?,
                    release_date: row.get(12)?,
                    original_release_date: row.get(13)?,
                    release_country: row.get(14)?,
                    release_type: row.get(15)?,
                    genres: parse_genres_json(row.get(16)?),
                    source_track_id: row.get(17)?,
                },
            })
        })
        .map_err(db_error)?;

    let mut albums = Vec::new();
    for row in rows {
        albums.push(row.map_err(db_error)?);
    }
    Ok(albums)
}

pub(super) fn rebuild_normalized_library_tables(connection: &Connection) -> io::Result<()> {
    let tracks = load_tracks_from_connection(connection)?;
    let albums = build_album_summaries(&tracks);
    let artists = build_artist_summaries_from_albums(&tracks, &albums);
    let transaction = connection.unchecked_transaction().map_err(db_error)?;
    transaction
        .execute("DELETE FROM albums", [])
        .map_err(db_error)?;
    transaction
        .execute("DELETE FROM artists", [])
        .map_err(db_error)?;

    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO albums
                 (id, artist_id, title, artist_name, track_count, artwork_track_id,
                  artwork_cache_key, artwork_source, artwork_mime_type, first_track_id,
                  musicbrainz_release_id, musicbrainz_release_group_id,
                  release_date, original_release_date, release_country, release_type,
                  genres_json, metadata_source_track_id)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .map_err(db_error)?;
        for album in &albums {
            let artwork_cache_key = album
                .artwork
                .as_ref()
                .map(|artwork| artwork.cache_key.clone());
            let artwork_source = album.artwork.as_ref().map(|artwork| artwork.source.clone());
            let artwork_mime_type = album
                .artwork
                .as_ref()
                .map(|artwork| artwork.mime_type.clone());
            let genres_json = genres_json(&album.metadata.genres);
            statement
                .execute(params![
                    album.id,
                    album.artist_id,
                    album.title,
                    album.artist,
                    album.track_count,
                    album.artwork_track_id,
                    artwork_cache_key,
                    artwork_source,
                    artwork_mime_type,
                    album.first_track_id,
                    album.metadata.musicbrainz_release_id,
                    album.metadata.musicbrainz_release_group_id,
                    album.metadata.release_date,
                    album.metadata.original_release_date,
                    album.metadata.release_country,
                    album.metadata.release_type,
                    genres_json,
                    album.metadata.source_track_id,
                ])
                .map_err(db_error)?;
        }
    }

    {
        let mut statement = transaction
            .prepare(
                "INSERT INTO artists
                 (id, name, album_count, track_count, artwork_track_id, first_album_id)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .map_err(db_error)?;
        for artist in &artists {
            statement
                .execute(params![
                    artist.id,
                    artist.name,
                    artist.album_count,
                    artist.track_count,
                    artist.artwork_track_id,
                    artist.first_album_id,
                ])
                .map_err(db_error)?;
        }
    }

    transaction.commit().map_err(db_error)?;
    Ok(())
}
