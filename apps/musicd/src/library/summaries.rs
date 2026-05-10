use std::collections::HashMap;

use crate::ids::stable_artist_id;
use crate::types::{
    AlbumArtworkOverride, AlbumMetadata, AlbumSummary, ArtistSummary, LibraryTrack, TrackArtwork,
};

use super::sort::{compare_albums, compare_artists, compare_track_album_order};

pub(crate) fn build_album_summaries(tracks: &[LibraryTrack]) -> Vec<AlbumSummary> {
    let mut grouped = HashMap::<String, Vec<LibraryTrack>>::new();
    for track in tracks {
        grouped
            .entry(track.album_id.clone())
            .or_default()
            .push(track.clone());
    }

    let mut albums = grouped
        .into_iter()
        .filter_map(|(album_id, mut album_tracks)| {
            album_tracks.sort_by(compare_track_album_order);
            let first_track = album_tracks.first()?.clone();
            let album_artwork_url = format!("/artwork/album/{album_id}");
            let artwork_track_id = album_tracks
                .iter()
                .find(|track| track.artwork.is_some())
                .map(|track| track.id.clone());
            let artwork = album_tracks.iter().find_map(|track| track.artwork.clone());
            Some(AlbumSummary {
                id: album_id,
                artist_id: stable_artist_id(&first_track.album_artist),
                title: first_track.album.clone(),
                artist: first_track.album_artist.clone(),
                track_count: album_tracks.len(),
                artwork_track_id: artwork_track_id.clone(),
                artwork: artwork.clone(),
                artwork_url: artwork.map(|_| album_artwork_url),
                first_track_id: first_track.id.clone(),
                metadata: album_metadata_from_tracks(&album_tracks),
            })
        })
        .collect::<Vec<_>>();

    albums.sort_by(compare_albums);
    albums
}

fn album_metadata_from_tracks(tracks: &[LibraryTrack]) -> AlbumMetadata {
    let source_track = tracks.iter().find(|track| {
        track.metadata.musicbrainz_release_id.is_some()
            || track.metadata.musicbrainz_release_group_id.is_some()
            || track.metadata.release_date.is_some()
            || track.metadata.original_release_date.is_some()
            || track.metadata.release_country.is_some()
            || track.metadata.release_type.is_some()
            || !track.metadata.genres.is_empty()
    });
    let mut genres = Vec::new();
    for track in tracks {
        for genre in &track.metadata.genres {
            if !genres.iter().any(|existing| existing == genre) {
                genres.push(genre.clone());
            }
        }
    }
    AlbumMetadata {
        musicbrainz_release_id: source_track
            .and_then(|track| track.metadata.musicbrainz_release_id.clone()),
        musicbrainz_release_group_id: source_track
            .and_then(|track| track.metadata.musicbrainz_release_group_id.clone()),
        release_date: source_track.and_then(|track| track.metadata.release_date.clone()),
        original_release_date: source_track
            .and_then(|track| track.metadata.original_release_date.clone()),
        release_country: source_track.and_then(|track| track.metadata.release_country.clone()),
        release_type: source_track.and_then(|track| track.metadata.release_type.clone()),
        genres,
        source_track_id: source_track.map(|track| track.id.clone()),
    }
}

pub(crate) fn apply_album_artwork_overrides(
    albums: &mut [AlbumSummary],
    overrides: &[AlbumArtworkOverride],
) {
    let override_records = overrides
        .iter()
        .map(|override_record| {
            (
                override_record.album_id.clone(),
                (
                    TrackArtwork {
                        cache_key: override_record.cache_key.clone(),
                        source: override_record.source.clone(),
                        mime_type: override_record.mime_type.clone(),
                    },
                    format!("/artwork/album/{}", override_record.album_id),
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    for album in albums {
        if let Some((override_artwork, override_url)) = override_records.get(&album.id) {
            album.artwork = Some(override_artwork.clone());
            album.artwork_url = Some(override_url.clone());
        }
    }
}

pub(crate) fn hydrate_artist_artwork_urls(artists: &mut [ArtistSummary], albums: &[AlbumSummary]) {
    let artwork_by_artist = albums
        .iter()
        .filter_map(|album| {
            album
                .artwork_url
                .as_ref()
                .map(|artwork_url| (album.artist_id.clone(), artwork_url.clone()))
        })
        .collect::<HashMap<_, _>>();

    for artist in artists {
        if let Some(artwork_url) = artwork_by_artist.get(&artist.id) {
            artist.artwork_url = Some(artwork_url.clone());
        }
    }
}

pub(crate) fn build_artist_summaries_from_albums(
    tracks: &[LibraryTrack],
    albums: &[AlbumSummary],
) -> Vec<ArtistSummary> {
    let mut track_counts = HashMap::<String, usize>::new();
    for track in tracks {
        *track_counts
            .entry(stable_artist_id(&track.album_artist))
            .or_default() += 1;
    }

    let mut grouped = HashMap::<String, Vec<AlbumSummary>>::new();
    for album in albums {
        grouped
            .entry(stable_artist_id(&album.artist))
            .or_default()
            .push(album.clone());
    }

    let mut artists = grouped
        .into_iter()
        .filter_map(|(artist_id, mut artist_albums)| {
            artist_albums.sort_by(compare_albums);
            let first_album = artist_albums.first()?.clone();
            let artwork_track_id = artist_albums
                .iter()
                .find_map(|album| album.artwork_track_id.clone());
            let artwork_url = artist_albums
                .iter()
                .find_map(|album| album.artwork_url.clone());
            Some(ArtistSummary {
                id: artist_id.clone(),
                name: first_album.artist.clone(),
                album_count: artist_albums.len(),
                track_count: track_counts.get(&artist_id).copied().unwrap_or(0),
                artwork_track_id,
                artwork_url,
                first_album_id: first_album.id,
            })
        })
        .collect::<Vec<_>>();

    artists.sort_by(compare_artists);
    artists
}

#[allow(dead_code)]
pub(crate) fn build_artist_summaries(tracks: &[LibraryTrack]) -> Vec<ArtistSummary> {
    let albums = build_album_summaries(tracks);
    build_artist_summaries_from_albums(tracks, &albums)
}
