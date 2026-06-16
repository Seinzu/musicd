use std::io;

use crate::renderer::{RendererKind, renderer_kind_for_location};
use crate::types::{AlbumSummary, LibraryTrack, PlaybackQueue, QueueMutationEntry, RendererGroup};

use super::ServiceState;

impl ServiceState {
    pub(crate) fn replace_queue_with_track(
        &self,
        renderer_location: &str,
        track: &LibraryTrack,
    ) -> io::Result<PlaybackQueue> {
        let group = self.queue_target_group(renderer_location)?;
        let queue = self.database.replace_queue(
            renderer_location,
            &format!("Track: {}", track.title),
            &[QueueMutationEntry {
                track_id: track.id.clone(),
                album_id: Some(track.album_id.clone()),
                source_kind: "track".to_string(),
                source_ref: Some(track.id.clone()),
            }],
        )?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
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
        let group = self.queue_target_group(renderer_location)?;
        let queue = self
            .database
            .replace_queue(renderer_location, &album.title, &entries)?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(crate) fn append_track_to_queue(
        &self,
        renderer_location: &str,
        track: &LibraryTrack,
    ) -> io::Result<PlaybackQueue> {
        let group = self.queue_target_group(renderer_location)?;
        let queue = self.database.append_queue_entries(
            renderer_location,
            &format!("Track: {}", track.title),
            &[QueueMutationEntry {
                track_id: track.id.clone(),
                album_id: Some(track.album_id.clone()),
                source_kind: "track".to_string(),
                source_ref: Some(track.id.clone()),
            }],
        )?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
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
        let group = self.queue_target_group(renderer_location)?;
        let queue =
            self.database
                .append_queue_entries(renderer_location, &album.title, &entries)?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(crate) fn play_next_track(
        &self,
        renderer_location: &str,
        track: &LibraryTrack,
    ) -> io::Result<PlaybackQueue> {
        let group = self.queue_target_group(renderer_location)?;
        let queue = self.database.insert_queue_entries_after_current(
            renderer_location,
            &format!("Track: {}", track.title),
            &[QueueMutationEntry {
                track_id: track.id.clone(),
                album_id: Some(track.album_id.clone()),
                source_kind: "track".to_string(),
                source_ref: Some(track.id.clone()),
            }],
        )?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
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
        let group = self.queue_target_group(renderer_location)?;
        let queue = self.database.insert_queue_entries_after_current(
            renderer_location,
            &album.title,
            &entries,
        )?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(crate) fn clear_queue(&self, renderer_location: &str) -> io::Result<()> {
        let group = self.queue_target_group(renderer_location)?;
        if let Some(group) = group.as_ref() {
            if let Err(error) = self.fan_out_group_transport_action(
                group,
                "clear",
                |state, member_location, renderer| {
                    state.run_renderer_action_with_private_queue_log(
                        member_location,
                        renderer,
                        "group-clear-stop",
                        |backend| backend.stop(renderer),
                    )?;
                    state.clear_renderer_private_queue(member_location, renderer, "group-clear");
                    Ok(())
                },
            ) {
                eprintln!("group queue clear stop failed for {renderer_location}: {error}");
            }
        } else if let Ok(renderer) = self.resolve_renderer(renderer_location) {
            self.clear_renderer_private_queue(renderer_location, &renderer, "clear-queue");
        }
        self.database.clear_queue(renderer_location)?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), None);
        Ok(())
    }

    pub(crate) fn move_queue_entry_up(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<PlaybackQueue> {
        let group = self.queue_target_group(renderer_location)?;
        let queue = self
            .database
            .move_queue_entry(renderer_location, queue_entry_id, -1)?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(crate) fn move_queue_entry_down(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<PlaybackQueue> {
        let group = self.queue_target_group(renderer_location)?;
        let queue = self
            .database
            .move_queue_entry(renderer_location, queue_entry_id, 1)?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(crate) fn remove_pending_queue_entry(
        &self,
        renderer_location: &str,
        queue_entry_id: i64,
    ) -> io::Result<PlaybackQueue> {
        let group = self.queue_target_group(renderer_location)?;
        let queue = self
            .database
            .remove_queue_entry(renderer_location, queue_entry_id)?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(super) fn queue_target_group(
        &self,
        renderer_location: &str,
    ) -> io::Result<Option<RendererGroup>> {
        if !matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::Group
        ) {
            return Ok(None);
        }
        self.load_renderer_group_for_queue(renderer_location)
            .map(Some)
    }

    pub(super) fn finish_queue_mutation(
        &self,
        renderer_location: &str,
        group: Option<&RendererGroup>,
        queue: Option<&PlaybackQueue>,
    ) {
        if let (Some(group), Some(queue)) = (group, queue) {
            self.refresh_group_queue_preload(renderer_location, group, queue);
        } else if let Some(queue) = queue {
            self.refresh_renderer_queue_preload(renderer_location, queue);
        }
        self.events.touch(renderer_location);
    }

    fn refresh_renderer_queue_preload(&self, renderer_location: &str, queue: &PlaybackQueue) {
        let session = self.playback_session(renderer_location);
        let is_active = session
            .as_ref()
            .map(|session| {
                matches!(
                    session.transport_state.as_str(),
                    "PLAYING" | "TRANSITIONING" | "PAUSED_PLAYBACK"
                )
            })
            .unwrap_or(false);
        if let (true, Some(current_entry_id)) = (is_active, queue.current_entry_id) {
            let renderer = match self.resolve_renderer(renderer_location) {
                Ok(renderer) => renderer,
                Err(error) => {
                    self.debug_log(
                        "playlist-sync-failed",
                        format!(
                            "renderer={} reason=queue-mutation resolve_error={}",
                            renderer_location, error
                        ),
                    );
                    return;
                }
            };
            let playlist_synced = self.sync_renderer_private_queue_from_musicd(
                renderer_location,
                &renderer,
                queue,
                current_entry_id,
                "queue-mutation",
            );
            if !playlist_synced {
                if let Err(error) = self.preload_next_queue_entry(
                    renderer_location,
                    &renderer,
                    queue,
                    current_entry_id,
                    false,
                ) {
                    eprintln!("next-track preload refresh failed for {renderer_location}: {error}");
                }
            }
        } else if session
            .as_ref()
            .and_then(|session| session.next_queue_entry_id)
            .is_some()
        {
            let _ = self
                .database
                .mark_next_queue_entry_preloaded(renderer_location, None);
        }
    }

    fn refresh_group_queue_preload(
        &self,
        renderer_location: &str,
        group: &RendererGroup,
        queue: &PlaybackQueue,
    ) {
        let session = self.playback_session(renderer_location);
        let is_active = session
            .as_ref()
            .map(|session| {
                matches!(
                    session.transport_state.as_str(),
                    "PLAYING" | "TRANSITIONING" | "PAUSED_PLAYBACK"
                )
            })
            .unwrap_or(false);
        if let (true, Some(current_entry_id)) = (is_active, queue.current_entry_id) {
            if let Err(error) = self.preload_next_group_queue_entry(
                renderer_location,
                group,
                queue,
                current_entry_id,
                false,
            ) {
                eprintln!(
                    "group next-track preload refresh failed for {renderer_location}: {error}"
                );
            }
        } else if session
            .as_ref()
            .and_then(|session| session.next_queue_entry_id)
            .is_some()
        {
            let _ = self
                .database
                .mark_next_queue_entry_preloaded(renderer_location, None);
        }
    }
}
