use std::net::UdpSocket;
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
    pub instance_name: String,
    pub library_path: PathBuf,
    pub config_path: PathBuf,
    pub bind_address: String,
    pub base_url: String,
    pub discovery_timeout_ms: u64,
    pub server_discovery_enabled: bool,
    pub default_renderer_location: Option<String>,
    pub radio_browser_base_url: String,
    pub debug_mode: bool,
    pub skip_startup_scan: bool,
    pub native_next_preload_enabled: bool,
    pub native_next_preload_playlist_extension_enabled: bool,
    pub library_watch_enabled: bool,
    pub library_watch_interval_ms: u64,
    pub library_watch_settle_ms: u64,
    pub tidal_helper_command: Option<String>,
    pub tidal_session_path: PathBuf,
    pub tidal_audio_quality: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            instance_name: std::env::var("MUSICD_INSTANCE_NAME")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "musicd".to_string()),
            library_path: std::env::var("MUSICD_LIBRARY_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/music")),
            config_path: std::env::var("MUSICD_CONFIG_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/config")),
            bind_address: std::env::var("MUSICD_BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:7878".to_string()),
            base_url: std::env::var("MUSICD_PUBLIC_BASE_URL")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "auto".to_string()),
            discovery_timeout_ms: std::env::var("MUSICD_DISCOVERY_TIMEOUT_MS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1500),
            server_discovery_enabled: parse_bool_env_default("MUSICD_SERVER_DISCOVERY", true),
            default_renderer_location: std::env::var("MUSICD_DEFAULT_RENDERER_LOCATION").ok(),
            radio_browser_base_url: std::env::var("MUSICD_RADIO_BROWSER_BASE_URL")
                .ok()
                .map(|value| value.trim().trim_end_matches('/').to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "https://de1.api.radio-browser.info".to_string()),
            debug_mode: parse_bool_env("MUSICD_DEBUG"),
            skip_startup_scan: parse_bool_env("MUSICD_SKIP_STARTUP_SCAN"),
            native_next_preload_enabled: parse_bool_env("MUSICD_NATIVE_NEXT_PRELOAD"),
            native_next_preload_playlist_extension_enabled: parse_bool_env(
                "MUSICD_NATIVE_NEXT_PRELOAD_PLAYLIST_EXTENSION",
            ),
            library_watch_enabled: parse_bool_env_default("MUSICD_LIBRARY_WATCH", true),
            library_watch_interval_ms: parse_u64_env("MUSICD_LIBRARY_WATCH_INTERVAL_MS", 10_000),
            library_watch_settle_ms: parse_u64_env("MUSICD_LIBRARY_WATCH_SETTLE_MS", 3_000),
            tidal_helper_command: std::env::var("MUSICD_TIDAL_HELPER_COMMAND")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            tidal_session_path: std::env::var("MUSICD_TIDAL_SESSION_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    std::env::var("MUSICD_CONFIG_PATH")
                        .map(PathBuf::from)
                        .unwrap_or_else(|_| PathBuf::from("/config"))
                        .join("tidal")
                        .join("session.json")
                }),
            tidal_audio_quality: std::env::var("MUSICD_TIDAL_AUDIO_QUALITY")
                .ok()
                .map(|value| value.trim().to_ascii_uppercase())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "LOSSLESS".to_string()),
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

    pub fn resolved_base_url(&self) -> String {
        resolve_public_base_url(&self.base_url, &self.bind_address)
    }
}

fn parse_bool_env(name: &str) -> bool {
    parse_bool_env_default(name, false)
}

fn parse_bool_env_default(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

fn parse_u64_env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn resolve_public_base_url(configured_base_url: &str, bind_address: &str) -> String {
    let trimmed = configured_base_url.trim().trim_end_matches('/');
    if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("auto") {
        return trimmed.to_string();
    }

    let (bind_host, bind_port) = split_bind_address(bind_address);
    let host = if bind_host.is_empty() || is_unspecified_or_loopback_host(&bind_host) {
        detect_local_lan_ip().unwrap_or_else(|| "127.0.0.1".to_string())
    } else {
        bind_host
    };

    format!("http://{}:{bind_port}", format_host_for_url(&host))
}

fn split_bind_address(bind_address: &str) -> (String, String) {
    bind_address
        .trim()
        .rsplit_once(':')
        .map(|(host, port)| {
            (
                host.trim().trim_matches(['[', ']']).to_string(),
                port.trim().to_string(),
            )
        })
        .unwrap_or_else(|| ("0.0.0.0".to_string(), "7878".to_string()))
}

fn is_unspecified_or_loopback_host(host: &str) -> bool {
    matches!(
        host.trim().to_ascii_lowercase().as_str(),
        "" | "0.0.0.0" | "::" | "::1" | "127.0.0.1" | "localhost"
    )
}

fn detect_local_lan_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_loopback()).then(|| ip.to_string())
}

fn format_host_for_url(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{AppConfig, resolve_public_base_url};
    use std::path::PathBuf;

    #[test]
    fn keeps_explicit_public_base_url() {
        assert_eq!(
            resolve_public_base_url("http://musicd.local:8787", "0.0.0.0:8787"),
            "http://musicd.local:8787"
        );
    }

    #[test]
    fn resolved_base_url_uses_bind_host_when_specific() {
        let config = AppConfig {
            instance_name: "musicd".to_string(),
            library_path: PathBuf::from("/music"),
            config_path: PathBuf::from("/config"),
            bind_address: "192.168.1.20:8787".to_string(),
            base_url: "auto".to_string(),
            discovery_timeout_ms: 1500,
            server_discovery_enabled: true,
            default_renderer_location: None,
            radio_browser_base_url: "https://de1.api.radio-browser.info".to_string(),
            debug_mode: false,
            skip_startup_scan: false,
            native_next_preload_enabled: false,
            native_next_preload_playlist_extension_enabled: false,
            library_watch_enabled: true,
            library_watch_interval_ms: 10_000,
            library_watch_settle_ms: 3_000,
            tidal_helper_command: None,
            tidal_session_path: PathBuf::from("/config/tidal/session.json"),
            tidal_audio_quality: "LOSSLESS".to_string(),
        };

        assert_eq!(config.resolved_base_url(), "http://192.168.1.20:8787");
    }
}
