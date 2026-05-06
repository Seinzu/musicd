use std::io;

use crate::types::{AlbumSummary, LibraryTrack, PlaybackQueue, QueueMutationEntry};

use super::ServiceState;

impl ServiceState {
    pub(crate) fn replace_queue_with_track(
        &self,
        renderer_location: &str,
        track: &LibraryTrack,
    ) -> io::Result<PlaybackQueue> {
        self.database
            .replace_queue(
                renderer_location,
                &format!("Track: {}", track.title),
                &[QueueMutationEntry {
                    track_id: track.id.clone(),
                    album_id: Some(track.album_id.clone()),
                    source_kind: "track".to_string(),
                    source_ref: Some(track.id.clone()),
                }],
            )
            .inspect(|_| self.events.touch(renderer_location))
    }

    pub(crate) fn replace_queue_with_album(
        &self,
        renderer_location: &str,
        album: &AlbumSummary,
    ) -> io::Result<PlaybackQueue> {
        let tracks = self.tracks_for_album(&album.id);
        let entries = tracks
            .into_iter()
            .map(|track| QueueMutationEntry {
                track_id: track.id,
                album_id: Some(album.id.clone()),
                source_kind: "album".to_string(),
                source_ref: Some(album.id.clone()),
            })
            .collect::<Vec<_>>();
        self.database
            .replace_queue(renderer_location, &album.title, &entries)
            .inspect(|_| self.events.touch(renderer_location))
    }

    pub(crate) fn append_track_to_queue(
        &self,
        renderer_location: &str,
        track: &LibraryTrack,
    ) -> io::Result<PlaybackQueue> {
        self.database
            .append_queue_entries(
                renderer_location,
                &format!("Track: {}", track.title),
                &[QueueMutationEntry {
                    track_id: track.id.clone(),
                    album_id: Some(track.album_id.clone()),
                    source_kind: "track".to_string(),
                    source_ref: Some(track.id.clone()),
                }],
            )
            .inspect(|_| self.events.touch(renderer_location))
    }

    pub(crate) fn append_album_to_queue(
        &self,
        renderer_location: &str,
        album: &AlbumSummary,
    ) -> io::Result<PlaybackQueue> {
        let tracks = self.tracks_for_album(&album.id);
        let entries = tracks
            .into_iter()
            .map(|track| QueueMutationEntry {
                track_id: track.id,
                album_id: Some(album.id.clone()),
                source_kind: "album".to_string(),
                source_ref: Some(album.id.clone()),
            })
            .collect::<Vec<_>>();
        self.database
            .append_queue_entries(renderer_location, &album.title, &entries)
            .inspect(|_| self.events.touch(renderer_location))
    }

    pub(crate) fn play_next_track(
        &self,
        renderer_location: &str,
        track: &LibraryTrack,
    ) -> io::Result<PlaybackQueue> {
        self.database
            .insert_queue_entries_after_current(
                renderer_location,
                &format!("Track: {}", track.title),
                &[QueueMutationEntry {
                    track_id: track.id.clone(),
                    album_id: Some(track.album_id.clone()),
                    source_kind: "track".to_string(),
                    source_ref: Some(track.id.clone()),
                }],
            )
            .inspect(|_| self.events.touch(renderer_location))
    }

    pub(crate) fn play_next_album(
        &self,
        renderer_location: &str,
        album: &AlbumSummary,
    ) -> io::Result<PlaybackQueue> {
        let tracks = self.tracks_for_album(&album.id);
        let entries = tracks
            .into_iter()
            .map(|track| QueueMutationEntry {
                track_id: track.id,
                album_id: Some(album.id.clone()),
                source_kind: "album".to_string(),
                source_ref: Some(album.id.clone()),
            })
            .collect::<Vec<_>>();
        self.database
            .insert_queue_entries_after_current(renderer_location, &album.title, &entries)
            .inspect(|_| self.events.touch(renderer_location))
    }

    pub(crate) fn clear_queue(&self, renderer_location: &str) -> io::Result<()> {
        self.database
            .clear_queue(renderer_location)
            .inspect(|_| self.events.touch(renderer_location))
    }

    pub(crate) fn move_queue_entry_up(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<PlaybackQueue> {
        self.database
            .move_queue_entry(renderer_location, queue_entry_id, -1)
            .inspect(|_| self.events.touch(renderer_location))
    }

    pub(crate) fn move_queue_entry_down(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<PlaybackQueue> {
        self.database
            .move_queue_entry(renderer_location, queue_entry_id, 1)
            .inspect(|_| self.events.touch(renderer_location))
    }

    pub(crate) fn remove_pending_queue_entry(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<PlaybackQueue> {
        self.database
            .remove_queue_entry(renderer_location, queue_entry_id)
            .inspect(|_| self.events.touch(renderer_location))
    }
}
