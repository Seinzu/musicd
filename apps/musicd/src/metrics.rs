use std::cell::Cell;
use std::sync::Weak;
use std::time::Duration;

use prometheus_client::collector::Collector;
use prometheus_client::encoding::text::encode;
use prometheus_client::encoding::{DescriptorEncoder, EncodeLabelSet, EncodeMetric};
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::gauge::ConstGauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

use crate::ServiceState;

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

#[derive(Debug)]
pub struct Metrics {
    registry: Registry,
    request_count: Family<RequestLabels, Counter>,
    request_duration: Family<DurationLabels, Histogram, fn() -> Histogram>,
}

fn build_histogram() -> Histogram {
    Histogram::new([0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0].into_iter())
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

        registry.register_collector(Box::new(SnapshotCollector { state }));

        Self {
            registry,
            request_count,
            request_duration,
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
            crate::directory_metrics(&state.config.config_path.join("artwork")).unwrap_or((0, 0));

        let entries: [(&str, &str, i64); 9] = [
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
    "/api/now-playing",
    "/api/play",
    "/api/play-album",
    "/api/queue",
    "/api/queue/append-album",
    "/api/queue/append-track",
    "/api/queue/clear",
    "/api/queue/move",
    "/api/queue/play-next-album",
    "/api/queue/play-next-track",
    "/api/queue/remove",
    "/api/renderers",
    "/api/renderers/android-local/completed",
    "/api/renderers/android-local/session",
    "/api/renderers/discover",
    "/api/renderers/register-android-local",
    "/api/server",
    "/api/session",
    "/api/tracks",
    "/api/transport/next",
    "/api/transport/pause",
    "/api/transport/play",
    "/api/transport/previous",
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
