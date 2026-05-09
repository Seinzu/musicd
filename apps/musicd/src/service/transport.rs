use std::io;
use std::thread;
use std::time::Duration;

use musicd_upnp::TransportSnapshot;

use crate::renderer::{RendererKind, renderer_kind_for_location};
use crate::types::{LibraryTrack, PlaybackQueue, RendererRecord};

use super::ServiceState;
use super::poll::{
    next_queue_entry_after, previous_queue_entry_before, queue_status_for_transport,
    should_adopt_preloaded_next_entry,
};

impl ServiceState {
    pub(crate) fn refresh_transport_state(
        &self,
        renderer_location: &str,
    ) -> io::Result<TransportSnapshot> {
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
                return Err(error);
            }
        };
        self.database.record_transport_snapshot(
            renderer_location,
            &snapshot.transport_info.transport_state,
            snapshot.position_info.track_uri.as_deref(),
            snapshot.position_info.rel_time_seconds,
            snapshot.position_info.track_duration_seconds,
        )?;
        Ok(snapshot)
    }

    pub(crate) fn wait_for_transport_state(
        &self,
        renderer_location: &str,
        stable_states: &[&str],
        attempts: usize,
        delay: Duration,
    ) -> io::Result<TransportSnapshot> {
        let mut last_snapshot = self.refresh_transport_state(renderer_location)?;
        self.debug_log(
            "transport-wait",
            format!(
                "renderer={} initial_state={} stable={:?}",
                renderer_location, last_snapshot.transport_info.transport_state, stable_states
            ),
        );
        for _ in 0..attempts {
            if stable_states.contains(&last_snapshot.transport_info.transport_state.as_str()) {
                return Ok(last_snapshot);
            }
            thread::sleep(delay);
            last_snapshot = self.refresh_transport_state(renderer_location)?;
            self.debug_log(
                "transport-wait",
                format!(
                    "renderer={} observed_state={}",
                    renderer_location, last_snapshot.transport_info.transport_state
                ),
            );
        }
        Ok(last_snapshot)
    }

    pub(crate) fn resume_renderer(&self, renderer_location: &str) -> io::Result<String> {
        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::Group
        ) {
            return self.resume_renderer_group(renderer_location);
        }
        let _ = self.remember_renderer_location(renderer_location);
        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::AndroidLocal | RendererKind::CliLocal
        ) {
            if let Some(queue) = self.queue_snapshot(renderer_location) {
                if queue.current_entry_id.is_some() {
                    let session = self.playback_session(renderer_location);
                    if matches!(
                        session
                            .as_ref()
                            .map(|session| session.transport_state.as_str()),
                        Some("PAUSED_PLAYBACK")
                    ) {
                        self.database
                            .set_queue_status(renderer_location, "playing", "PLAYING")?;
                        return Ok("Playback resumed.".to_string());
                    }
                    let (track, _, renderer_name, _) =
                        self.start_current_queue_entry(renderer_location)?;
                    return Ok(format!(
                        "Now playing '{}' on {}.",
                        track.title, renderer_name
                    ));
                }
            }
            return Err(io::Error::new(io::ErrorKind::NotFound, "queue is empty"));
        }
        let queue = self.queue_snapshot(renderer_location);
        let session = self.playback_session(renderer_location);
        self.debug_log(
            "resume-request",
            format!(
                "renderer={} queue_current={:?} session_state={}",
                renderer_location,
                queue.as_ref().and_then(|queue| queue.current_entry_id),
                session
                    .as_ref()
                    .map(|session| session.transport_state.as_str())
                    .unwrap_or("<none>")
            ),
        );

        if let Some(queue) = queue.as_ref() {
            if session
                .as_ref()
                .map(|session| {
                    matches!(
                        session.transport_state.as_str(),
                        "STOPPED" | "NO_MEDIA_PRESENT" | "READY" | "COMPLETED"
                    )
                })
                .unwrap_or(true)
                && queue.current_entry_id.is_some()
            {
                let (track, _, renderer_name, _) =
                    self.start_current_queue_entry(renderer_location)?;
                return Ok(format!(
                    "Now playing '{}' on {}.",
                    track.title, renderer_name
                ));
            }
        }

        let renderer = self.resolve_renderer(renderer_location)?;
        if let Err(error) = self.renderer_backend(renderer_location)?.play(&renderer) {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        let snapshot = self.refresh_transport_state(renderer_location)?;
        self.database.set_queue_status(
            renderer_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
            &snapshot.transport_info.transport_state,
        )?;
        Ok("Playback resumed.".to_string())
    }

    pub(crate) fn pause_renderer(&self, renderer_location: &str) -> io::Result<String> {
        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::Group
        ) {
            let group = self.load_renderer_group_for_queue(renderer_location)?;
            let fanout = self.fan_out_group_transport_action(
                &group,
                "pause",
                |state, member_location, renderer| {
                    state.renderer_backend(member_location)?.pause(renderer)
                },
            )?;
            self.database
                .mark_next_queue_entry_preloaded(renderer_location, None)?;
            self.database
                .set_queue_status(renderer_location, "paused", "PAUSED_PLAYBACK")?;
            self.record_group_session_warning(renderer_location, "pause", &fanout);
            return Ok(self.group_fanout_message("paused", &fanout));
        }
        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::AndroidLocal | RendererKind::CliLocal
        ) {
            self.database
                .mark_next_queue_entry_preloaded(renderer_location, None)?;
            self.database
                .set_queue_status(renderer_location, "paused", "PAUSED_PLAYBACK")?;
            return Ok("Playback paused.".to_string());
        }
        let renderer = self.resolve_renderer(renderer_location)?;
        self.debug_log("pause-request", format!("renderer={renderer_location}"));
        if let Err(error) = self.renderer_backend(renderer_location)?.pause(&renderer) {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        let snapshot = self.wait_for_transport_state(
            renderer_location,
            &["PAUSED_PLAYBACK", "STOPPED", "NO_MEDIA_PRESENT"],
            6,
            Duration::from_millis(250),
        )?;
        self.database
            .mark_next_queue_entry_preloaded(renderer_location, None)?;
        self.database.set_queue_status(
            renderer_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
            &snapshot.transport_info.transport_state,
        )?;
        self.debug_log(
            "pause-settled",
            format!(
                "renderer={} state={} position={:?} duration={:?}",
                renderer_location,
                snapshot.transport_info.transport_state,
                snapshot.position_info.rel_time_seconds,
                snapshot.position_info.track_duration_seconds
            ),
        );
        if snapshot.transport_info.transport_state == "PAUSED_PLAYBACK" {
            Ok("Playback paused.".to_string())
        } else {
            Ok(format!(
                "Pause requested. Renderer now reports {}.",
                snapshot.transport_info.transport_state
            ))
        }
    }

    pub(crate) fn stop_renderer(&self, renderer_location: &str) -> io::Result<String> {
        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::Group
        ) {
            let group = self.load_renderer_group_for_queue(renderer_location)?;
            let fanout = self.fan_out_group_transport_action(
                &group,
                "stop",
                |state, member_location, renderer| {
                    state.renderer_backend(member_location)?.stop(renderer)
                },
            )?;
            self.database
                .mark_next_queue_entry_preloaded(renderer_location, None)?;
            self.database
                .set_queue_status(renderer_location, "stopped", "STOPPED")?;
            self.record_group_session_warning(renderer_location, "stop", &fanout);
            return Ok(self.group_fanout_message("stopped", &fanout));
        }
        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::AndroidLocal | RendererKind::CliLocal
        ) {
            self.database
                .mark_next_queue_entry_preloaded(renderer_location, None)?;
            self.database
                .set_queue_status(renderer_location, "stopped", "STOPPED")?;
            return Ok("Playback stopped.".to_string());
        }
        let renderer = self.resolve_renderer(renderer_location)?;
        self.debug_log("stop-request", format!("renderer={renderer_location}"));
        if let Err(error) = self.renderer_backend(renderer_location)?.stop(&renderer) {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        let snapshot = self.refresh_transport_state(renderer_location)?;
        self.database
            .mark_next_queue_entry_preloaded(renderer_location, None)?;
        self.database.set_queue_status(
            renderer_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
            &snapshot.transport_info.transport_state,
        )?;
        Ok("Playback stopped.".to_string())
    }

    pub(crate) fn skip_to_next(&self, renderer_location: &str) -> io::Result<String> {
        self.debug_log("next-request", format!("renderer={renderer_location}"));
        if let Some(queue) = self.queue_snapshot(renderer_location) {
            if let Some(current_entry_id) = queue.current_entry_id {
                if let Some(next_entry) = next_queue_entry_after(&queue, current_entry_id) {
                    self.database
                        .select_queue_entry(renderer_location, next_entry.id)?;
                    let (track, _, renderer_name, _) =
                        self.start_current_queue_entry(renderer_location)?;
                    return Ok(format!(
                        "Skipped to '{}' on {}.",
                        track.title, renderer_name
                    ));
                }
            }
        }

        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::AndroidLocal | RendererKind::CliLocal | RendererKind::Group
        ) {
            return Ok("No later track in the queue.".to_string());
        }

        let renderer = self.resolve_renderer(renderer_location)?;
        if let Err(error) = self.renderer_backend(renderer_location)?.next(&renderer) {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        let snapshot = self.refresh_transport_state(renderer_location)?;
        self.database.set_queue_status(
            renderer_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
            &snapshot.transport_info.transport_state,
        )?;
        Ok("Skipped to the next track.".to_string())
    }

    pub(crate) fn skip_to_previous(&self, renderer_location: &str) -> io::Result<String> {
        self.debug_log("previous-request", format!("renderer={renderer_location}"));
        if let Some(queue) = self.queue_snapshot(renderer_location) {
            if let Some(current_entry_id) = queue.current_entry_id {
                if let Some(previous_entry) = previous_queue_entry_before(&queue, current_entry_id)
                {
                    self.database
                        .select_queue_entry(renderer_location, previous_entry.id)?;
                    let (track, _, renderer_name, _) =
                        self.start_current_queue_entry(renderer_location)?;
                    return Ok(format!(
                        "Went back to '{}' on {}.",
                        track.title, renderer_name
                    ));
                }
            }
        }

        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::AndroidLocal | RendererKind::CliLocal | RendererKind::Group
        ) {
            return Ok("No earlier track in the queue.".to_string());
        }

        let renderer = self.resolve_renderer(renderer_location)?;
        if let Err(error) = self
            .renderer_backend(renderer_location)?
            .previous(&renderer)
        {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        let snapshot = self.refresh_transport_state(renderer_location)?;
        self.database.set_queue_status(
            renderer_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
            &snapshot.transport_info.transport_state,
        )?;
        Ok("Moved to the previous track.".to_string())
    }

    pub(crate) fn preload_next_queue_entry(
        &self,
        renderer_location: &str,
        renderer: &RendererRecord,
        queue: &PlaybackQueue,
        current_entry_id: i64,
    ) -> io::Result<()> {
        let Some(next_entry) = next_queue_entry_after(queue, current_entry_id) else {
            let had_preloaded_next = self
                .playback_session(renderer_location)
                .and_then(|session| session.next_queue_entry_id)
                .is_some();
            if had_preloaded_next {
                self.clear_renderer_next_queue_entry(renderer_location, renderer, "no-successor");
            }
            self.database
                .mark_next_queue_entry_preloaded(renderer_location, None)?;
            return Ok(());
        };

        let session = self.playback_session(renderer_location);
        if session
            .as_ref()
            .and_then(|session| session.next_queue_entry_id)
            == Some(next_entry.id)
        {
            return Ok(());
        }

        let track = self.find_track(&next_entry.track_id).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "queued next track not found")
        })?;
        let resource = self.stream_resource_for_track(&track);
        if renderer.capabilities.supports_set_next_av_transport_uri() == Some(false) {
            self.database
                .mark_next_queue_entry_preloaded(renderer_location, None)?;
            return Ok(());
        }
        if let Err(error) = self
            .renderer_backend(renderer_location)?
            .preload_next(renderer, &resource)
        {
            let _ = self.mark_renderer_unreachable(renderer_location, &error);
            return Err(error);
        }
        self.database
            .mark_next_queue_entry_preloaded(renderer_location, Some(next_entry.id))?;
        self.debug_log(
            "preload-next",
            format!(
                "renderer={} current_entry={} next_entry={} next_track={}",
                renderer_location, current_entry_id, next_entry.id, track.title
            ),
        );
        Ok(())
    }

    pub(crate) fn adopt_renderer_advanced_entry(
        &self,
        renderer_location: &str,
        queue: &PlaybackQueue,
        snapshot: &TransportSnapshot,
    ) -> io::Result<bool> {
        let Some(current_entry_id) = queue.current_entry_id else {
            return Ok(false);
        };
        let Some(next_entry) = next_queue_entry_after(queue, current_entry_id) else {
            return Ok(false);
        };
        let Some(track_uri) = snapshot.position_info.track_uri.as_deref() else {
            return Ok(false);
        };

        let next_track = self.find_track(&next_entry.track_id).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "queued next track not found")
        })?;
        let expected_stream_url = self.stream_resource_for_track(&next_track).stream_url;
        if !should_adopt_preloaded_next_entry(queue, snapshot, Some(&expected_stream_url)) {
            return Ok(false);
        }

        self.database.adopt_next_queue_entry_as_current(
            renderer_location,
            next_entry.id,
            &next_track.id,
            track_uri,
            next_track.duration_seconds,
        )?;
        if next_queue_entry_after(queue, next_entry.id).is_none() {
            match self.resolve_renderer(renderer_location) {
                Ok(renderer) => self.clear_renderer_next_queue_entry(
                    renderer_location,
                    &renderer,
                    "adopted-final",
                ),
                Err(error) => self.debug_log(
                    "clear-next-failed",
                    format!(
                        "renderer={} reason=adopted-final resolve_error={}",
                        renderer_location, error
                    ),
                ),
            }
        }
        self.debug_log(
            "renderer-advanced",
            format!(
                "renderer={} adopted_entry={} track={} uri={}",
                renderer_location, next_entry.id, next_track.title, track_uri
            ),
        );
        Ok(true)
    }

    fn clear_renderer_next_queue_entry(
        &self,
        renderer_location: &str,
        renderer: &RendererRecord,
        reason: &str,
    ) {
        if renderer.capabilities.supports_set_next_av_transport_uri() == Some(false) {
            return;
        }
        match self.renderer_backend(renderer_location) {
            Ok(backend) => match backend.clear_next(renderer) {
                Ok(()) => {
                    let _ = self.mark_renderer_reachable(renderer);
                    self.debug_log(
                        "clear-next",
                        format!("renderer={} reason={}", renderer_location, reason),
                    );
                }
                Err(error) => {
                    self.debug_log(
                        "clear-next-failed",
                        format!(
                            "renderer={} reason={} error={}",
                            renderer_location, reason, error
                        ),
                    );
                }
            },
            Err(error) => {
                self.debug_log(
                    "clear-next-failed",
                    format!(
                        "renderer={} reason={} backend_error={}",
                        renderer_location, reason, error
                    ),
                );
            }
        }
    }

    pub(crate) fn start_current_queue_entry(
        &self,
        renderer_location: &str,
    ) -> io::Result<(LibraryTrack, i64, String, String)> {
        let queue = self
            .database
            .load_queue(renderer_location)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue not found"))?;
        let current_entry = queue
            .entries
            .iter()
            .find(|entry| Some(entry.id) == queue.current_entry_id)
            .or_else(|| queue.entries.first())
            .cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queue is empty"))?;
        let track = self
            .find_track(&current_entry.track_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queued track not found"))?;
        let resource = self.stream_resource_for_track(&track);
        let stream_url = resource.stream_url.clone();
        self.debug_log(
            "queue-start",
            format!(
                "renderer={} entry={} track={} uri={}",
                renderer_location, current_entry.id, track.title, stream_url
            ),
        );
        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::Group
        ) {
            let group = self.load_renderer_group_for_queue(renderer_location)?;
            return match self.play_stream_on_group_members(&group, &resource) {
                Ok(fanout) => {
                    self.database.mark_queue_play_started(
                        renderer_location,
                        current_entry.id,
                        &track.id,
                        &stream_url,
                        track.duration_seconds,
                    )?;
                    if let Err(error) = self.preload_next_group_queue_entry(
                        renderer_location,
                        &group,
                        &queue,
                        current_entry.id,
                    ) {
                        eprintln!(
                            "group next-track preload failed for {renderer_location}: {error}"
                        );
                    }
                    self.record_group_session_warning(renderer_location, "start", &fanout);
                    Ok((
                        track,
                        current_entry.id,
                        if fanout.succeeded_count() == fanout.total_count() {
                            format!("{} ({} renderers)", group.name, fanout.succeeded_count())
                        } else {
                            format!(
                                "{} ({} of {} renderers)",
                                group.name,
                                fanout.succeeded_count(),
                                fanout.total_count()
                            )
                        },
                        renderer_location.to_string(),
                    ))
                }
                Err(error) => {
                    let _ = self.database.mark_queue_play_error(
                        renderer_location,
                        Some(current_entry.id),
                        &error.to_string(),
                    );
                    Err(error)
                }
            };
        }
        let renderer = self.resolve_renderer(renderer_location)?;

        match self
            .renderer_backend(renderer_location)?
            .play_stream(&renderer, &resource)
        {
            Ok(()) => {
                let _ = self.mark_renderer_reachable(&renderer);
                self.database.mark_queue_play_started(
                    renderer_location,
                    current_entry.id,
                    &track.id,
                    &stream_url,
                    track.duration_seconds,
                )?;
                if let Err(error) = self.preload_next_queue_entry(
                    renderer_location,
                    &renderer,
                    &queue,
                    current_entry.id,
                ) {
                    eprintln!("next-track preload failed for {renderer_location}: {error}");
                }
                Ok((
                    track,
                    current_entry.id,
                    renderer.name.clone(),
                    renderer.location.clone(),
                ))
            }
            Err(error) => {
                let _ = self.mark_renderer_unreachable(renderer_location, &error);
                let _ = self.database.mark_queue_play_error(
                    renderer_location,
                    Some(current_entry.id),
                    &error.to_string(),
                );
                Err(error)
            }
        }
    }

    fn resume_renderer_group(&self, renderer_location: &str) -> io::Result<String> {
        let group = self.load_renderer_group_for_queue(renderer_location)?;
        let queue = self.queue_snapshot(renderer_location);
        let session = self.playback_session(renderer_location);
        let current_entry_id = queue.as_ref().and_then(|queue| queue.current_entry_id);
        if current_entry_id.is_none() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "queue is empty"));
        }

        if session
            .as_ref()
            .map(|session| {
                matches!(
                    session.transport_state.as_str(),
                    "STOPPED" | "NO_MEDIA_PRESENT" | "READY" | "COMPLETED" | "ERROR"
                )
            })
            .unwrap_or(true)
        {
            let (track, _, renderer_name, _) = self.start_current_queue_entry(renderer_location)?;
            return Ok(format!(
                "Now playing '{}' on {}.",
                track.title, renderer_name
            ));
        }

        let fanout = self.fan_out_group_transport_action(
            &group,
            "play",
            |state, member_location, renderer| {
                state.renderer_backend(member_location)?.play(renderer)
            },
        )?;
        self.database
            .set_queue_status(renderer_location, "playing", "PLAYING")?;
        self.record_group_session_warning(renderer_location, "play", &fanout);
        Ok(self.group_fanout_message("resumed", &fanout))
    }
}
