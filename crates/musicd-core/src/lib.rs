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
    pub default_renderer_location: Option<String>,
    pub debug_mode: bool,
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

    pub fn resolved_base_url(&self) -> String {
        resolve_public_base_url(&self.base_url, &self.bind_address)
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
        .map(|(host, port)| (host.trim().trim_matches(['[', ']']).to_string(), port.trim().to_string()))
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
            default_renderer_location: None,
            debug_mode: false,
        };

        assert_eq!(config.resolved_base_url(), "http://192.168.1.20:8787");
    }
}
