use std::io;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use musicd_upnp::TransportSnapshot;

use crate::renderer::{RendererKind, renderer_kind_for_location};
use crate::types::{PlaybackQueue, PlaybackSession, QueueEntry};

use super::ServiceState;
use super::events::fingerprint;

const ACTIVE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(15);

pub(crate) fn spawn_queue_worker(state: Arc<ServiceState>) {
    thread::spawn(move || {
        loop {
            if let Err(error) = state.poll_active_queues() {
                eprintln!("queue worker error: {error}");
            }
            let interval = if state.events.any_subscribers() {
                ACTIVE_POLL_INTERVAL
            } else {
                IDLE_POLL_INTERVAL
            };
            thread::sleep(interval);
        }
    });
}

pub(crate) fn queue_status_for_transport(transport_state: &str) -> &'static str {
    match transport_state {
        "PLAYING" | "TRANSITIONING" => "playing",
        "PAUSED_PLAYBACK" => "paused",
        "STOPPED" | "NO_MEDIA_PRESENT" => "stopped",
        "READY" => "ready",
        "COMPLETED" => "completed",
        "ERROR" => "error",
        _ => "ready",
    }
}

pub(crate) fn next_queue_entry_after(
    queue: &PlaybackQueue,
    current_entry_id: i64,
) -> Option<&QueueEntry> {
    let current_position = queue
        .entries
        .iter()
        .find(|entry| entry.id == current_entry_id)
        .map(|entry| entry.position)?;

    queue
        .entries
        .iter()
        .find(|entry| entry.position > current_position)
}

pub(crate) fn previous_queue_entry_before(
    queue: &PlaybackQueue,
    current_entry_id: i64,
) -> Option<&QueueEntry> {
    let current_position = queue
        .entries
        .iter()
        .find(|entry| entry.id == current_entry_id)
        .map(|entry| entry.position)?;

    queue
        .entries
        .iter()
        .rev()
        .find(|entry| entry.position < current_position)
}

pub(crate) fn should_adopt_preloaded_next_entry(
    queue: &PlaybackQueue,
    snapshot: &TransportSnapshot,
    expected_next_track_uri: Option<&str>,
) -> bool {
    let Some(current_entry_id) = queue.current_entry_id else {
        return false;
    };
    if next_queue_entry_after(queue, current_entry_id).is_none() {
        return false;
    }
    let Some(track_uri) = snapshot.position_info.track_uri.as_deref() else {
        return false;
    };
    expected_next_track_uri.is_some_and(|expected_uri| track_uri == expected_uri)
}

pub(crate) fn should_auto_advance(
    queue: &PlaybackQueue,
    session: Option<&PlaybackSession>,
    snapshot: &TransportSnapshot,
    state: &ServiceState,
) -> bool {
    if !matches!(
        snapshot.transport_info.transport_state.as_str(),
        "STOPPED" | "NO_MEDIA_PRESENT"
    ) {
        return false;
    }

    let session = match session {
        Some(session) => session,
        None => return false,
    };
    if session.transport_state == "PAUSED_PLAYBACK" {
        return false;
    }
    let current_entry_id = match queue.current_entry_id {
        Some(current_entry_id) => current_entry_id,
        None => return false,
    };
    let current_entry = match queue
        .entries
        .iter()
        .find(|entry| entry.id == current_entry_id)
    {
        Some(current_entry) => current_entry,
        None => return false,
    };
    if current_entry.entry_status != "playing" {
        return false;
    }

    let track = match state.find_track(&current_entry.track_id) {
        Some(track) => track,
        None => return false,
    };
    let expected_duration = snapshot
        .position_info
        .track_duration_seconds
        .or(track.duration_seconds)
        .or(session.duration_seconds);
    let observed_position = session
        .position_seconds
        .into_iter()
        .chain(snapshot.position_info.rel_time_seconds)
        .max()
        .unwrap_or(0);

    expected_duration
        .map(|duration| observed_position.saturating_add(2) >= duration)
        .unwrap_or(false)
}

impl ServiceState {
    pub(crate) fn poll_active_queues(&self) -> io::Result<()> {
        for renderer_location in self.database.list_playing_queue_renderers()? {
            if matches!(
                renderer_kind_for_location(&renderer_location),
                RendererKind::Group
            ) {
                self.debug_log("group-queue-poll", format!("renderer={renderer_location}"));
                if let Err(error) = self.poll_renderer_group_queue(&renderer_location) {
                    eprintln!("group queue poll failed for {renderer_location}: {error}");
                }
                continue;
            }
            if matches!(
                renderer_kind_for_location(&renderer_location),
                RendererKind::AndroidLocal
            ) {
                continue;
            }
            self.debug_log("queue-poll", format!("renderer={renderer_location}"));
            if let Err(error) = self.poll_renderer_queue(&renderer_location) {
                eprintln!("queue poll failed for {renderer_location}: {error}");
            }
        }
        Ok(())
    }

    pub(crate) fn poll_renderer_queue(&self, renderer_location: &str) -> io::Result<()> {
        let mut queue = match self.queue_snapshot(renderer_location) {
            Some(queue) => queue,
            None => return Ok(()),
        };
        let session = self.playback_session(renderer_location);
        let previous_queue_status = queue.status.clone();
        let renderer = self.resolve_renderer(renderer_location)?;
        let snapshot = match self
            .renderer_backend(renderer_location)?
            .transport_snapshot(&renderer)
        {
            Ok(snapshot) => {
                let _ = self.mark_renderer_reachable(&renderer);
                snapshot
            }
            Err(error) => {
                let _ = self.mark_renderer_unreachable(renderer_location, &error);
                let _ = self
                    .database
                    .record_transport_poll_error(renderer_location, &error.to_string());
                return Err(error);
            }
        };

        let snapshot_fingerprint = fingerprint(&queue, &snapshot);
        let state_changed = self
            .events
            .note_state(renderer_location, snapshot_fingerprint);
        if state_changed {
            self.database.record_transport_snapshot(
                renderer_location,
                &snapshot.transport_info.transport_state,
                snapshot.position_info.track_uri.as_deref(),
                snapshot.position_info.rel_time_seconds,
                snapshot.position_info.track_duration_seconds,
            )?;
            self.database.sync_queue_status(
                renderer_location,
                queue_status_for_transport(&snapshot.transport_info.transport_state),
            )?;
        }
        if self.debug_enabled() {
            let previous_state = session
                .as_ref()
                .map(|session| session.transport_state.as_str())
                .unwrap_or("<none>");
            if previous_state != snapshot.transport_info.transport_state
                || previous_queue_status
                    != queue_status_for_transport(&snapshot.transport_info.transport_state)
            {
                self.debug_log(
                    "transport-transition",
                    format!(
                        "renderer={} session_state={} -> {} queue_status={} -> {} position={:?} duration={:?}",
                        renderer_location,
                        previous_state,
                        snapshot.transport_info.transport_state,
                        previous_queue_status,
                        queue_status_for_transport(&snapshot.transport_info.transport_state),
                        snapshot.position_info.rel_time_seconds,
                        snapshot.position_info.track_duration_seconds
                    ),
                );
            }
        }

        if self.adopt_renderer_advanced_entry(renderer_location, &queue, &snapshot)? {
            if let Some(updated_queue) = self.queue_snapshot(renderer_location) {
                queue = updated_queue;
            }
        }

        if let Some(current_entry_id) = queue.current_entry_id.filter(|_| {
            matches!(
                snapshot.transport_info.transport_state.as_str(),
                "PLAYING" | "TRANSITIONING"
            )
        }) {
            if let Err(error) = self.preload_next_queue_entry(
                renderer_location,
                &renderer,
                &queue,
                current_entry_id,
            ) {
                eprintln!("next-track preload refresh failed for {renderer_location}: {error}");
            }
        }

        if matches!(
            snapshot.transport_info.transport_state.as_str(),
            "STOPPED" | "NO_MEDIA_PRESENT"
        ) && matches!(
            session
                .as_ref()
                .map(|session| session.transport_state.as_str()),
            Some("PAUSED_PLAYBACK")
        ) {
            self.debug_log(
                "auto-advance-suppressed",
                format!(
                    "renderer={} stopped_after_pause position={:?} duration={:?}",
                    renderer_location,
                    snapshot.position_info.rel_time_seconds,
                    snapshot.position_info.track_duration_seconds
                ),
            );
        }

        if should_auto_advance(&queue, session.as_ref(), &snapshot, self) {
            self.debug_log(
                "auto-advance",
                format!(
                    "renderer={} current_entry={:?} position={:?} duration={:?}",
                    renderer_location,
                    queue.current_entry_id,
                    snapshot.position_info.rel_time_seconds,
                    snapshot.position_info.track_duration_seconds
                ),
            );
            let next_entry_id = self
                .database
                .advance_queue_after_completion(renderer_location)?;
            if next_entry_id.is_some() {
                let _ = self.start_current_queue_entry(renderer_location)?;
            }
            self.events.touch(renderer_location);
        }

        Ok(())
    }
}
