use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrackSummary {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    #[serde(default)]
    pub duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AlbumSummary {
    pub id: String,
    pub title: String,
    pub artist: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Renderer {
    pub location: String,
    pub name: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub health: Option<RendererHealth>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RendererHealth {
    #[serde(default)]
    pub reachable: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Queue {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub current_entry_id: Option<i64>,
    #[serde(default)]
    pub entries: Vec<QueueEntry>,
    #[serde(default)]
    pub session: Option<Session>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QueueEntry {
    pub id: i64,
    pub position: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub transport_state: String,
    #[serde(default)]
    pub position_seconds: Option<u64>,
    #[serde(default)]
    pub duration_seconds: Option<u64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
}

pub struct ApiClient {
    base_url: String,
    http: Client,
}

impl ApiClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .context("building HTTP client")?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn server_info(&self) -> Result<ServerInfo> {
        self.get_json("/api/server")
    }

    pub fn list_albums(&self) -> Result<Vec<AlbumSummary>> {
        self.get_json("/api/albums")
    }

    pub fn list_tracks(&self) -> Result<Vec<TrackSummary>> {
        self.get_json("/api/tracks")
    }

    pub fn list_renderers(&self) -> Result<Vec<Renderer>> {
        self.get_json("/api/renderers")
    }

    pub fn discover_renderers(&self) -> Result<Vec<Renderer>> {
        let url = format!("{}/api/renderers/discover", self.base_url);
        let res = self
            .http
            .post(&url)
            .send()
            .with_context(|| format!("POST {url}"))?
            .error_for_status()
            .with_context(|| format!("response from {url}"))?;
        res.json::<Vec<Renderer>>()
            .with_context(|| format!("parsing JSON from {url}"))
    }

    pub fn queue(&self, renderer_location: &str) -> Result<Queue> {
        let url = format!("{}/api/queue", self.base_url);
        let res = self
            .http
            .get(&url)
            .query(&[("renderer_location", renderer_location)])
            .send()
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("response from {url}"))?;
        res.json::<Queue>()
            .with_context(|| format!("parsing JSON from {url}"))
    }

    pub fn play_track(&self, renderer_location: &str, track_id: &str) -> Result<()> {
        self.post_form(
            "/api/play",
            &[
                ("renderer_location", renderer_location),
                ("track_id", track_id),
            ],
        )
    }

    pub fn play_album(&self, renderer_location: &str, album_id: &str) -> Result<()> {
        self.post_form(
            "/api/play-album",
            &[
                ("renderer_location", renderer_location),
                ("album_id", album_id),
            ],
        )
    }

    pub fn append_track(&self, renderer_location: &str, track_id: &str) -> Result<()> {
        self.post_form(
            "/api/queue/append-track",
            &[
                ("renderer_location", renderer_location),
                ("track_id", track_id),
            ],
        )
    }

    pub fn append_album(&self, renderer_location: &str, album_id: &str) -> Result<()> {
        self.post_form(
            "/api/queue/append-album",
            &[
                ("renderer_location", renderer_location),
                ("album_id", album_id),
            ],
        )
    }

    pub fn transport_play(&self, renderer_location: &str) -> Result<()> {
        self.post_form(
            "/api/transport/play",
            &[("renderer_location", renderer_location)],
        )
    }

    pub fn transport_pause(&self, renderer_location: &str) -> Result<()> {
        self.post_form(
            "/api/transport/pause",
            &[("renderer_location", renderer_location)],
        )
    }

    pub fn transport_stop(&self, renderer_location: &str) -> Result<()> {
        self.post_form(
            "/api/transport/stop",
            &[("renderer_location", renderer_location)],
        )
    }

    pub fn transport_next(&self, renderer_location: &str) -> Result<()> {
        self.post_form(
            "/api/transport/next",
            &[("renderer_location", renderer_location)],
        )
    }

    pub fn transport_previous(&self, renderer_location: &str) -> Result<()> {
        self.post_form(
            "/api/transport/previous",
            &[("renderer_location", renderer_location)],
        )
    }

    pub fn queue_clear(&self, renderer_location: &str) -> Result<()> {
        self.post_form(
            "/api/queue/clear",
            &[("renderer_location", renderer_location)],
        )
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let url = format!("{}{path}", self.base_url);
        let res = self
            .http
            .get(&url)
            .send()
            .with_context(|| format!("GET {url}"))?
            .error_for_status()
            .with_context(|| format!("response from {url}"))?;
        res.json::<T>()
            .with_context(|| format!("parsing JSON from {url}"))
    }

    fn post_form(&self, path: &str, params: &[(&str, &str)]) -> Result<()> {
        let url = format!("{}{path}", self.base_url);
        self.http
            .post(&url)
            .form(params)
            .send()
            .with_context(|| format!("POST {url}"))?
            .error_for_status()
            .with_context(|| format!("response from {url}"))?;
        Ok(())
    }
}
