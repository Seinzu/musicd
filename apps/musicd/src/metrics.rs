use std::cell::Cell;
use std::io;
use std::path::Path;
use std::sync::Weak;
use std::time::Duration;

use prometheus_client::collector::Collector;
use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::{DescriptorEncoder, EncodeLabelSet, EncodeMetric};
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::{ConstGauge, Gauge};
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

use crate::service::ServiceState;

#[derive(Clone, Debug, Hash, Eq, PartialEq, EncodeLabelSet)]
pub struct RequestLabels {
    pub method: String,
    pub route: String,
    pub status: String,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, EncodeLabelSet)]
pub struct DurationLabels {
    pub method: String,
    pub route: String,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, EncodeLabelSet)]
pub struct OutcomeLabels {
    pub outcome: String,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, EncodeLabelSet)]
pub struct ChangeLabels {
    pub kind: String,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, EncodeLabelSet)]
pub struct StageLabels {
    pub stage: String,
}

#[derive(Debug)]
pub struct Metrics {
    registry: Registry,
    request_count: Family<RequestLabels, Counter>,
    request_duration: Family<DurationLabels, Histogram, fn() -> Histogram>,
    stream_transfer_count: Family<OutcomeLabels, Counter>,
    stream_transfer_bytes: Counter,
    stream_transfer_duration: Histogram,
    stream_max_file_read_duration: Histogram,
    stream_max_socket_write_duration: Histogram,
    stream_stall_count: Family<StageLabels, Counter>,
    renderer_health_transition_count: Family<OutcomeLabels, Counter>,
    library_watcher_poll_count: Family<OutcomeLabels, Counter>,
    library_watcher_poll_duration: Histogram,
    library_watcher_directories_examined: Gauge,
    library_watcher_files_examined: Gauge,
    library_watcher_skipped_entries: Gauge,
    library_watcher_change_count: Family<ChangeLabels, Counter>,
    library_watcher_event_count: Family<OutcomeLabels, Counter>,
    library_watcher_event_batch_count: Family<OutcomeLabels, Counter>,
    library_watcher_pending_paths: Gauge,
    library_watcher_native_active: Gauge,
}

fn build_histogram() -> Histogram {
    Histogram::new(
        [
            0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ]
        .into_iter(),
    )
}

fn build_library_watcher_histogram() -> Histogram {
    Histogram::new(
        [
            0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 300.0, 900.0,
        ]
        .into_iter(),
    )
}

fn build_stream_transfer_histogram() -> Histogram {
    Histogram::new(
        [
            1.0, 5.0, 15.0, 30.0, 60.0, 300.0, 900.0, 1800.0, 3600.0, 7200.0,
        ]
        .into_iter(),
    )
}

fn build_stream_operation_histogram() -> Histogram {
    Histogram::new(
        [
            0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0,
        ]
        .into_iter(),
    )
}

impl Metrics {
    pub fn new(state: Weak<ServiceState>) -> Self {
        let mut registry = Registry::default();

        let request_count = Family::<RequestLabels, Counter>::default();
        registry.register(
            "musicd_http_requests",
            "HTTP requests served by musicd, partitioned by method, route, and status",
            request_count.clone(),
        );

        let request_duration: Family<DurationLabels, Histogram, fn() -> Histogram> =
            Family::new_with_constructor(build_histogram);
        registry.register(
            "musicd_http_request_duration_seconds",
            "HTTP request handler duration in seconds, partitioned by method and route",
            request_duration.clone(),
        );

        let stream_transfer_count = Family::<OutcomeLabels, Counter>::default();
        registry.register(
            "musicd_stream_transfers",
            "Local-library stream transfers partitioned by outcome",
            stream_transfer_count.clone(),
        );

        let stream_transfer_bytes = Counter::default();
        registry.register(
            "musicd_stream_bytes",
            "Local-library audio bytes written to renderers",
            stream_transfer_bytes.clone(),
        );

        let stream_transfer_duration = build_stream_transfer_histogram();
        registry.register(
            "musicd_stream_transfer_duration_seconds",
            "End-to-end duration of finished local-library stream requests",
            stream_transfer_duration.clone(),
        );

        let stream_max_file_read_duration = build_stream_operation_histogram();
        registry.register(
            "musicd_stream_max_file_read_duration_seconds",
            "Longest individual file read in each finished local-library stream request",
            stream_max_file_read_duration.clone(),
        );

        let stream_max_socket_write_duration = build_stream_operation_histogram();
        registry.register(
            "musicd_stream_max_socket_write_duration_seconds",
            "Longest socket write or flush in each finished local-library stream request",
            stream_max_socket_write_duration.clone(),
        );

        let stream_stall_count = Family::<StageLabels, Counter>::default();
        registry.register(
            "musicd_stream_stalls",
            "Slow local-library stream operations partitioned by stage",
            stream_stall_count.clone(),
        );

        let renderer_health_transition_count = Family::<OutcomeLabels, Counter>::default();
        registry.register(
            "musicd_renderer_health_transitions",
            "Detected renderer playback health transitions partitioned by outcome",
            renderer_health_transition_count.clone(),
        );

        let library_watcher_poll_count = Family::<OutcomeLabels, Counter>::default();
        registry.register(
            "musicd_library_watcher_polls",
            "Library watcher poll attempts partitioned by outcome",
            library_watcher_poll_count.clone(),
        );

        let library_watcher_poll_duration = build_library_watcher_histogram();
        registry.register(
            "musicd_library_watcher_poll_duration_seconds",
            "Time spent in attempted library watcher polls",
            library_watcher_poll_duration.clone(),
        );

        let library_watcher_directories_examined = Gauge::default();
        registry.register(
            "musicd_library_watcher_directories_examined",
            "Directories examined by the most recent completed library watcher poll",
            library_watcher_directories_examined.clone(),
        );

        let library_watcher_files_examined = Gauge::default();
        registry.register(
            "musicd_library_watcher_files_examined",
            "Audio files examined by the most recent completed library watcher poll",
            library_watcher_files_examined.clone(),
        );

        let library_watcher_skipped_entries = Gauge::default();
        registry.register(
            "musicd_library_watcher_skipped_entries",
            "Entries skipped by the most recent completed library watcher poll",
            library_watcher_skipped_entries.clone(),
        );

        let library_watcher_change_count = Family::<ChangeLabels, Counter>::default();
        registry.register(
            "musicd_library_watcher_changes",
            "Library changes applied by the watcher partitioned by kind",
            library_watcher_change_count.clone(),
        );

        let library_watcher_event_count = Family::<OutcomeLabels, Counter>::default();
        registry.register(
            "musicd_library_watcher_events",
            "Native filesystem events received by the library watcher partitioned by outcome",
            library_watcher_event_count.clone(),
        );

        let library_watcher_event_batch_count = Family::<OutcomeLabels, Counter>::default();
        registry.register(
            "musicd_library_watcher_event_batches",
            "Debounced native filesystem event batches partitioned by outcome",
            library_watcher_event_batch_count.clone(),
        );

        let library_watcher_pending_paths = Gauge::default();
        registry.register(
            "musicd_library_watcher_pending_paths",
            "Filesystem paths currently pending debounce or retry",
            library_watcher_pending_paths.clone(),
        );

        let library_watcher_native_active = Gauge::default();
        registry.register(
            "musicd_library_watcher_native_active",
            "Whether the native filesystem watcher is active (1 active, 0 inactive)",
            library_watcher_native_active.clone(),
        );

        registry.register_collector(Box::new(SnapshotCollector { state }));

        Self {
            registry,
            request_count,
            request_duration,
            stream_transfer_count,
            stream_transfer_bytes,
            stream_transfer_duration,
            stream_max_file_read_duration,
            stream_max_socket_write_duration,
            stream_stall_count,
            renderer_health_transition_count,
            library_watcher_poll_count,
            library_watcher_poll_duration,
            library_watcher_directories_examined,
            library_watcher_files_examined,
            library_watcher_skipped_entries,
            library_watcher_change_count,
            library_watcher_event_count,
            library_watcher_event_batch_count,
            library_watcher_pending_paths,
            library_watcher_native_active,
        }
    }

    pub fn record_request(&self, method: &str, route: &str, status: u16, duration: Duration) {
        self.request_count
            .get_or_create(&RequestLabels {
                method: method.to_string(),
                route: route.to_string(),
                status: status.to_string(),
            })
            .inc();

        self.request_duration
            .get_or_create(&DurationLabels {
                method: method.to_string(),
                route: route.to_string(),
            })
            .observe(duration.as_secs_f64());
    }

    pub(crate) fn record_stream_stall(&self, stage: &str) {
        self.stream_stall_count
            .get_or_create(&StageLabels {
                stage: stage.to_string(),
            })
            .inc();
    }

    pub(crate) fn record_stream_transfer(
        &self,
        outcome: &str,
        bytes: u64,
        duration: Duration,
        max_file_read: Duration,
        max_socket_write: Duration,
    ) {
        self.stream_transfer_count
            .get_or_create(&OutcomeLabels {
                outcome: outcome.to_string(),
            })
            .inc();
        self.stream_transfer_bytes.inc_by(bytes);
        self.stream_transfer_duration
            .observe(duration.as_secs_f64());
        self.stream_max_file_read_duration
            .observe(max_file_read.as_secs_f64());
        self.stream_max_socket_write_duration
            .observe(max_socket_write.as_secs_f64());
    }

    pub(crate) fn record_renderer_health_transition(&self, outcome: &str) {
        self.renderer_health_transition_count
            .get_or_create(&OutcomeLabels {
                outcome: outcome.to_string(),
            })
            .inc();
    }

    pub(crate) fn record_library_watcher_deferred(&self) {
        self.library_watcher_poll_count
            .get_or_create(&OutcomeLabels {
                outcome: "deferred".to_string(),
            })
            .inc();
    }

    pub(crate) fn record_library_watcher_error(&self, duration: Duration) {
        self.library_watcher_poll_count
            .get_or_create(&OutcomeLabels {
                outcome: "error".to_string(),
            })
            .inc();
        self.library_watcher_poll_duration
            .observe(duration.as_secs_f64());
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_library_watcher_poll(
        &self,
        outcome: &str,
        duration: Duration,
        files_examined: usize,
        directories_examined: usize,
        skipped_entries: usize,
        upserted: usize,
        removed: usize,
    ) {
        self.library_watcher_poll_count
            .get_or_create(&OutcomeLabels {
                outcome: outcome.to_string(),
            })
            .inc();
        self.library_watcher_poll_duration
            .observe(duration.as_secs_f64());
        self.library_watcher_files_examined
            .set(saturating_i64(files_examined));
        self.library_watcher_directories_examined
            .set(saturating_i64(directories_examined));
        self.library_watcher_skipped_entries
            .set(saturating_i64(skipped_entries));
        self.record_library_watcher_changes(upserted, removed);
    }

    pub(crate) fn record_library_watcher_event(&self, outcome: &str) {
        self.library_watcher_event_count
            .get_or_create(&OutcomeLabels {
                outcome: outcome.to_string(),
            })
            .inc();
    }

    pub(crate) fn record_library_watcher_event_batch(
        &self,
        outcome: &str,
        upserted: usize,
        removed: usize,
    ) {
        self.library_watcher_event_batch_count
            .get_or_create(&OutcomeLabels {
                outcome: outcome.to_string(),
            })
            .inc();
        self.record_library_watcher_changes(upserted, removed);
    }

    pub(crate) fn set_library_watcher_pending_paths(&self, count: usize) {
        self.library_watcher_pending_paths
            .set(saturating_i64(count));
    }

    pub(crate) fn set_library_watcher_native_active(&self, active: bool) {
        self.library_watcher_native_active
            .set(if active { 1 } else { 0 });
    }

    fn record_library_watcher_changes(&self, upserted: usize, removed: usize) {
        self.library_watcher_change_count
            .get_or_create(&ChangeLabels {
                kind: "upserted".to_string(),
            })
            .inc_by(upserted as u64);
        self.library_watcher_change_count
            .get_or_create(&ChangeLabels {
                kind: "removed".to_string(),
            })
            .inc_by(removed as u64);
    }

    pub fn encode(&self) -> String {
        let mut buffer = String::new();
        if encode(&mut buffer, &self.registry).is_err() {
            return String::new();
        }
        buffer
    }
}

#[derive(Debug)]
struct SnapshotCollector {
    state: Weak<ServiceState>,
}

impl Collector for SnapshotCollector {
    fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), std::fmt::Error> {
        let Some(state) = self.state.upgrade() else {
            return Ok(());
        };

        let renderers = state.enriched_renderer_snapshot();
        let reachable_renderers = renderers
            .iter()
            .filter(|renderer| {
                renderer.last_reachable_unix.is_some() && renderer.last_error.is_none()
            })
            .count();
        let playing_queue_renderers = state
            .database
            .list_playing_queue_renderers()
            .map(|values| values.len())
            .unwrap_or(0);
        let db_path = state.config.config_path.join("musicd.db");
        let db_bytes = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
        let (artwork_files, artwork_bytes) =
            directory_metrics(&state.config.config_path.join("artwork")).unwrap_or((0, 0));

        let entries: [(&str, &str, i64); 10] = [
            (
                "musicd_tracks_total",
                "Number of indexed tracks",
                state.track_count() as i64,
            ),
            (
                "musicd_albums_total",
                "Number of indexed albums",
                state.albums_snapshot().len() as i64,
            ),
            (
                "musicd_artists_total",
                "Number of indexed artists",
                state.artists_snapshot().len() as i64,
            ),
            (
                "musicd_renderers_total",
                "Number of remembered viable renderers",
                renderers.len() as i64,
            ),
            (
                "musicd_renderers_reachable",
                "Number of renderers currently considered reachable",
                reachable_renderers as i64,
            ),
            (
                "musicd_playback_queues_playing",
                "Number of renderer queues currently marked as playing",
                playing_queue_renderers as i64,
            ),
            (
                "musicd_library_streams_active",
                "Number of active local-library audio streams",
                saturating_i64(state.active_library_stream_count()),
            ),
            (
                "musicd_sqlite_bytes",
                "Size of the SQLite database in bytes",
                db_bytes as i64,
            ),
            (
                "musicd_artwork_cache_files",
                "Number of cached artwork files",
                artwork_files as i64,
            ),
            (
                "musicd_artwork_cache_bytes",
                "Size of the artwork cache in bytes",
                artwork_bytes as i64,
            ),
        ];

        for (name, help, value) in entries {
            let metric = ConstGauge::new(value);
            let metric_encoder =
                encoder.encode_descriptor(name, help, None, metric.metric_type())?;
            metric.encode(metric_encoder)?;
        }

        Ok(())
    }
}

pub fn route_template(path: &str) -> String {
    if path == "/" {
        return "/".to_string();
    }

    if let Some(rest) = path.strip_prefix("/api/albums/") {
        if rest == "artwork/select" {
            return "/api/albums/artwork/select".to_string();
        }
        if rest.ends_with("/artwork/candidates") {
            return "/api/albums/{album_id}/artwork/candidates".to_string();
        }
        return "/api/albums/{album_id}".to_string();
    }
    if path.starts_with("/api/tracks/") {
        return "/api/tracks/{track_id}".to_string();
    }
    if path.starts_with("/api/artists/") {
        return "/api/artists/{artist_id}".to_string();
    }
    if path.starts_with("/track/") {
        return "/track/{track_id}".to_string();
    }
    if path.starts_with("/album/") {
        return "/album/{album_id}".to_string();
    }
    if path.starts_with("/stream/track/") {
        return "/stream/track/{track_id}".to_string();
    }
    if path.starts_with("/stream/tidal/") {
        return "/stream/tidal/{track_id}".to_string();
    }
    if path.starts_with("/artwork/track/") {
        return "/artwork/track/{track_id}".to_string();
    }
    if path.starts_with("/artwork/album/") {
        return "/artwork/album/{album_id}".to_string();
    }

    if KNOWN_ROUTES.binary_search(&path).is_ok() {
        return path.to_string();
    }

    "<other>".to_string()
}

const KNOWN_ROUTES: &[&str] = &[
    "/",
    "/api/albums",
    "/api/albums/artwork/select",
    "/api/artists",
    "/api/events",
    "/api/like",
    "/api/now-playing",
    "/api/play",
    "/api/play-album",
    "/api/playback-targets",
    "/api/queue",
    "/api/queue/append-album",
    "/api/queue/append-track",
    "/api/queue/clear",
    "/api/queue/move",
    "/api/queue/play-next-album",
    "/api/queue/play-next-track",
    "/api/queue/remove",
    "/api/queue/tidal/append-album",
    "/api/queue/tidal/append-track",
    "/api/queue/tidal/play-next-album",
    "/api/queue/tidal/play-next-track",
    "/api/recommendation-seeds",
    "/api/recommendations",
    "/api/recommendations/dismiss",
    "/api/recommendations/import",
    "/api/renderer-groups",
    "/api/renderer-groups/delete",
    "/api/renderer-groups/update",
    "/api/renderers",
    "/api/renderers/android-local/completed",
    "/api/renderers/android-local/session",
    "/api/renderers/cli-local/completed",
    "/api/renderers/cli-local/session",
    "/api/renderers/discover",
    "/api/renderers/register-android-local",
    "/api/renderers/register-cli-local",
    "/api/renderers/volume",
    "/api/server",
    "/api/session",
    "/api/tidal/auth-url",
    "/api/tidal/complete-auth",
    "/api/tidal/play-album",
    "/api/tidal/play-track",
    "/api/tidal/search-albums",
    "/api/tidal/search-tracks",
    "/api/tracks",
    "/api/transport/next",
    "/api/transport/pause",
    "/api/transport/play",
    "/api/transport/previous",
    "/api/transport/retry",
    "/api/transport/stop",
    "/health",
    "/metrics",
    "/play",
    "/play-album",
    "/queue/append-album",
    "/queue/append-track",
    "/queue/clear",
    "/queue/move-down",
    "/queue/move-up",
    "/queue/panel",
    "/queue/play-next-album",
    "/queue/play-next-track",
    "/queue/remove-entry",
    "/rescan",
    "/stream/current",
    "/transport/next",
    "/transport/pause",
    "/transport/play",
    "/transport/previous",
    "/transport/stop",
];

thread_local! {
    static REQUEST_STATUS: Cell<u16> = const { Cell::new(0) };
}

pub fn set_response_status(code: u16) {
    REQUEST_STATUS.with(|cell| cell.set(code));
}

pub fn take_response_status() -> u16 {
    REQUEST_STATUS.with(|cell| {
        let value = cell.get();
        cell.set(0);
        value
    })
}

fn directory_metrics(path: &Path) -> io::Result<(u64, u64)> {
    if !path.exists() {
        return Ok((0, 0));
    }

    let mut file_count = 0_u64;
    let mut total_bytes = 0_u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            file_count += 1;
            total_bytes += metadata.len();
        }
    }
    Ok((file_count, total_bytes))
}

fn saturating_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::Metrics;
    use crate::service::ServiceState;
    use std::sync::Weak;
    use std::time::Duration;

    #[test]
    fn exports_library_watcher_metrics() {
        let metrics = Metrics::new(Weak::<ServiceState>::new());
        metrics.record_library_watcher_deferred();
        metrics.record_library_watcher_error(Duration::from_millis(250));
        metrics.record_library_watcher_poll(
            "completed",
            Duration::from_millis(1500),
            16_034,
            4_750,
            12,
            2,
            1,
        );
        metrics.record_library_watcher_event("received");
        metrics.record_library_watcher_event_batch("completed", 3, 2);
        metrics.set_library_watcher_pending_paths(4);
        metrics.set_library_watcher_native_active(true);
        metrics.record_stream_stall("socket_write");
        metrics.record_renderer_health_transition("stalled");
        metrics.record_stream_transfer(
            "completed",
            1_048_576,
            Duration::from_secs(90),
            Duration::from_millis(25),
            Duration::from_secs(3),
        );

        let encoded = metrics.encode();
        assert!(encoded.contains("musicd_library_watcher_polls_total{outcome=\"deferred\"} 1"));
        assert!(encoded.contains("musicd_library_watcher_polls_total{outcome=\"error\"} 1"));
        assert!(encoded.contains("musicd_library_watcher_polls_total{outcome=\"completed\"} 1"));
        assert!(encoded.contains("musicd_library_watcher_files_examined 16034"));
        assert!(encoded.contains("musicd_library_watcher_directories_examined 4750"));
        assert!(encoded.contains("musicd_library_watcher_skipped_entries 12"));
        assert!(encoded.contains("musicd_library_watcher_changes_total{kind=\"upserted\"} 5"));
        assert!(encoded.contains("musicd_library_watcher_changes_total{kind=\"removed\"} 3"));
        assert!(encoded.contains("musicd_library_watcher_events_total{outcome=\"received\"} 1"));
        assert!(
            encoded.contains("musicd_library_watcher_event_batches_total{outcome=\"completed\"} 1")
        );
        assert!(encoded.contains("musicd_library_watcher_pending_paths 4"));
        assert!(encoded.contains("musicd_library_watcher_native_active 1"));
        assert!(encoded.contains("musicd_stream_stalls_total{stage=\"socket_write\"} 1"));
        assert!(
            encoded.contains("musicd_renderer_health_transitions_total{outcome=\"stalled\"} 1")
        );
        assert!(encoded.contains("musicd_stream_transfers_total{outcome=\"completed\"} 1"));
        assert!(encoded.contains("musicd_stream_bytes_total 1048576"));
        assert!(encoded.contains("musicd_stream_transfer_duration_seconds_count 1"));
        assert!(encoded.contains("musicd_stream_max_file_read_duration_seconds_count 1"));
        assert!(encoded.contains("musicd_stream_max_socket_write_duration_seconds_count 1"));
    }
}
