use std::cmp::Ordering;

use crate::types::{AlbumSummary, ArtistSummary, LibraryTrack};

pub(crate) fn compare_library_tracks(left: &LibraryTrack, right: &LibraryTrack) -> Ordering {
    (
        left.artist.as_str(),
        left.album.as_str(),
        numeric_sort_key(left.disc_number),
        numeric_sort_key(left.track_number),
        left.title.as_str(),
        left.relative_path.as_str(),
    )
        .cmp(&(
            right.artist.as_str(),
            right.album.as_str(),
            numeric_sort_key(right.disc_number),
            numeric_sort_key(right.track_number),
            right.title.as_str(),
            right.relative_path.as_str(),
        ))
}

pub(crate) fn compare_track_album_order(left: &LibraryTrack, right: &LibraryTrack) -> Ordering {
    (
        numeric_sort_key(left.disc_number),
        numeric_sort_key(left.track_number),
        left.title.as_str(),
        left.relative_path.as_str(),
    )
        .cmp(&(
            numeric_sort_key(right.disc_number),
            numeric_sort_key(right.track_number),
            right.title.as_str(),
            right.relative_path.as_str(),
        ))
}

pub(crate) fn compare_albums(left: &AlbumSummary, right: &AlbumSummary) -> Ordering {
    (left.artist.as_str(), left.title.as_str(), left.id.as_str()).cmp(&(
        right.artist.as_str(),
        right.title.as_str(),
        right.id.as_str(),
    ))
}

pub(crate) fn compare_artists(left: &ArtistSummary, right: &ArtistSummary) -> Ordering {
    (left.name.as_str(), left.id.as_str()).cmp(&(right.name.as_str(), right.id.as_str()))
}

fn numeric_sort_key(value: Option<u32>) -> (bool, u32) {
    (value.is_none(), value.unwrap_or(u32::MAX))
}
