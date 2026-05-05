use std::fs;
use std::io;
use std::path::PathBuf;

use musicd_upnp::StreamResource;

use crate::artwork::{
    artwork_cache_path, download_artwork_candidate, fetch_musicbrainz_cover_art_for_release,
    image_extension_for_mime, musicbrainz_client, search_musicbrainz_album_artwork,
};
use crate::ids::stable_track_id;
use crate::types::{AlbumArtworkOverride, AlbumArtworkSearchCandidate, AlbumSummary, LibraryTrack};
use crate::util::now_unix_timestamp;

use super::ServiceState;

impl ServiceState {
    pub(crate) fn track_artwork_path(&self, track: &LibraryTrack) -> Option<PathBuf> {
        track
            .artwork
            .as_ref()
            .map(|artwork| artwork_cache_path(&self.config.config_path, &artwork.cache_key))
    }

    pub(crate) fn album_artwork_path(&self, album_id: &str) -> Option<PathBuf> {
        self.find_album(album_id).and_then(|album| {
            album
                .artwork
                .as_ref()
                .map(|artwork| artwork_cache_path(&self.config.config_path, &artwork.cache_key))
        })
    }

    pub(crate) fn artwork_url_for_track(&self, track: &LibraryTrack) -> Option<String> {
        self.relative_artwork_url_for_track(track)
            .map(|artwork_url| {
                format!(
                    "{}/{}",
                    self.config.resolved_base_url().trim_end_matches('/'),
                    artwork_url.trim_start_matches('/')
                )
            })
    }

    pub(crate) fn relative_artwork_url_for_track(&self, track: &LibraryTrack) -> Option<String> {
        if track.artwork.is_some() {
            return Some(format!("/artwork/track/{}", track.id));
        }

        self.find_album(&track.album_id)
            .filter(|album| album.artwork.is_some())
            .map(|_| format!("/artwork/album/{}", track.album_id))
    }

    pub(crate) fn search_album_artwork_candidates(
        &self,
        album_id: &str,
    ) -> io::Result<Vec<AlbumArtworkSearchCandidate>> {
        let album = self
            .find_album(album_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "album not found"))?;
        let client = musicbrainz_client()?;
        search_musicbrainz_album_artwork(&client, &album.artist, &album.title)
    }

    pub(crate) fn apply_album_artwork_candidate(
        &self,
        album_id: &str,
        release_id: &str,
    ) -> io::Result<AlbumSummary> {
        let album = self
            .find_album(album_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "album not found"))?;
        let client = musicbrainz_client()?;
        let candidate = fetch_musicbrainz_cover_art_for_release(&client, release_id)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no front artwork found"))?;
        let downloaded = download_artwork_candidate(&client, &candidate.image_url)?;
        let cache_key = stable_track_id(&format!("mb-release:{}:{}", album.id, release_id));
        let extension = image_extension_for_mime(&downloaded.mime_type).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported remote artwork MIME type",
            )
        })?;
        let destination = self
            .config
            .config_path
            .join("artwork")
            .join(format!("{cache_key}.{extension}"));
        fs::write(&destination, downloaded.bytes)?;

        let override_record = AlbumArtworkOverride {
            album_id: album.id.clone(),
            cache_key: format!("{cache_key}.{extension}"),
            source: candidate.source,
            mime_type: downloaded.mime_type,
            musicbrainz_release_id: Some(release_id.to_string()),
            applied_unix: now_unix_timestamp(),
        };
        self.database
            .upsert_album_artwork_override(&override_record)?;
        self.refresh_album_artwork_overrides()?;
        self.find_album(album_id)
            .ok_or_else(|| io::Error::other("updated album could not be reloaded"))
    }

    pub(crate) fn stream_resource_for_track(&self, track: &LibraryTrack) -> StreamResource {
        StreamResource {
            stream_url: format!(
                "{}/stream/track/{}",
                self.config.resolved_base_url().trim_end_matches('/'),
                track.id
            ),
            mime_type: track.mime_type.clone(),
            title: track.title.clone(),
            album_art_url: self.artwork_url_for_track(track),
        }
    }
}
