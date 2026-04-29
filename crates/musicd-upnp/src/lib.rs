use std::collections::HashMap;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

const MEDIA_RENDERER_ST: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
const AV_TRANSPORT_SERVICE: &str = "urn:schemas-upnp-org:service:AVTransport:1";
const RENDERING_CONTROL_SERVICE: &str = "urn:schemas-upnp-org:service:RenderingControl:1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamResource {
    pub stream_url: String,
    pub mime_type: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryResponse {
    pub location: String,
    pub server: Option<String>,
    pub search_target: Option<String>,
    pub usn: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpnpService {
    pub service_type: String,
    pub service_id: Option<String>,
    pub control_url: String,
    pub event_sub_url: Option<String>,
    pub scpd_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDescription {
    pub location: String,
    pub url_base: String,
    pub friendly_name: String,
    pub device_type: String,
    pub manufacturer: Option<String>,
    pub model_name: Option<String>,
    pub services: Vec<UpnpService>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererDescription {
    pub location: String,
    pub friendly_name: String,
    pub device_type: String,
    pub manufacturer: Option<String>,
    pub model_name: Option<String>,
    pub av_transport_control_url: String,
    pub rendering_control_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportInfo {
    pub transport_state: String,
    pub transport_status: Option<String>,
    pub current_speed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionInfo {
    pub track_uri: Option<String>,
    pub rel_time_seconds: Option<u64>,
    pub track_duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportSnapshot {
    pub transport_info: TransportInfo,
    pub position_info: PositionInfo,
}

impl DeviceDescription {
    pub fn find_service(&self, service_type: &str) -> Option<&UpnpService> {
        self.services
            .iter()
            .find(|service| service.service_type == service_type)
    }
}

impl RendererDescription {
    pub fn from_device(device: DeviceDescription) -> io::Result<Self> {
        let av_transport_control_url = device
            .find_service(AV_TRANSPORT_SERVICE)
            .map(|service| service.control_url.clone())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "device description did not contain AVTransport service",
                )
            })?;

        let rendering_control_url = device
            .find_service(RENDERING_CONTROL_SERVICE)
            .map(|service| service.control_url.clone());

        Ok(Self {
            location: device.location,
            friendly_name: device.friendly_name,
            device_type: device.device_type,
            manufacturer: device.manufacturer,
            model_name: device.model_name,
            av_transport_control_url,
            rendering_control_url,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpUrl {
    host: String,
    port: u16,
    path_and_query: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpResponse {
    status_code: u16,
    reason_phrase: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

pub fn discover_renderers(timeout: Duration) -> io::Result<Vec<DiscoveryResponse>> {
    let socket = UdpSocket::bind("0.0.0.0:0")?;
    socket.set_read_timeout(Some(Duration::from_millis(250)))?;

    let request = format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: {MEDIA_RENDERER_ST}\r\nUSER-AGENT: musicd/0.1 UPnP/1.1 Rust/1.94\r\n\r\n"
    );

    socket.send_to(request.as_bytes(), "239.255.255.250:1900")?;

    let deadline = Instant::now() + timeout;
    let mut seen = HashMap::new();
    let mut buffer = [0_u8; 8192];

    while Instant::now() < deadline {
        match socket.recv_from(&mut buffer) {
            Ok((size, _addr)) => {
                let response = String::from_utf8_lossy(&buffer[..size]);
                if let Some(parsed) = parse_ssdp_response(&response) {
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

pub fn fetch_device_description(location: &str) -> io::Result<DeviceDescription> {
    let response = http_request("GET", location, &[("Accept", "application/xml")], None)?;
    if response.status_code != 200 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "device description request failed with HTTP {} {}",
                response.status_code, response.reason_phrase
            ),
        ));
    }

    let body = String::from_utf8(response.body).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("device description body was not valid UTF-8: {error}"),
        )
    })?;

    parse_device_description(location, &body)
}

pub fn inspect_renderer(location: &str) -> io::Result<RendererDescription> {
    RendererDescription::from_device(fetch_device_description(location)?)
}

pub fn set_av_transport_uri(control_url: &str, resource: &StreamResource) -> io::Result<()> {
    let body = build_set_av_transport_uri_envelope(
        0,
        &resource.stream_url,
        &resource.mime_type,
        Some(&resource.title),
    );
    let response = http_request(
        "POST",
        control_url,
        &[
            ("Content-Type", "text/xml; charset=\"utf-8\""),
            (
                "SOAPACTION",
                "\"urn:schemas-upnp-org:service:AVTransport:1#SetAVTransportURI\"",
            ),
        ],
        Some(body.as_bytes()),
    )?;

    expect_successful_soap("SetAVTransportURI", response)
}

pub fn set_next_av_transport_uri(control_url: &str, resource: &StreamResource) -> io::Result<()> {
    let body = build_set_next_av_transport_uri_envelope(
        0,
        &resource.stream_url,
        &resource.mime_type,
        Some(&resource.title),
    );
    let response = http_request(
        "POST",
        control_url,
        &[
            ("Content-Type", "text/xml; charset=\"utf-8\""),
            (
                "SOAPACTION",
                "\"urn:schemas-upnp-org:service:AVTransport:1#SetNextAVTransportURI\"",
            ),
        ],
        Some(body.as_bytes()),
    )?;

    expect_successful_soap("SetNextAVTransportURI", response)
}

pub fn play(control_url: &str) -> io::Result<()> {
    let body = build_play_envelope(0, 1);
    let response = av_transport_action(control_url, "Play", body.as_bytes())?;

    if expect_successful_soap("Play", response.clone()).is_ok() {
        return Ok(());
    }

    if is_transition_not_available_fault(&response) {
        std::thread::sleep(Duration::from_millis(250));
        if transport_is_starting_or_playing(control_url) {
            return Ok(());
        }

        let retry = http_request(
            "POST",
            control_url,
            &[
                ("Content-Type", "text/xml; charset=\"utf-8\""),
                (
                    "SOAPACTION",
                    "\"urn:schemas-upnp-org:service:AVTransport:1#Play\"",
                ),
            ],
            Some(body.as_bytes()),
        )?;
        if expect_successful_soap("Play", retry.clone()).is_ok()
            || (is_transition_not_available_fault(&retry)
                && transport_is_starting_or_playing(control_url))
        {
            return Ok(());
        }

        return expect_successful_soap("Play", retry);
    }

    expect_successful_soap("Play", response)
}

pub fn pause(control_url: &str) -> io::Result<()> {
    let body = build_pause_envelope(0);
    let response = av_transport_action(control_url, "Pause", body.as_bytes())?;
    expect_successful_soap("Pause", response)
}

pub fn stop(control_url: &str) -> io::Result<()> {
    let body = build_stop_envelope(0);
    let response = av_transport_action(control_url, "Stop", body.as_bytes())?;
    expect_successful_soap("Stop", response)
}

pub fn next(control_url: &str) -> io::Result<()> {
    let body = build_next_envelope(0);
    let response = av_transport_action(control_url, "Next", body.as_bytes())?;
    expect_successful_soap("Next", response)
}

pub fn previous(control_url: &str) -> io::Result<()> {
    let body = build_previous_envelope(0);
    let response = av_transport_action(control_url, "Previous", body.as_bytes())?;
    expect_successful_soap("Previous", response)
}

pub fn get_transport_info(control_url: &str) -> io::Result<TransportInfo> {
    let body = build_get_transport_info_envelope(0);
    let response = http_request(
        "POST",
        control_url,
        &[
            ("Content-Type", "text/xml; charset=\"utf-8\""),
            (
                "SOAPACTION",
                "\"urn:schemas-upnp-org:service:AVTransport:1#GetTransportInfo\"",
            ),
        ],
        Some(body.as_bytes()),
    )?;
    expect_successful_soap("GetTransportInfo", response.clone())?;
    parse_transport_info_response(&response.body)
}

pub fn get_position_info(control_url: &str) -> io::Result<PositionInfo> {
    let body = build_get_position_info_envelope(0);
    let response = http_request(
        "POST",
        control_url,
        &[
            ("Content-Type", "text/xml; charset=\"utf-8\""),
            (
                "SOAPACTION",
                "\"urn:schemas-upnp-org:service:AVTransport:1#GetPositionInfo\"",
            ),
        ],
        Some(body.as_bytes()),
    )?;
    expect_successful_soap("GetPositionInfo", response.clone())?;
    parse_position_info_response(&response.body)
}

pub fn get_transport_snapshot(control_url: &str) -> io::Result<TransportSnapshot> {
    Ok(TransportSnapshot {
        transport_info: get_transport_info(control_url)?,
        position_info: get_position_info(control_url)?,
    })
}

pub fn play_stream(
    renderer_location: &str,
    resource: &StreamResource,
) -> io::Result<RendererDescription> {
    let renderer = inspect_renderer(renderer_location)?;
    set_av_transport_uri(&renderer.av_transport_control_url, resource)?;
    play(&renderer.av_transport_control_url)?;
    Ok(renderer)
}

pub fn build_set_av_transport_uri_envelope(
    instance_id: u32,
    stream_url: &str,
    mime_type: &str,
    title: Option<&str>,
) -> String {
    let metadata = didl_lite_metadata(stream_url, mime_type, title.unwrap_or("Unknown title"));

    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:SetAVTransportURI xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID><CurrentURI>{}</CurrentURI><CurrentURIMetaData>{}</CurrentURIMetaData></u:SetAVTransportURI></s:Body></s:Envelope>"#,
        xml_escape(stream_url),
        xml_escape(&metadata),
    )
}

pub fn build_set_next_av_transport_uri_envelope(
    instance_id: u32,
    stream_url: &str,
    mime_type: &str,
    title: Option<&str>,
) -> String {
    let metadata = didl_lite_metadata(stream_url, mime_type, title.unwrap_or("Unknown title"));
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:SetNextAVTransportURI xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID><NextURI>{}</NextURI><NextURIMetaData>{}</NextURIMetaData></u:SetNextAVTransportURI></s:Body></s:Envelope>"#,
        xml_escape(stream_url),
        xml_escape(&metadata),
    )
}

pub fn build_play_envelope(instance_id: u32, speed: u8) -> String {
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:Play xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID><Speed>{speed}</Speed></u:Play></s:Body></s:Envelope>"#
    )
}

pub fn build_pause_envelope(instance_id: u32) -> String {
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:Pause xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID></u:Pause></s:Body></s:Envelope>"#
    )
}

pub fn build_stop_envelope(instance_id: u32) -> String {
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:Stop xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID></u:Stop></s:Body></s:Envelope>"#
    )
}

pub fn build_next_envelope(instance_id: u32) -> String {
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:Next xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID></u:Next></s:Body></s:Envelope>"#
    )
}

pub fn build_previous_envelope(instance_id: u32) -> String {
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:Previous xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID></u:Previous></s:Body></s:Envelope>"#
    )
}

pub fn build_get_transport_info_envelope(instance_id: u32) -> String {
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetTransportInfo xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID></u:GetTransportInfo></s:Body></s:Envelope>"#
    )
}

pub fn build_get_position_info_envelope(instance_id: u32) -> String {
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetPositionInfo xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID></u:GetPositionInfo></s:Body></s:Envelope>"#
    )
}

fn expect_successful_soap(action: &str, response: HttpResponse) -> io::Result<()> {
    if (200..300).contains(&response.status_code) {
        return Ok(());
    }

    let preview = String::from_utf8_lossy(&response.body);
    Err(io::Error::new(
        io::ErrorKind::Other,
        format!(
            "{action} failed with HTTP {} {}: {}",
            response.status_code,
            response.reason_phrase,
            preview.trim()
        ),
    ))
}

fn av_transport_action(control_url: &str, action: &str, body: &[u8]) -> io::Result<HttpResponse> {
    let soap_action = format!("\"urn:schemas-upnp-org:service:AVTransport:1#{action}\"");
    http_request(
        "POST",
        control_url,
        &[
            ("Content-Type", "text/xml; charset=\"utf-8\""),
            ("SOAPACTION", soap_action.as_str()),
        ],
        Some(body),
    )
}

fn transport_is_starting_or_playing(control_url: &str) -> bool {
    matches!(
        get_transport_info(control_url)
            .map(|info| info.transport_state)
            .ok()
            .as_deref(),
        Some("PLAYING" | "TRANSITIONING")
    )
}

fn is_transition_not_available_fault(response: &HttpResponse) -> bool {
    if !(400..600).contains(&response.status_code) {
        return false;
    }

    let body = match std::str::from_utf8(&response.body) {
        Ok(body) => body,
        Err(_) => return false,
    };

    matches!(extract_first_tag(body, "errorCode"), Some("701"))
        || matches!(
            extract_first_tag(body, "errorDescription"),
            Some("Transition not available")
        )
}

fn parse_ssdp_response(response: &str) -> Option<DiscoveryResponse> {
    let mut lines = response.lines();
    let status_line = lines.next()?.trim();
    if !status_line.starts_with("HTTP/1.1 200") && !status_line.starts_with("HTTP/1.0 200") {
        return None;
    }

    let headers = parse_header_lines(lines);
    let location = headers.get("location")?.to_string();

    Some(DiscoveryResponse {
        location,
        server: headers.get("server").cloned(),
        search_target: headers.get("st").cloned(),
        usn: headers.get("usn").cloned(),
    })
}

fn parse_transport_info_response(body: &[u8]) -> io::Result<TransportInfo> {
    let xml = String::from_utf8(body.to_vec()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("transport info body was not valid UTF-8: {error}"),
        )
    })?;
    let transport_state = extract_first_tag(&xml, "CurrentTransportState")
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "transport info response was missing CurrentTransportState",
            )
        })?
        .trim()
        .to_string();
    let transport_status = extract_first_tag(&xml, "CurrentTransportStatus")
        .map(str::trim)
        .map(str::to_string);
    let current_speed = extract_first_tag(&xml, "CurrentSpeed")
        .map(str::trim)
        .map(str::to_string);

    Ok(TransportInfo {
        transport_state,
        transport_status,
        current_speed,
    })
}

fn parse_position_info_response(body: &[u8]) -> io::Result<PositionInfo> {
    let xml = String::from_utf8(body.to_vec()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("position info body was not valid UTF-8: {error}"),
        )
    })?;

    let track_uri = extract_first_tag(&xml, "TrackURI")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let rel_time_seconds = extract_first_tag(&xml, "RelTime").and_then(parse_upnp_time);
    let track_duration_seconds = extract_first_tag(&xml, "TrackDuration").and_then(parse_upnp_time);

    Ok(PositionInfo {
        track_uri,
        rel_time_seconds,
        track_duration_seconds,
    })
}

fn parse_device_description(location: &str, xml: &str) -> io::Result<DeviceDescription> {
    let url_base = extract_first_tag(xml, "URLBase")
        .map(str::to_string)
        .unwrap_or_else(|| base_url(location));

    let device_section = extract_first_tag(xml, "device").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "device description was missing a <device> section",
        )
    })?;

    let friendly_name = extract_first_tag(device_section, "friendlyName")
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "device description was missing <friendlyName>",
            )
        })?
        .trim()
        .to_string();

    let device_type = extract_first_tag(device_section, "deviceType")
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "device description was missing <deviceType>",
            )
        })?
        .trim()
        .to_string();

    let manufacturer = extract_first_tag(device_section, "manufacturer")
        .map(str::trim)
        .map(str::to_string);
    let model_name = extract_first_tag(device_section, "modelName")
        .map(str::trim)
        .map(str::to_string);

    let service_list = extract_first_tag(device_section, "serviceList").unwrap_or("");
    let services = extract_all_tag_blocks(service_list, "service")
        .into_iter()
        .filter_map(|service_xml| {
            let service_type = extract_first_tag(service_xml, "serviceType")?
                .trim()
                .to_string();
            let control_url = extract_first_tag(service_xml, "controlURL")?
                .trim()
                .to_string();
            let service_id = extract_first_tag(service_xml, "serviceId")
                .map(str::trim)
                .map(str::to_string);
            let event_sub_url = extract_first_tag(service_xml, "eventSubURL")
                .map(str::trim)
                .map(str::to_string);
            let scpd_url = extract_first_tag(service_xml, "SCPDURL")
                .map(str::trim)
                .map(str::to_string);

            Some(UpnpService {
                service_type,
                service_id,
                control_url: resolve_url(&url_base, &control_url),
                event_sub_url: event_sub_url.map(|value| resolve_url(&url_base, &value)),
                scpd_url: scpd_url.map(|value| resolve_url(&url_base, &value)),
            })
        })
        .collect();

    Ok(DeviceDescription {
        location: location.to_string(),
        url_base,
        friendly_name,
        device_type,
        manufacturer,
        model_name,
        services,
    })
}

fn http_request(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> io::Result<HttpResponse> {
    let parsed = parse_http_url(url)?;
    let mut request = format!(
        "{method} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: musicd/0.1\r\n",
        parsed.path_and_query, parsed.host
    );

    for (name, value) in headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }

    let body = body.unwrap_or(&[]);
    request.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));

    let mut stream = open_tcp_stream(&parsed)?;
    stream.write_all(request.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(body)?;
    }
    stream.flush()?;

    let mut response_bytes = Vec::new();
    stream.read_to_end(&mut response_bytes)?;
    parse_http_response(&response_bytes)
}

fn open_tcp_stream(url: &HttpUrl) -> io::Result<TcpStream> {
    let address = format!("{}:{}", url.host, url.port);
    let mut addrs = address.to_socket_addrs()?;
    let socket_addr = addrs.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            format!("could not resolve {address}"),
        )
    })?;

    let stream = TcpStream::connect_timeout(&socket_addr, Duration::from_secs(5))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    Ok(stream)
}

fn parse_http_response(bytes: &[u8]) -> io::Result<HttpResponse> {
    let header_end = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP response was missing headers",
            )
        })?;
    let header_bytes = &bytes[..header_end];
    let body_bytes = &bytes[header_end + 4..];

    let header_text = String::from_utf8(header_bytes.to_vec()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("HTTP headers were not valid UTF-8: {error}"),
        )
    })?;

    let mut lines = header_text.lines();
    let status_line = lines.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "HTTP response was missing a status line",
        )
    })?;

    let mut status_parts = status_line.splitn(3, ' ');
    let _version = status_parts.next();
    let status_code = status_parts
        .next()
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "HTTP status line was malformed")
        })?
        .parse::<u16>()
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("HTTP status code was invalid: {error}"),
            )
        })?;
    let reason_phrase = status_parts.next().unwrap_or("").trim().to_string();
    let headers = parse_header_lines(lines);

    let body = if headers
        .get("transfer-encoding")
        .map(|value| value.eq_ignore_ascii_case("chunked"))
        .unwrap_or(false)
    {
        decode_chunked_body(body_bytes)?
    } else {
        body_bytes.to_vec()
    };

    Ok(HttpResponse {
        status_code,
        reason_phrase,
        headers,
        body,
    })
}

fn decode_chunked_body(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut cursor = 0;

    while cursor < bytes.len() {
        let line_end = find_crlf(bytes, cursor).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "chunked response was missing chunk size",
            )
        })?;
        let size_line = std::str::from_utf8(&bytes[cursor..line_end]).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("chunk size line was not valid UTF-8: {error}"),
            )
        })?;
        let size_hex = size_line.split(';').next().unwrap_or("").trim();
        let chunk_size = usize::from_str_radix(size_hex, 16).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("chunk size was invalid: {error}"),
            )
        })?;
        cursor = line_end + 2;

        if chunk_size == 0 {
            return Ok(output);
        }

        let chunk_end = cursor + chunk_size;
        if chunk_end > bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk extended beyond available response body",
            ));
        }

        output.extend_from_slice(&bytes[cursor..chunk_end]);
        cursor = chunk_end + 2;
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "chunked response terminated unexpectedly",
    ))
}

fn parse_http_url(url: &str) -> io::Result<HttpUrl> {
    let without_scheme = url.strip_prefix("http://").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("only plain http URLs are supported right now: {url}"),
        )
    })?;

    let (authority, path) = match without_scheme.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (without_scheme, "/".to_string()),
    };

    if authority.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("URL was missing a host: {url}"),
        ));
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port_text))
            if !host.contains(']') && port_text.chars().all(|char| char.is_ascii_digit()) =>
        {
            let port = port_text.parse::<u16>().map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("URL port was invalid: {error}"),
                )
            })?;
            (host.to_string(), port)
        }
        _ => (authority.to_string(), 80),
    };

    Ok(HttpUrl {
        host,
        port,
        path_and_query: path,
    })
}

fn parse_header_lines<'a>(lines: impl Iterator<Item = &'a str>) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    headers
}

fn base_url(location: &str) -> String {
    if let Ok(parsed) = parse_http_url(location) {
        format!("http://{}:{}", parsed.host, parsed.port)
    } else {
        location.to_string()
    }
}

fn resolve_url(base: &str, value: &str) -> String {
    if value.starts_with("http://") {
        return value.to_string();
    }

    if value.starts_with('/') {
        return format!("{}{}", base.trim_end_matches('/'), value);
    }

    format!("{}/{}", base.trim_end_matches('/'), value)
}

fn didl_lite_metadata(stream_url: &str, mime_type: &str, title: &str) -> String {
    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="track-0" parentID="library" restricted="1"><dc:title>{}</dc:title><upnp:class>object.item.audioItem.musicTrack</upnp:class><res protocolInfo="http-get:*:{}:*">{}</res></item></DIDL-Lite>"#,
        xml_escape(title),
        xml_escape(mime_type),
        xml_escape(stream_url),
    )
}

fn extract_first_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open_tag = format!("<{tag}>");
    let close_tag = format!("</{tag}>");
    let start = xml.find(&open_tag)? + open_tag.len();
    let end = xml[start..].find(&close_tag)? + start;
    Some(&xml[start..end])
}

fn extract_all_tag_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let open_tag = format!("<{tag}>");
    let close_tag = format!("</{tag}>");
    let mut blocks = Vec::new();
    let mut remainder = xml;

    while let Some(start_index) = remainder.find(&open_tag) {
        let content_start = start_index + open_tag.len();
        let after_open = &remainder[content_start..];
        let Some(end_offset) = after_open.find(&close_tag) else {
            break;
        };
        blocks.push(&after_open[..end_offset]);
        remainder = &after_open[end_offset + close_tag.len()..];
    }

    blocks
}

fn find_crlf(bytes: &[u8], start: usize) -> Option<usize> {
    bytes[start..]
        .windows(2)
        .position(|window| window == b"\r\n")
        .map(|offset| start + offset)
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn parse_upnp_time(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() || value == "NOT_IMPLEMENTED" {
        return None;
    }

    let mut parts = value.split(':');
    let hours = parts.next()?.parse::<u64>().ok()?;
    let minutes = parts.next()?.parse::<u64>().ok()?;
    let seconds = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }

    Some((hours * 3600) + (minutes * 60) + seconds)
}

impl fmt::Display for RendererDescription {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "Renderer: {}", self.friendly_name)?;
        writeln!(formatter, "Location: {}", self.location)?;
        writeln!(formatter, "Device type: {}", self.device_type)?;
        if let Some(manufacturer) = &self.manufacturer {
            writeln!(formatter, "Manufacturer: {manufacturer}")?;
        }
        if let Some(model_name) = &self.model_name {
            writeln!(formatter, "Model: {model_name}")?;
        }
        writeln!(formatter, "AVTransport: {}", self.av_transport_control_url)?;
        if let Some(control_url) = &self.rendering_control_url {
            writeln!(formatter, "RenderingControl: {control_url}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_get_position_info_envelope, build_get_transport_info_envelope, build_next_envelope,
        build_pause_envelope, build_play_envelope, build_previous_envelope,
        build_set_av_transport_uri_envelope, build_set_next_av_transport_uri_envelope,
        build_stop_envelope, decode_chunked_body, is_transition_not_available_fault,
        parse_device_description, parse_http_response, parse_http_url,
        parse_position_info_response, parse_ssdp_response, parse_transport_info_response,
        resolve_url,
    };
    use std::collections::HashMap;

    #[test]
    fn set_transport_uri_contains_escaped_values() {
        let body = build_set_av_transport_uri_envelope(
            0,
            "http://server.local/stream/this&that.flac",
            "audio/flac",
            Some("Fish & Chips"),
        );

        assert!(body.contains("SetAVTransportURI"));
        assert!(body.contains("this&amp;that.flac"));
        assert!(body.contains("&lt;DIDL-Lite"));
        assert!(body.contains("Fish &amp;amp; Chips"));
        assert!(body.contains("audio/flac"));
    }

    #[test]
    fn play_envelope_contains_requested_speed() {
        let body = build_play_envelope(0, 1);

        assert!(body.contains("<Speed>1</Speed>"));
    }

    #[test]
    fn pause_stop_next_previous_envelopes_contain_actions() {
        assert!(build_pause_envelope(0).contains("Pause"));
        assert!(build_stop_envelope(0).contains("Stop"));
        assert!(build_next_envelope(0).contains("Next"));
        assert!(build_previous_envelope(0).contains("Previous"));
    }

    #[test]
    fn set_next_transport_uri_contains_escaped_values() {
        let body = build_set_next_av_transport_uri_envelope(
            0,
            "http://server.local/stream/next&track.flac",
            "audio/flac",
            Some("Next & Best"),
        );

        assert!(body.contains("SetNextAVTransportURI"));
        assert!(body.contains("next&amp;track.flac"));
        assert!(body.contains("&lt;DIDL-Lite"));
        assert!(body.contains("Next &amp;amp; Best"));
        assert!(body.contains("audio/flac"));
    }

    #[test]
    fn transport_info_envelope_contains_action() {
        let body = build_get_transport_info_envelope(0);
        assert!(body.contains("GetTransportInfo"));
    }

    #[test]
    fn position_info_envelope_contains_action() {
        let body = build_get_position_info_envelope(0);
        assert!(body.contains("GetPositionInfo"));
    }

    #[test]
    fn parses_ssdp_response_headers() {
        let response = concat!(
            "HTTP/1.1 200 OK\r\n",
            "CACHE-CONTROL: max-age=1800\r\n",
            "LOCATION: http://192.168.1.55:49152/description.xml\r\n",
            "ST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n",
            "USN: uuid:renderer-1::urn:schemas-upnp-org:device:MediaRenderer:1\r\n",
            "SERVER: Linux/5.10 UPnP/1.0 Renderer/1.0\r\n",
            "\r\n"
        );

        let parsed = parse_ssdp_response(response).expect("valid SSDP response");
        assert_eq!(parsed.location, "http://192.168.1.55:49152/description.xml");
        assert_eq!(
            parsed.search_target.as_deref(),
            Some("urn:schemas-upnp-org:device:MediaRenderer:1")
        );
    }

    #[test]
    fn parses_device_description_and_resolves_control_urls() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <URLBase>http://192.168.1.55:49152</URLBase>
  <device>
    <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
    <friendlyName>Living Room CXN V2</friendlyName>
    <manufacturer>Cambridge Audio</manufacturer>
    <modelName>CXN V2</modelName>
    <serviceList>
      <service>
        <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
        <serviceId>urn:upnp-org:serviceId:AVTransport</serviceId>
        <controlURL>/upnp/control/avtransport1</controlURL>
        <eventSubURL>/upnp/event/avtransport1</eventSubURL>
        <SCPDURL>/xml/AVTransport1.xml</SCPDURL>
      </service>
    </serviceList>
  </device>
</root>"#;

        let description =
            parse_device_description("http://192.168.1.55:49152/description.xml", xml).unwrap();
        assert_eq!(description.friendly_name, "Living Room CXN V2");
        assert_eq!(description.services.len(), 1);
        assert_eq!(
            description.services[0].control_url,
            "http://192.168.1.55:49152/upnp/control/avtransport1"
        );
    }

    #[test]
    fn parses_plain_http_urls() {
        let parsed = parse_http_url("http://192.168.1.55:49152/description.xml").unwrap();
        assert_eq!(parsed.host, "192.168.1.55");
        assert_eq!(parsed.port, 49152);
        assert_eq!(parsed.path_and_query, "/description.xml");
    }

    #[test]
    fn decodes_chunked_http_bodies() {
        let body = decode_chunked_body(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap();
        assert_eq!(body, b"Wikipedia");
    }

    #[test]
    fn parses_chunked_http_response() {
        let response = parse_http_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\ntest\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(response.status_code, 200);
        assert_eq!(response.body, b"test");
    }

    #[test]
    fn parses_transport_info_response() {
        let body = br#"
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:GetTransportInfoResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <CurrentTransportState>PLAYING</CurrentTransportState>
      <CurrentTransportStatus>OK</CurrentTransportStatus>
      <CurrentSpeed>1</CurrentSpeed>
    </u:GetTransportInfoResponse>
  </s:Body>
</s:Envelope>"#;

        let parsed = parse_transport_info_response(body).unwrap();
        assert_eq!(parsed.transport_state, "PLAYING");
        assert_eq!(parsed.transport_status.as_deref(), Some("OK"));
        assert_eq!(parsed.current_speed.as_deref(), Some("1"));
    }

    #[test]
    fn parses_position_info_response() {
        let body = br#"
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:GetPositionInfoResponse xmlns:u="urn:schemas-upnp-org:service:AVTransport:1">
      <TrackURI>http://musicd.local/stream/track/abc</TrackURI>
      <TrackDuration>00:03:42</TrackDuration>
      <RelTime>00:01:11</RelTime>
    </u:GetPositionInfoResponse>
  </s:Body>
</s:Envelope>"#;

        let parsed = parse_position_info_response(body).unwrap();
        assert_eq!(
            parsed.track_uri.as_deref(),
            Some("http://musicd.local/stream/track/abc")
        );
        assert_eq!(parsed.track_duration_seconds, Some(222));
        assert_eq!(parsed.rel_time_seconds, Some(71));
    }

    #[test]
    fn resolves_relative_and_absolute_urls() {
        assert_eq!(
            resolve_url("http://192.168.1.55:49152", "/upnp/control"),
            "http://192.168.1.55:49152/upnp/control"
        );
        assert_eq!(
            resolve_url("http://192.168.1.55:49152/base", "upnp/control"),
            "http://192.168.1.55:49152/base/upnp/control"
        );
    }

    #[test]
    fn detects_transition_not_available_fault() {
        let response = super::HttpResponse {
            status_code: 500,
            reason_phrase: "SOAP Error".to_string(),
            headers: HashMap::new(),
            body: br#"<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <s:Fault>
      <detail>
        <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
          <errorCode>701</errorCode>
          <errorDescription>Transition not available</errorDescription>
        </UPnPError>
      </detail>
    </s:Fault>
  </s:Body>
</s:Envelope>"#
                .to_vec(),
        };

        assert!(is_transition_not_available_fault(&response));
    }
}
