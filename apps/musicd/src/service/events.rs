use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use musicd_upnp::TransportSnapshot;

use crate::types::PlaybackQueue;

#[derive(Debug, Default)]
pub(crate) struct PlaybackEvents {
    inner: Mutex<Inner>,
    cv: Condvar,
}

#[derive(Debug, Default)]
struct Inner {
    renderers: HashMap<String, RendererState>,
    total_subscribers: usize,
}

#[derive(Debug, Default)]
struct RendererState {
    subscriber_count: usize,
    version: u64,
    last_fingerprint: Option<u64>,
}

impl PlaybackEvents {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn subscribe(&self, location: &str) -> SubscriberGuard<'_> {
        let mut inner = self.inner.lock().expect("playback events lock poisoned");
        inner.total_subscribers += 1;
        inner
            .renderers
            .entry(location.to_string())
            .or_default()
            .subscriber_count += 1;
        SubscriberGuard {
            events: self,
            location: location.to_string(),
        }
    }

    pub(crate) fn any_subscribers(&self) -> bool {
        self.inner
            .lock()
            .expect("playback events lock poisoned")
            .total_subscribers
            > 0
    }

    pub(crate) fn version(&self, location: &str) -> u64 {
        self.inner
            .lock()
            .expect("playback events lock poisoned")
            .renderers
            .get(location)
            .map(|state| state.version)
            .unwrap_or(0)
    }

    /// Block until the renderer's version exceeds `last_seen`, or until `timeout`
    /// expires. Returns the most recent version observed under the lock.
    pub(crate) fn wait_for_change(
        &self,
        location: &str,
        last_seen: u64,
        timeout: Duration,
    ) -> u64 {
        let deadline = Instant::now() + timeout;
        let mut inner = self.inner.lock().expect("playback events lock poisoned");
        loop {
            let current = inner
                .renderers
                .get(location)
                .map(|state| state.version)
                .unwrap_or(0);
            if current != last_seen {
                return current;
            }
            let now = Instant::now();
            if now >= deadline {
                return current;
            }
            let (next_inner, _result) = self
                .cv
                .wait_timeout(inner, deadline - now)
                .expect("playback events lock poisoned");
            inner = next_inner;
        }
    }

    /// Record an observed state fingerprint. If it differs from the last one,
    /// bump the version and wake any waiting subscribers. Returns whether the
    /// state was treated as changed.
    pub(crate) fn note_state(&self, location: &str, fingerprint: u64) -> bool {
        let mut inner = self.inner.lock().expect("playback events lock poisoned");
        let entry = inner.renderers.entry(location.to_string()).or_default();
        if entry.last_fingerprint == Some(fingerprint) {
            return false;
        }
        entry.last_fingerprint = Some(fingerprint);
        entry.version = entry.version.wrapping_add(1);
        drop(inner);
        self.cv.notify_all();
        true
    }

    /// Force a notification without an associated fingerprint (e.g. after a
    /// user-driven mutation). Invalidates the cached fingerprint so the next
    /// poll will be treated as a change.
    pub(crate) fn touch(&self, location: &str) {
        let mut inner = self.inner.lock().expect("playback events lock poisoned");
        let entry = inner.renderers.entry(location.to_string()).or_default();
        entry.last_fingerprint = None;
        entry.version = entry.version.wrapping_add(1);
        drop(inner);
        self.cv.notify_all();
    }

    fn release_subscriber(&self, location: &str) {
        let mut inner = self.inner.lock().expect("playback events lock poisoned");
        inner.total_subscribers = inner.total_subscribers.saturating_sub(1);
        if let Some(entry) = inner.renderers.get_mut(location) {
            entry.subscriber_count = entry.subscriber_count.saturating_sub(1);
        }
    }
}

pub(crate) struct SubscriberGuard<'a> {
    events: &'a PlaybackEvents,
    location: String,
}

impl Drop for SubscriberGuard<'_> {
    fn drop(&mut self) {
        self.events.release_subscriber(&self.location);
    }
}

/// Hash the parts of the renderer state that should drive an SSE notification.
///
/// Position is included (rounded to whole seconds) so the progress UI updates
/// while a track plays, but not so granularly that we churn the version
/// counter.
pub(crate) fn fingerprint(queue: &PlaybackQueue, snapshot: &TransportSnapshot) -> u64 {
    let mut hasher = DefaultHasher::new();
    snapshot.transport_info.transport_state.hash(&mut hasher);
    snapshot.transport_info.transport_status.hash(&mut hasher);
    snapshot.position_info.track_uri.hash(&mut hasher);
    snapshot.position_info.rel_time_seconds.hash(&mut hasher);
    snapshot
        .position_info
        .track_duration_seconds
        .hash(&mut hasher);
    queue.version.hash(&mut hasher);
    queue.current_entry_id.hash(&mut hasher);
    queue.status.hash(&mut hasher);
    hasher.finish()
}
