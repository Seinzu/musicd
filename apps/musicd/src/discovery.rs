use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

use musicd_core::AppConfig;
use socket2::{Domain, Protocol, SockAddr, Socket, Type};

pub(crate) const MUSICD_SERVER_ST: &str = "urn:schemas-musicd-org:device:MusicdServer:1";
const MUSICD_SERVER_ALIAS_ST: &str = "musicd:server";
const SSDP_ADDR: &str = "239.255.255.250:1900";
const SSDP_MULTICAST: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiscoveredMusicdServer {
    pub(crate) location: String,
    pub(crate) base_url: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) usn: Option<String>,
    pub(crate) server: Option<String>,
}

pub(crate) fn spawn_server_discovery_advertiser(config: AppConfig) {
    if !config.server_discovery_enabled {
        eprintln!("musicd server discovery: disabled");
        return;
    }

    thread::spawn(move || {
        if let Err(error) = run_server_discovery_advertiser(config) {
            eprintln!("musicd server discovery stopped: {error}");
        }
    });
}

pub(crate) fn discover_musicd_servers(
    timeout: Duration,
) -> io::Result<Vec<DiscoveredMusicdServer>> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(Duration::from_millis(250)))?;

    let request = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_ADDR}\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: {MUSICD_SERVER_ST}\r\nUSER-AGENT: musicd/{} UPnP/1.1 Rust\r\n\r\n",
        env!("CARGO_PKG_VERSION")
    );
    socket.send_to(request.as_bytes(), SSDP_ADDR)?;

    let deadline = Instant::now() + timeout;
    let mut seen = HashMap::new();
    let mut buffer = [0_u8; 8192];

    while Instant::now() < deadline {
        match socket.recv_from(&mut buffer) {
            Ok((size, _addr)) => {
                let response = String::from_utf8_lossy(&buffer[..size]);
                if let Some(parsed) = parse_musicd_ssdp_response(&response) {
                    seen.entry(parsed.location.clone()).or_insert(parsed);
                }
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::TimedOut => {}
            Err(error) => return Err(error),
        }
    }

    Ok(seen.into_values().collect())
}

pub(crate) fn render_musicd_device_description_xml(config: &AppConfig) -> String {
    let base_url = config.resolved_base_url();
    let uuid = musicd_uuid(config);
    format!(
        r#"<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <specVersion>
    <major>1</major>
    <minor>0</minor>
  </specVersion>
  <URLBase>{}/</URLBase>
  <device>
    <deviceType>{}</deviceType>
    <friendlyName>{}</friendlyName>
    <manufacturer>Apodixis</manufacturer>
    <modelName>musicd server</modelName>
    <modelNumber>{}</modelNumber>
    <UDN>{}</UDN>
    <presentationURL>{}/</presentationURL>
  </device>
</root>"#,
        xml_escape(base_url.trim_end_matches('/')),
        MUSICD_SERVER_ST,
        xml_escape(&config.instance_name),
        env!("CARGO_PKG_VERSION"),
        uuid,
        xml_escape(base_url.trim_end_matches('/')),
    )
}

fn run_server_discovery_advertiser(config: AppConfig) -> io::Result<()> {
    let socket = multicast_socket()?;
    let base_url = config.resolved_base_url();
    eprintln!(
        "musicd server discovery: advertising {} at {}/description.xml",
        config.instance_name,
        base_url.trim_end_matches('/')
    );

    let mut buffer = [0_u8; 8192];
    loop {
        let (size, peer) = socket.recv_from(&mut buffer)?;
        let request = String::from_utf8_lossy(&buffer[..size]);
        let Some(response) = build_ssdp_response(&config, &request) else {
            continue;
        };
        if let Err(error) = socket.send_to(response.as_bytes(), peer) {
            eprintln!("musicd server discovery: response to {peer} failed: {error}");
        }
    }
}

fn multicast_socket() -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    {
        let _ = socket.set_reuse_port(true);
    }
    let address = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 1900);
    socket.bind(&SockAddr::from(address))?;
    socket.join_multicast_v4(&SSDP_MULTICAST, &Ipv4Addr::UNSPECIFIED)?;
    socket.set_read_timeout(None)?;
    Ok(socket.into())
}

fn build_ssdp_response(config: &AppConfig, request: &str) -> Option<String> {
    if !request
        .lines()
        .next()
        .map(|line| line.to_ascii_uppercase().starts_with("M-SEARCH"))
        .unwrap_or(false)
    {
        return None;
    }

    let headers = parse_header_lines(request.lines().skip(1));
    let man = headers.get("man").map(|value| value.to_ascii_lowercase());
    if !man
        .as_deref()
        .map(|value| value.contains("ssdp:discover"))
        .unwrap_or(false)
    {
        return None;
    }

    let requested_st = headers.get("st").map(String::as_str).unwrap_or("");
    if !matches_musicd_search_target(requested_st) {
        return None;
    }

    let response_st = if requested_st.eq_ignore_ascii_case("ssdp:all")
        || requested_st.eq_ignore_ascii_case("upnp:rootdevice")
    {
        MUSICD_SERVER_ST
    } else {
        requested_st
    };
    let base_url = config.resolved_base_url();
    let location = format!("{}/description.xml", base_url.trim_end_matches('/'));
    Some(format!(
        "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nEXT:\r\nLOCATION: {location}\r\nSERVER: musicd/{} UPnP/1.1 Rust\r\nST: {response_st}\r\nUSN: {}::{MUSICD_SERVER_ST}\r\nMUSICD-BASE-URL: {}\r\nMUSICD-NAME: {}\r\n\r\n",
        env!("CARGO_PKG_VERSION"),
        musicd_uuid(config),
        base_url.trim_end_matches('/'),
        config.instance_name,
    ))
}

fn matches_musicd_search_target(search_target: &str) -> bool {
    search_target.eq_ignore_ascii_case(MUSICD_SERVER_ST)
        || search_target.eq_ignore_ascii_case(MUSICD_SERVER_ALIAS_ST)
        || search_target.eq_ignore_ascii_case("ssdp:all")
        || search_target.eq_ignore_ascii_case("upnp:rootdevice")
}

fn parse_musicd_ssdp_response(response: &str) -> Option<DiscoveredMusicdServer> {
    let mut lines = response.lines();
    let status_line = lines.next()?.trim();
    if !status_line.starts_with("HTTP/1.1 200") && !status_line.starts_with("HTTP/1.0 200") {
        return None;
    }

    let headers = parse_header_lines(lines);
    let location = headers.get("location")?.to_string();
    let search_target = headers.get("st").map(String::as_str).unwrap_or("");
    let has_musicd_search_target = search_target.eq_ignore_ascii_case(MUSICD_SERVER_ST)
        || search_target.eq_ignore_ascii_case(MUSICD_SERVER_ALIAS_ST);
    let has_musicd_headers =
        headers.contains_key("musicd-base-url") || headers.contains_key("musicd-name");
    if !has_musicd_search_target && !has_musicd_headers {
        return None;
    }

    Some(DiscoveredMusicdServer {
        location,
        base_url: headers.get("musicd-base-url").cloned(),
        name: headers.get("musicd-name").cloned(),
        usn: headers.get("usn").cloned(),
        server: headers.get("server").cloned(),
    })
}

fn parse_header_lines<'a>(lines: impl Iterator<Item = &'a str>) -> HashMap<String, String> {
    lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect()
}

fn musicd_uuid(config: &AppConfig) -> String {
    let seed = format!("{}\0{}", config.instance_name, config.resolved_base_url());
    format!("uuid:musicd-{:016x}", stable_hash(seed.as_bytes()))
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::{
        MUSICD_SERVER_ST, build_ssdp_response, matches_musicd_search_target,
        parse_musicd_ssdp_response, render_musicd_device_description_xml,
    };
    use musicd_core::AppConfig;
    use std::path::PathBuf;

    fn config() -> AppConfig {
        AppConfig {
            instance_name: "kitchen musicd".to_string(),
            library_path: PathBuf::from("/music"),
            config_path: PathBuf::from("/config"),
            bind_address: "0.0.0.0:8787".to_string(),
            base_url: "http://192.168.1.50:8787".to_string(),
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
        }
    }

    #[test]
    fn recognises_musicd_search_targets() {
        assert!(matches_musicd_search_target(MUSICD_SERVER_ST));
        assert!(matches_musicd_search_target("musicd:server"));
        assert!(matches_musicd_search_target("ssdp:all"));
        assert!(!matches_musicd_search_target(
            "urn:schemas-upnp-org:device:MediaRenderer:1"
        ));
    }

    #[test]
    fn builds_musicd_ssdp_response() {
        let request = format!(
            "M-SEARCH * HTTP/1.1\r\nMAN: \"ssdp:discover\"\r\nST: {MUSICD_SERVER_ST}\r\n\r\n"
        );
        let response = build_ssdp_response(&config(), &request).expect("response");
        assert!(response.contains("LOCATION: http://192.168.1.50:8787/description.xml"));
        assert!(response.contains("MUSICD-BASE-URL: http://192.168.1.50:8787"));
        assert!(response.contains("MUSICD-NAME: kitchen musicd"));
    }

    #[test]
    fn parses_musicd_ssdp_response() {
        let response = format!(
            "HTTP/1.1 200 OK\r\nLOCATION: http://192.168.1.50:8787/description.xml\r\nST: {MUSICD_SERVER_ST}\r\nUSN: uuid:musicd-test::{MUSICD_SERVER_ST}\r\nMUSICD-BASE-URL: http://192.168.1.50:8787\r\nMUSICD-NAME: kitchen musicd\r\n\r\n"
        );
        let parsed = parse_musicd_ssdp_response(&response).expect("parsed response");
        assert_eq!(parsed.location, "http://192.168.1.50:8787/description.xml");
        assert_eq!(parsed.base_url.as_deref(), Some("http://192.168.1.50:8787"));
        assert_eq!(parsed.name.as_deref(), Some("kitchen musicd"));
    }

    #[test]
    fn ignores_non_musicd_upnp_responses() {
        let response = "HTTP/1.1 200 OK\r\nLOCATION: http://192.168.1.50/description.xml\r\nST: upnp:rootdevice\r\nSERVER: Hue/1.0 UPnP/1.0 IpBridge/1.76.0\r\nUSN: uuid:bridge::upnp:rootdevice\r\n\r\n";
        assert!(parse_musicd_ssdp_response(response).is_none());
    }

    #[test]
    fn renders_device_description() {
        let xml = render_musicd_device_description_xml(&config());
        assert!(
            xml.contains("<deviceType>urn:schemas-musicd-org:device:MusicdServer:1</deviceType>")
        );
        assert!(xml.contains("<friendlyName>kitchen musicd</friendlyName>"));
        assert!(xml.contains("<presentationURL>http://192.168.1.50:8787/</presentationURL>"));
    }
}
