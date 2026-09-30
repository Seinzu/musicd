//! Restricts which hosts UPnP HTTP requests may reach.
//!
//! Renderer locations reach the server from API clients and SSDP replies, and
//! control URLs come from device description XML, so none of them can be
//! trusted to point at a renderer. When the policy is enabled, requests are only
//! sent to private LAN addresses, which keeps the server from being used to
//! reach its own loopback services, cloud metadata endpoints, or the internet.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};

use reqwest::Url;

static LAN_ONLY: AtomicBool = AtomicBool::new(false);

/// Only allow UPnP requests to private LAN addresses from now on.
pub fn restrict_requests_to_lan() {
    LAN_ONLY.store(true, Ordering::Relaxed);
}

pub(crate) fn check_request_target(url: &str) -> io::Result<()> {
    if !LAN_ONLY.load(Ordering::Relaxed) {
        return Ok(());
    }
    check_lan_target(url)
}

/// Checks that `url` is an http(s) URL whose host is, or resolves only to,
/// private LAN addresses.
pub fn check_lan_target(url: &str) -> io::Result<()> {
    let parsed = Url::parse(url).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("renderer URL was invalid: {error}"),
        )
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(refused(
            url,
            "only http and https renderer URLs are allowed",
        ));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| refused(url, "renderer URL has no host"))?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(address) = host.parse::<IpAddr>() {
        return if is_lan_address(address) {
            Ok(())
        } else {
            Err(refused(url, "address is not on the local network"))
        };
    }

    let port = parsed.port_or_known_default().unwrap_or(80);
    let addresses = (host, port)
        .to_socket_addrs()
        .map_err(|error| refused(url, &format!("host did not resolve: {error}")))?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(refused(url, "host did not resolve"));
    }
    if addresses.iter().all(|address| is_lan_address(address.ip())) {
        Ok(())
    } else {
        Err(refused(url, "host resolves outside the local network"))
    }
}

fn refused(url: &str, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("refusing renderer request to {url}: {reason}"),
    )
}

pub(crate) fn is_lan_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_lan_ipv4(address),
        IpAddr::V6(address) => match address.to_ipv4_mapped() {
            Some(mapped) => is_lan_ipv4(mapped),
            None => is_lan_ipv6(address),
        },
    }
}

fn is_lan_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, ..] = address.octets();
    // Cloud metadata services live inside the link-local range.
    if address == Ipv4Addr::new(169, 254, 169, 254) {
        return false;
    }
    address.is_private()
        || address.is_link_local()
        // Carrier-grade NAT space, used by Tailscale and similar VPNs.
        || (first == 100 && (64..128).contains(&second))
}

fn is_lan_ipv6(address: Ipv6Addr) -> bool {
    let first = address.segments()[0];
    // Unique local (fc00::/7) and link-local (fe80::/10) addresses.
    (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
}

#[cfg(test)]
mod tests {
    use super::{check_lan_target, is_lan_address};
    use std::io;
    use std::net::IpAddr;

    fn lan(address: &str) -> bool {
        is_lan_address(address.parse::<IpAddr>().unwrap())
    }

    #[test]
    fn private_lan_addresses_are_allowed() {
        for address in [
            "192.168.1.55",
            "10.0.0.2",
            "172.16.4.1",
            "172.31.255.254",
            "169.254.10.1",
            "100.100.1.1",
            "fd12:3456::1",
            "fe80::1",
            "::ffff:192.168.1.5",
        ] {
            assert!(lan(address), "{address} should be allowed");
        }
    }

    #[test]
    fn loopback_public_and_metadata_addresses_are_refused() {
        for address in [
            "127.0.0.1",
            "0.0.0.0",
            "8.8.8.8",
            "172.32.0.1",
            "169.254.169.254",
            "224.0.0.251",
            "255.255.255.255",
            "::1",
            "::",
            "2001:4860::8888",
            "::ffff:127.0.0.1",
        ] {
            assert!(!lan(address), "{address} should be refused");
        }
    }

    #[test]
    fn check_lan_target_validates_scheme_and_host() {
        assert!(check_lan_target("http://192.168.1.55:49152/description.xml").is_ok());
        assert!(check_lan_target("http://[fe80::1]:49152/description.xml").is_ok());
        for url in [
            "http://127.0.0.1:8080/",
            "http://localhost:8080/",
            "http://169.254.169.254/latest/meta-data/",
            "file:///etc/passwd",
            "gopher://192.168.1.1/",
        ] {
            let error = check_lan_target(url).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied, "{url}");
        }
    }
}
