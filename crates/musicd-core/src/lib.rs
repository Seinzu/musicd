use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub album_id: String,
    pub artist_id: String,
    pub path: PathBuf,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Album {
    pub id: String,
    pub title: String,
    pub artist_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artist {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererProtocol {
    UpnpAvTransport,
    AirPlay2,
    Chromecast,
}

impl RendererProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::UpnpAvTransport => "UPnP AVTransport",
            Self::AirPlay2 => "AirPlay 2",
            Self::Chromecast => "Chromecast",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renderer {
    pub id: String,
    pub name: String,
    pub host: String,
    pub protocol: RendererProtocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub library_path: PathBuf,
    pub config_path: PathBuf,
    pub bind_address: String,
    pub base_url: String,
    pub discovery_timeout_ms: u64,
    pub default_renderer_location: Option<String>,
    pub debug_mode: bool,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            library_path: std::env::var("MUSICD_LIBRARY_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/music")),
            config_path: std::env::var("MUSICD_CONFIG_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/config")),
            bind_address: std::env::var("MUSICD_BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:7878".to_string()),
            base_url: std::env::var("MUSICD_PUBLIC_BASE_URL")
                .unwrap_or_else(|_| "http://192.168.1.10:7878".to_string()),
            discovery_timeout_ms: std::env::var("MUSICD_DISCOVERY_TIMEOUT_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1500),
            default_renderer_location: std::env::var("MUSICD_DEFAULT_RENDERER_LOCATION").ok(),
            debug_mode: parse_bool_env("MUSICD_DEBUG"),
        }
    }

    pub fn components(&self) -> [&'static str; 5] {
        [
            "NAS filesystem scanner",
            "metadata extraction pipeline",
            "SQLite library database",
            "HTTP stream server",
            "UPnP renderer adapter",
        ]
    }
}

fn parse_bool_env(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}
