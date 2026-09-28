use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::types::RendererPlaybackHealth;

const STARTUP_GRACE: Duration = Duration::from_secs(15);
const POSITION_STALL_THRESHOLD: Duration = Duration::from_secs(8);
const BACKPRESSURE_WINDOW: Duration = Duration::from_secs(20);
const BACKPRESSURE_RETENTION: Duration = Duration::from_secs(60);
const REQUIRED_BACKPRESSURE_EVENTS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RendererHealthTransition {
    Stalled,
    Recovered,
}

#[derive(Debug)]
struct BackpressureObservation {
    window_started: Instant,
    last_observed: Instant,
    count: u32,
}

#[derive(Debug)]
struct RendererObservation {
    track_id: String,
    track_started: Instant,
    last_position: u64,
    last_progress: Instant,
    health: Option<RendererPlaybackHealth>,
}

#[derive(Debug, Default)]
struct PlaybackHealthState {
    active_streams: HashMap<String, usize>,
    backpressure: HashMap<String, BackpressureObservation>,
    renderers: HashMap<String, RendererObservation>,
}

#[derive(Debug, Default)]
pub(crate) struct PlaybackHealthMonitor {
    state: Mutex<PlaybackHealthState>,
}

impl PlaybackHealthMonitor {
    pub(crate) fn stream_started(&self, track_id: &str) {
        let mut state = self.state.lock().expect("playback health lock poisoned");
        if !state.active_streams.contains_key(track_id) {
            state.backpressure.remove(track_id);
        }
        *state
            .active_streams
            .entry(track_id.to_string())
            .or_default() += 1;
    }

    pub(crate) fn stream_finished(&self, track_id: &str) {
        let mut state = self.state.lock().expect("playback health lock poisoned");
        let should_remove = state
            .active_streams
            .get_mut(track_id)
            .map(|count| {
                *count = count.saturating_sub(1);
                *count == 0
            })
            .unwrap_or(false);
        if should_remove {
            state.active_streams.remove(track_id);
        }
    }

    pub(crate) fn record_backpressure(&self, track_id: &str) {
        self.record_backpressure_at(track_id, Instant::now());
    }

    fn record_backpressure_at(&self, track_id: &str, now: Instant) {
        let mut state = self.state.lock().expect("playback health lock poisoned");
        state.backpressure.retain(|_, observation| {
            now.duration_since(observation.last_observed) <= BACKPRESSURE_RETENTION
        });
        let observation =
            state
                .backpressure
                .entry(track_id.to_string())
                .or_insert(BackpressureObservation {
                    window_started: now,
                    last_observed: now,
                    count: 0,
                });
        if now.duration_since(observation.window_started) > BACKPRESSURE_WINDOW {
            observation.window_started = now;
            observation.count = 0;
        }
        observation.last_observed = now;
        observation.count = observation.count.saturating_add(1);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn observe_renderer(
        &self,
        renderer_location: &str,
        track_id: Option<&str>,
        transport_state: &str,
        position_seconds: Option<u64>,
        duration_seconds: Option<u64>,
        observed_unix: i64,
    ) -> Option<RendererHealthTransition> {
        self.observe_renderer_at(
            renderer_location,
            track_id,
            transport_state,
            position_seconds,
            duration_seconds,
            observed_unix,
            Instant::now(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn observe_renderer_at(
        &self,
        renderer_location: &str,
        track_id: Option<&str>,
        transport_state: &str,
        position_seconds: Option<u64>,
        duration_seconds: Option<u64>,
        observed_unix: i64,
        now: Instant,
    ) -> Option<RendererHealthTransition> {
        let mut state = self.state.lock().expect("playback health lock poisoned");
        let previously_stalled = state
            .renderers
            .get(renderer_location)
            .and_then(|observation| observation.health.as_ref())
            .is_some();

        let Some(track_id) = track_id else {
            state.renderers.remove(renderer_location);
            return previously_stalled.then_some(RendererHealthTransition::Recovered);
        };
        let Some(position_seconds) = position_seconds else {
            state.renderers.remove(renderer_location);
            return previously_stalled.then_some(RendererHealthTransition::Recovered);
        };
        if transport_state != "PLAYING"
            || duration_seconds
                .is_some_and(|duration| position_seconds.saturating_add(3) >= duration)
        {
            state.renderers.remove(renderer_location);
            return previously_stalled.then_some(RendererHealthTransition::Recovered);
        }

        let track_changed = state
            .renderers
            .get(renderer_location)
            .map(|observation| observation.track_id != track_id)
            .unwrap_or(true);
        if track_changed {
            state.renderers.insert(
                renderer_location.to_string(),
                RendererObservation {
                    track_id: track_id.to_string(),
                    track_started: now,
                    last_position: position_seconds,
                    last_progress: now,
                    health: None,
                },
            );
            return previously_stalled.then_some(RendererHealthTransition::Recovered);
        }

        let made_progress = state
            .renderers
            .get(renderer_location)
            .is_some_and(|observation| observation.last_position != position_seconds);
        if made_progress {
            let observation = state
                .renderers
                .get_mut(renderer_location)
                .expect("renderer observation should exist");
            let recovered = observation.health.take().is_some();
            observation.last_position = position_seconds;
            observation.last_progress = now;
            return recovered.then_some(RendererHealthTransition::Recovered);
        }

        let has_active_stream = state.active_streams.get(track_id).copied().unwrap_or(0) > 0;
        let has_recent_backpressure = state.backpressure.get(track_id).is_some_and(|observation| {
            observation.count >= REQUIRED_BACKPRESSURE_EVENTS
                && now.duration_since(observation.last_observed) <= BACKPRESSURE_WINDOW
        });
        let observation = state
            .renderers
            .get_mut(renderer_location)
            .expect("renderer observation should exist");
        if observation.health.is_none()
            && has_active_stream
            && has_recent_backpressure
            && now.duration_since(observation.track_started) >= STARTUP_GRACE
            && now.duration_since(observation.last_progress) >= POSITION_STALL_THRESHOLD
        {
            let stalled_for_seconds = now.duration_since(observation.last_progress).as_secs();
            observation.health = Some(RendererPlaybackHealth {
                state: "stalled".to_string(),
                reason: "renderer_not_consuming_stream".to_string(),
                detected_unix: observed_unix,
                position_seconds: Some(position_seconds),
                stalled_for_seconds,
                recommended_action: "restart_renderer".to_string(),
                message: "Renderer appears to have stopped consuming audio. Retry playback, then restart the renderer if the problem continues.".to_string(),
            });
            return Some(RendererHealthTransition::Stalled);
        }
        None
    }

    pub(crate) fn health(&self, renderer_location: &str) -> Option<RendererPlaybackHealth> {
        self.state
            .lock()
            .expect("playback health lock poisoned")
            .renderers
            .get(renderer_location)
            .and_then(|observation| observation.health.clone())
    }

    pub(crate) fn clear_renderer(
        &self,
        renderer_location: &str,
    ) -> Option<RendererHealthTransition> {
        self.state
            .lock()
            .expect("playback health lock poisoned")
            .renderers
            .remove(renderer_location)
            .and_then(|observation| observation.health)
            .map(|_| RendererHealthTransition::Recovered)
    }
}

#[cfg(test)]
mod tests {
    use super::{PlaybackHealthMonitor, RendererHealthTransition};
    use std::time::{Duration, Instant};

    #[test]
    fn requires_active_stream_backpressure_and_frozen_position() {
        let monitor = PlaybackHealthMonitor::default();
        let started = Instant::now();
        monitor.stream_started("track-1");
        monitor.record_backpressure_at("track-1", started + Duration::from_secs(1));
        monitor.record_backpressure_at("track-1", started + Duration::from_secs(3));

        assert_eq!(
            monitor.observe_renderer_at(
                "renderer-1",
                Some("track-1"),
                "PLAYING",
                Some(12),
                Some(180),
                100,
                started,
            ),
            None,
        );
        assert_eq!(
            monitor.observe_renderer_at(
                "renderer-1",
                Some("track-1"),
                "PLAYING",
                Some(12),
                Some(180),
                116,
                started + Duration::from_secs(16),
            ),
            Some(RendererHealthTransition::Stalled),
        );
        assert_eq!(
            monitor.health("renderer-1").map(|health| health.reason),
            Some("renderer_not_consuming_stream".to_string()),
        );
    }

    #[test]
    fn position_progress_clears_a_stalled_renderer() {
        let monitor = PlaybackHealthMonitor::default();
        let started = Instant::now();
        monitor.stream_started("track-1");
        monitor.record_backpressure_at("track-1", started + Duration::from_secs(1));
        monitor.record_backpressure_at("track-1", started + Duration::from_secs(3));
        monitor.observe_renderer_at(
            "renderer-1",
            Some("track-1"),
            "PLAYING",
            Some(12),
            Some(180),
            100,
            started,
        );
        monitor.observe_renderer_at(
            "renderer-1",
            Some("track-1"),
            "PLAYING",
            Some(12),
            Some(180),
            116,
            started + Duration::from_secs(16),
        );

        assert_eq!(
            monitor.observe_renderer_at(
                "renderer-1",
                Some("track-1"),
                "PLAYING",
                Some(13),
                Some(180),
                118,
                started + Duration::from_secs(18),
            ),
            Some(RendererHealthTransition::Recovered),
        );
        assert!(monitor.health("renderer-1").is_none());
    }

    #[test]
    fn ordinary_renderer_pacing_does_not_trigger_when_position_advances() {
        let monitor = PlaybackHealthMonitor::default();
        let started = Instant::now();
        monitor.stream_started("track-1");
        monitor.record_backpressure_at("track-1", started + Duration::from_secs(1));
        monitor.record_backpressure_at("track-1", started + Duration::from_secs(3));
        monitor.observe_renderer_at(
            "renderer-1",
            Some("track-1"),
            "PLAYING",
            Some(1),
            Some(180),
            100,
            started,
        );

        assert_eq!(
            monitor.observe_renderer_at(
                "renderer-1",
                Some("track-1"),
                "PLAYING",
                Some(9),
                Some(180),
                116,
                started + Duration::from_secs(16),
            ),
            None,
        );
        assert!(monitor.health("renderer-1").is_none());
    }

    #[test]
    fn frozen_position_without_stream_backpressure_does_not_trigger() {
        let monitor = PlaybackHealthMonitor::default();
        let started = Instant::now();
        monitor.stream_started("track-1");
        monitor.observe_renderer_at(
            "renderer-1",
            Some("track-1"),
            "PLAYING",
            Some(12),
            Some(180),
            100,
            started,
        );

        assert_eq!(
            monitor.observe_renderer_at(
                "renderer-1",
                Some("track-1"),
                "PLAYING",
                Some(12),
                Some(180),
                130,
                started + Duration::from_secs(30),
            ),
            None,
        );
        assert!(monitor.health("renderer-1").is_none());
    }
}
