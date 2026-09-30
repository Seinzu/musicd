//! Request checks that keep web pages on other sites from driving the server.
//!
//! These back up authentication (see `docs/authentication.md`): in `off` and
//! `optional` mode most routes are open to any browser on the LAN, and
//! `SameSite=Strict` cookies still reach musicd from other apps on the same
//! host, since browsers treat a different port as the same site. Two checks
//! stop other sites from borrowing a LAN browser:
//!
//! - DNS rebinding: a page on `evil.example` can point its own name at the
//!   server's LAN address and then read responses. Browsers always send that
//!   name as `Host`, so only IP addresses, local names and configured names are
//!   accepted.
//! - Cross-site requests: a page can submit forms or load images that hit
//!   state-changing routes. Browsers mark those with `Origin` and
//!   `Sec-Fetch-Site`, so writes carrying a foreign origin are refused.
//!
//! The Android app, `musicdctl` and renderers send neither `Origin` nor
//! `Sec-Fetch-Site`, so they are unaffected.

use std::net::IpAddr;

use musicd_core::AppConfig;

use super::HttpRequest;

/// Name suffixes that public DNS does not hand out, so an attacker cannot
/// point one at the server.
const LOCAL_NAME_SUFFIXES: &[&str] = &[
    ".local",
    ".lan",
    ".home",
    ".home.arpa",
    ".internal",
    ".localdomain",
];

/// GET routes that change server state. The web UI's older action routes
/// are POST-only now, but opening the scan progress feed still starts a scan.
/// Browsers send cross-site GETs freely (links, images), so these need the
/// same check as POSTs.
const STATE_CHANGING_GET_PATHS: &[&str] = &["/rescan-progress"];

/// Why a request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardRejection {
    UnknownHost,
    CrossSiteWrite,
}

impl GuardRejection {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::UnknownHost => {
                "unrecognised Host header; add this name to MUSICD_ALLOWED_HOSTS to allow it"
            }
            Self::CrossSiteWrite => "cross-site requests cannot change musicd state",
        }
    }
}

pub(crate) fn check_request(
    request: &HttpRequest,
    config: &AppConfig,
) -> Result<(), GuardRejection> {
    if let Some(host) = request.header("host")
        && !host_is_allowed(host, config)
    {
        return Err(GuardRejection::UnknownHost);
    }
    if changes_state(request) && is_cross_site(request, config) {
        return Err(GuardRejection::CrossSiteWrite);
    }
    Ok(())
}

fn changes_state(request: &HttpRequest) -> bool {
    match request.method.as_str() {
        "GET" | "HEAD" | "OPTIONS" => STATE_CHANGING_GET_PATHS.contains(&request.path.as_str()),
        _ => true,
    }
}

fn is_cross_site(request: &HttpRequest, config: &AppConfig) -> bool {
    if let Some(origin) = request.header("origin") {
        if origin_is_configured(origin, config) {
            return false;
        }
        let Some(host) = request.header("host") else {
            return true;
        };
        return !origin_matches_host(origin, host);
    }
    matches!(
        request
            .header("sec-fetch-site")
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("cross-site" | "same-site")
    )
}

/// A reverse proxy may rewrite `Host`, so an origin whose name the operator
/// configured is trusted even when it differs from `Host`.
fn origin_is_configured(origin: &str, config: &AppConfig) -> bool {
    let Some((_, authority)) = origin.trim().split_once("://") else {
        return false;
    };
    let (host, _) = split_host_port(authority);
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    config.allowed_hosts.contains(&host)
        || base_url_host(&config.base_url).is_some_and(|base_host| base_host == host)
}

fn origin_matches_host(origin: &str, host: &str) -> bool {
    let origin = origin.trim();
    let (scheme, authority) = match origin.split_once("://") {
        Some(parts) => parts,
        // Covers `Origin: null` from sandboxed frames and file:// pages.
        None => return false,
    };
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" => "80",
        "https" => "443",
        _ => return false,
    };
    normalized_authority(authority, default_port) == normalized_authority(host, default_port)
}

/// Lower-cases an authority and drops an explicit default port.
fn normalized_authority(authority: &str, default_port: &str) -> String {
    let authority = authority.trim().trim_end_matches('/').to_ascii_lowercase();
    let (host, port) = split_host_port(&authority);
    match port {
        Some(port) if port != default_port => format!("{host}:{port}"),
        _ => host.to_string(),
    }
}

fn split_host_port(authority: &str) -> (&str, Option<&str>) {
    if let Some(rest) = authority.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((host, tail)) => (host, tail.strip_prefix(':')),
            None => (authority, None),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, Some(port)),
        _ => (authority, None),
    }
}

fn host_is_allowed(host_header: &str, config: &AppConfig) -> bool {
    let (host, _) = split_host_port(host_header.trim());
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    // Rebinding needs a DNS name, so a literal address is always safe.
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    if host == "localhost" || !host.contains('.') {
        return true;
    }
    if LOCAL_NAME_SUFFIXES
        .iter()
        .any(|suffix| host.ends_with(suffix))
    {
        return true;
    }
    if config.allowed_hosts.contains(&host) {
        return true;
    }
    base_url_host(&config.base_url).is_some_and(|base_host| base_host == host)
}

fn base_url_host(base_url: &str) -> Option<String> {
    let (_, rest) = base_url.trim().split_once("://")?;
    let authority = rest.split('/').next()?;
    let (host, _) = split_host_port(authority);
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

#[cfg(test)]
mod tests {
    use super::{GuardRejection, check_request};
    use crate::http::HttpRequest;
    use musicd_core::AppConfig;

    fn config() -> AppConfig {
        let mut config = AppConfig::from_env();
        config.base_url = "http://music.example.net:8787".to_string();
        config.allowed_hosts = vec!["jukebox.example.org".to_string()];
        config
    }

    fn request(method: &str, path: &str, headers: &[(&str, &str)]) -> HttpRequest {
        HttpRequest {
            method: method.to_string(),
            target: path.to_string(),
            path: path.to_string(),
            headers: headers
                .iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value.to_string()))
                .collect(),
            ..HttpRequest::default()
        }
    }

    fn check(method: &str, path: &str, headers: &[(&str, &str)]) -> Result<(), GuardRejection> {
        check_request(&request(method, path, headers), &config())
    }

    #[test]
    fn accepts_addresses_local_names_and_configured_hosts() {
        for host in [
            "192.168.1.20:8787",
            "[fe80::1]:8787",
            "localhost:8787",
            "nas:8787",
            "musicd.local",
            "tower.home.arpa:8787",
            "music.example.net:8787",
            "JUKEBOX.example.org",
        ] {
            assert_eq!(
                check("GET", "/api/queue", &[("Host", host)]),
                Ok(()),
                "{host}"
            );
        }
        assert_eq!(check("GET", "/api/queue", &[]), Ok(()));
    }

    #[test]
    fn rejects_public_names_used_for_dns_rebinding() {
        for host in [
            "attacker.example.com",
            "rebind.it:8787",
            "localhost.evil.com",
        ] {
            assert_eq!(
                check("GET", "/api/play-history", &[("Host", host)]),
                Err(GuardRejection::UnknownHost),
                "{host}"
            );
        }
    }

    #[test]
    fn clients_without_browser_headers_can_write() {
        assert_eq!(
            check("POST", "/api/queue/clear", &[("Host", "192.168.1.20:8787")]),
            Ok(())
        );
        assert_eq!(
            check("GET", "/rescan-progress", &[("Host", "192.168.1.20:8787")]),
            Ok(())
        );
    }

    #[test]
    fn same_origin_browser_writes_are_allowed() {
        let host = ("Host", "192.168.1.20:8787");
        assert_eq!(
            check(
                "POST",
                "/api/play",
                &[host, ("Origin", "http://192.168.1.20:8787")]
            ),
            Ok(())
        );
        assert_eq!(
            check(
                "GET",
                "/rescan-progress",
                &[host, ("Sec-Fetch-Site", "same-origin")]
            ),
            Ok(())
        );
        assert_eq!(
            check(
                "GET",
                "/rescan-progress",
                &[host, ("Sec-Fetch-Site", "none")]
            ),
            Ok(())
        );
        assert_eq!(
            check(
                "POST",
                "/api/play",
                &[
                    ("Host", "musicd.local"),
                    ("Origin", "http://musicd.local:80")
                ]
            ),
            Ok(())
        );
    }

    #[test]
    fn cross_site_writes_are_rejected() {
        let host = ("Host", "192.168.1.20:8787");
        for headers in [
            vec![host, ("Origin", "https://evil.example.com")],
            vec![host, ("Origin", "http://192.168.1.20:9999")],
            vec![host, ("Origin", "null")],
            vec![host, ("Sec-Fetch-Site", "cross-site")],
            vec![host, ("Sec-Fetch-Site", "same-site")],
            vec![("Origin", "http://192.168.1.20:8787")],
        ] {
            assert_eq!(
                check("POST", "/api/queue/clear", &headers),
                Err(GuardRejection::CrossSiteWrite),
                "{headers:?}"
            );
        }
        assert_eq!(
            check(
                "GET",
                "/rescan-progress",
                &[host, ("Sec-Fetch-Site", "cross-site")]
            ),
            Err(GuardRejection::CrossSiteWrite)
        );
    }

    #[test]
    fn cross_site_reads_are_left_to_the_browser() {
        assert_eq!(
            check(
                "GET",
                "/album/abc",
                &[
                    ("Host", "192.168.1.20:8787"),
                    ("Sec-Fetch-Site", "cross-site")
                ]
            ),
            Ok(())
        );
    }

    #[test]
    fn configured_origins_are_trusted_behind_a_proxy() {
        let host = ("Host", "192.168.1.20:8787");
        for origin in [
            "https://jukebox.example.org",
            "http://music.example.net:8787",
        ] {
            assert_eq!(
                check("POST", "/api/play", &[host, ("Origin", origin)]),
                Ok(()),
                "{origin}"
            );
        }
    }
}
