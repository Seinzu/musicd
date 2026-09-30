use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
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
    pub queue_entry_id: Option<i64>,
    #[serde(default)]
    pub transport_state: String,
    #[serde(default)]
    pub current_track_uri: Option<String>,
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
    client_id: String,
    http: Client,
}

impl ApiClient {
    pub fn new(base_url: &str, client_id: &str, token: Option<&str>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        if let Some(token) = token {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .context("API token contains characters that can't go in a header")?;
            value.set_sensitive(true);
            headers.insert(AUTHORIZATION, value);
        }
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .default_headers(headers)
            .build()
            .context("building HTTP client")?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client_id: client_id.to_string(),
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
            .form(&[("client_id", self.client_id.as_str())])
            .send()
            .with_context(|| format!("POST {url}"))
            .and_then(|res| check_status(res, &url))?;
        res.json::<Vec<Renderer>>()
            .with_context(|| format!("parsing JSON from {url}"))
    }

    pub fn register_cli_local_renderer(&self, name: &str) -> Result<()> {
        let renderer_location = self.cli_local_renderer_location();
        self.post_form(
            "/api/renderers/register-cli-local",
            &[
                ("renderer_location", renderer_location.as_str()),
                ("name", name),
                ("visibility", "private"),
            ],
        )
    }

    pub fn cli_local_renderer_location(&self) -> String {
        format!("cli-local://{}", self.client_id)
    }

    pub fn report_cli_local_session(
        &self,
        renderer_location: &str,
        transport_state: &str,
        current_track_uri: Option<&str>,
        duration_seconds: Option<u64>,
    ) -> Result<()> {
        let mut form = vec![
            ("renderer_location", renderer_location),
            ("transport_state", transport_state),
        ];
        if let Some(uri) = current_track_uri {
            form.push(("current_track_uri", uri));
        }
        let duration;
        if let Some(value) = duration_seconds {
            duration = value.to_string();
            form.push(("duration_seconds", duration.as_str()));
        }
        self.post_form("/api/renderers/cli-local/session", &form)
    }

    pub fn report_cli_local_completed(&self, renderer_location: &str) -> Result<()> {
        self.post_form(
            "/api/renderers/cli-local/completed",
            &[("renderer_location", renderer_location)],
        )
    }

    pub fn queue(&self, renderer_location: &str) -> Result<Queue> {
        let url = format!("{}/api/queue", self.base_url);
        let res = self
            .http
            .get(&url)
            .query(&[
                ("renderer_location", renderer_location),
                ("client_id", self.client_id.as_str()),
            ])
            .send()
            .with_context(|| format!("GET {url}"))
            .and_then(|res| check_status(res, &url))?;
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
            .query(&[("client_id", self.client_id.as_str())])
            .send()
            .with_context(|| format!("GET {url}"))
            .and_then(|res| check_status(res, &url))?;
        res.json::<T>()
            .with_context(|| format!("parsing JSON from {url}"))
    }

    fn post_form(&self, path: &str, params: &[(&str, &str)]) -> Result<()> {
        let url = format!("{}{path}", self.base_url);
        let mut form = Vec::with_capacity(params.len() + 1);
        form.push(("client_id", self.client_id.as_str()));
        form.extend_from_slice(params);
        self.http
            .post(&url)
            .form(&form)
            .send()
            .with_context(|| format!("POST {url}"))
            .and_then(|res| check_status(res, &url))?;
        Ok(())
    }
}

/// Like `error_for_status`, but explains auth failures.
fn check_status(res: Response, url: &str) -> Result<Response> {
    match res.status() {
        StatusCode::UNAUTHORIZED => {
            bail!(
                "{url}: the server needs authentication. Run `musicdctl pair` to pair this computer"
            )
        }
        StatusCode::FORBIDDEN => bail!("{url}: this device's token isn't allowed to do that"),
        _ => res
            .error_for_status()
            .with_context(|| format!("response from {url}")),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PairingStart {
    pub pairing_id: String,
    pub code: String,
    pub expires_in: u64,
    pub poll_interval: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingPoll {
    Pending,
    Approved(String),
    Denied,
    /// Expired, unknown, or already collected.
    Gone,
}

#[derive(Deserialize)]
struct PairingPollBody {
    status: String,
    #[serde(default)]
    token: Option<String>,
}

/// Unauthenticated calls used to obtain a token in the first place.
pub struct PairingClient {
    base_url: String,
    http: Client,
}

impl PairingClient {
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

    pub fn start(&self, device_name: &str) -> Result<PairingStart> {
        let url = format!("{}/api/pair/start", self.base_url);
        let res = self
            .http
            .post(&url)
            .form(&[("name", device_name)])
            .send()
            .with_context(|| format!("POST {url}"))?;
        if res.status() == StatusCode::NOT_FOUND {
            bail!(
                "{} doesn't support pairing; update the server",
                self.base_url
            );
        }
        res.error_for_status()
            .with_context(|| format!("response from {url}"))?
            .json::<PairingStart>()
            .with_context(|| format!("parsing JSON from {url}"))
    }

    pub fn poll(&self, pairing_id: &str) -> Result<PairingPoll> {
        let url = format!("{}/api/pair/poll", self.base_url);
        let res = self
            .http
            .post(&url)
            .form(&[("pairing_id", pairing_id)])
            .send()
            .with_context(|| format!("POST {url}"))?;
        if res.status() == StatusCode::NOT_FOUND {
            return Ok(PairingPoll::Gone);
        }
        let body = res
            .error_for_status()
            .with_context(|| format!("response from {url}"))?
            .json::<PairingPollBody>()
            .with_context(|| format!("parsing JSON from {url}"))?;
        Ok(parse_poll(body))
    }
}

fn parse_poll(body: PairingPollBody) -> PairingPoll {
    match (body.status.as_str(), body.token) {
        ("approved", Some(token)) if !token.is_empty() => PairingPoll::Approved(token),
        ("pending", _) => PairingPoll::Pending,
        ("denied", _) => PairingPoll::Denied,
        _ => PairingPoll::Gone,
    }
}

#[cfg(test)]
mod tests {
    use super::{PairingPoll, PairingPollBody, parse_poll};

    fn body(json: &str) -> PairingPollBody {
        serde_json::from_str(json).expect("valid JSON")
    }

    #[test]
    fn parses_poll_responses() {
        assert_eq!(
            parse_poll(body(r#"{"ok":true,"status":"pending"}"#)),
            PairingPoll::Pending
        );
        assert_eq!(
            parse_poll(body(r#"{"ok":true,"status":"approved","token":"mdt_x"}"#)),
            PairingPoll::Approved("mdt_x".to_string())
        );
        assert_eq!(
            parse_poll(body(r#"{"ok":true,"status":"approved"}"#)),
            PairingPoll::Gone
        );
        assert_eq!(
            parse_poll(body(r#"{"ok":true,"status":"denied"}"#)),
            PairingPoll::Denied
        );
    }
}
