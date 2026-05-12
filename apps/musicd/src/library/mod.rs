use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::types::{AlbumArtworkOverride, AlbumSummary, ArtistSummary, LibraryTrack};

mod metadata;
mod scan;
mod sort;
mod summaries;

pub(crate) use metadata::inspect_embedded_metadata;
#[cfg(test)]
pub(crate) use metadata::{decode_id3v1_text, parse_vorbis_comment_block};
pub(crate) use scan::scan_library;
pub(crate) use scan::{ScanProgressEvent, scan_library_with_progress};
pub(crate) use sort::compare_track_album_order;
#[cfg(test)]
pub(crate) use summaries::build_artist_summaries;
pub(crate) use summaries::{build_album_summaries, build_artist_summaries_from_albums};

use summaries::{apply_album_artwork_overrides, hydrate_artist_artwork_urls};

#[derive(Debug)]
pub(crate) struct Library {
    pub(crate) scan_root: PathBuf,
    pub(crate) tracks: Arc<[LibraryTrack]>,
    pub(crate) albums: Arc<[AlbumSummary]>,
    pub(crate) artists: Arc<[ArtistSummary]>,
    pub(crate) track_index: HashMap<String, usize>,
    pub(crate) album_index: HashMap<String, usize>,
    pub(crate) artist_index: HashMap<String, usize>,
    pub(crate) tracks_by_album: HashMap<String, Vec<usize>>,
    pub(crate) albums_by_artist: HashMap<String, Vec<usize>>,
}

impl Library {
    pub(crate) fn build(
        scan_root: PathBuf,
        tracks: Vec<LibraryTrack>,
        overrides: &[AlbumArtworkOverride],
    ) -> Self {
        let mut albums = build_album_summaries(&tracks);
        apply_album_artwork_overrides(&mut albums, overrides);
        let mut artists = build_artist_summaries_from_albums(&tracks, &albums);
        hydrate_artist_artwork_urls(&mut artists, &albums);

        let mut track_index = HashMap::with_capacity(tracks.len());
        let mut tracks_by_album: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, track) in tracks.iter().enumerate() {
            track_index.insert(track.id.clone(), idx);
            tracks_by_album
                .entry(track.album_id.clone())
                .or_default()
                .push(idx);
        }
        for indexes in tracks_by_album.values_mut() {
            indexes.sort_by(|&a, &b| compare_track_album_order(&tracks[a], &tracks[b]));
        }

        let mut album_index = HashMap::with_capacity(albums.len());
        let mut albums_by_artist: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, album) in albums.iter().enumerate() {
            album_index.insert(album.id.clone(), idx);
            albums_by_artist
                .entry(album.artist_id.clone())
                .or_default()
                .push(idx);
        }

        let mut artist_index = HashMap::with_capacity(artists.len());
        for (idx, artist) in artists.iter().enumerate() {
            artist_index.insert(artist.id.clone(), idx);
        }

        Self {
            scan_root,
            tracks: Arc::from(tracks),
            albums: Arc::from(albums),
            artists: Arc::from(artists),
            track_index,
            album_index,
            artist_index,
            tracks_by_album,
            albums_by_artist,
        }
    }
}
