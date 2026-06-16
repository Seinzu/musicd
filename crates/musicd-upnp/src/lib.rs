use std::collections::HashMap;
use std::fmt;
use std::io;
use std::net::UdpSocket;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use reqwest::Method;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

const MEDIA_RENDERER_ST: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";
const AV_TRANSPORT_SERVICE: &str = "urn:schemas-upnp-org:service:AVTransport:1";
const RENDERING_CONTROL_SERVICE: &str = "urn:schemas-upnp-org:service:RenderingControl:1";
const PLAYLIST_EXTENSION_SERVICE: &str = "urn:UuVol-com:service:PlaylistExtension:1";
const SM_SEARCH_SERVICE: &str = "urn:UuVol-com:service:SMSearch:1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamResource {
    pub stream_url: String,
    pub mime_type: String,
    pub title: String,
    pub album_art_url: Option<String>,
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
    pub services: Vec<UpnpService>,
    pub av_transport_control_url: String,
    pub rendering_control_url: Option<String>,
    pub capabilities: RendererCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RendererCapabilities {
    pub av_transport_actions: Option<Vec<String>>,
    pub has_playlist_extension_service: Option<bool>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpnpActionResponse {
    pub action: String,
    pub values: Vec<(String, String)>,
    pub raw_xml: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpnpActionDescription {
    pub name: String,
    pub arguments: Vec<UpnpActionArgument>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpnpActionArgument {
    pub name: String,
    pub direction: Option<String>,
    pub related_state_variable: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererPlaylist {
    pub id_array_token: Option<u32>,
    pub ids: Vec<u32>,
    pub entries: Vec<RendererPlaylistEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererPlaylistEntry {
    pub id: u32,
    pub uri: String,
    pub title: Option<String>,
    pub metadata: Option<String>,
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
        let av_transport_actions = device
            .find_service(AV_TRANSPORT_SERVICE)
            .and_then(|service| service.scpd_url.as_deref())
            .map(fetch_service_actions)
            .transpose()
            .ok()
            .flatten();
        let has_playlist_extension_service =
            Some(device.find_service(PLAYLIST_EXTENSION_SERVICE).is_some());

        Ok(Self {
            location: device.location,
            friendly_name: device.friendly_name,
            device_type: device.device_type,
            manufacturer: device.manufacturer,
            model_name: device.model_name,
            services: device.services,
            av_transport_control_url,
            rendering_control_url,
            capabilities: RendererCapabilities {
                av_transport_actions,
                has_playlist_extension_service,
            },
        })
    }
}

impl RendererCapabilities {
    pub fn supports_action(&self, action: &str) -> Option<bool> {
        self.av_transport_actions
            .as_ref()
            .map(|actions| actions.iter().any(|candidate| candidate == action))
    }

    pub fn supports_set_next_av_transport_uri(&self) -> Option<bool> {
        self.supports_action("SetNextAVTransportURI")
    }

    pub fn supports_pause(&self) -> Option<bool> {
        self.supports_action("Pause")
    }

    pub fn supports_stop(&self) -> Option<bool> {
        self.supports_action("Stop")
    }

    pub fn supports_next(&self) -> Option<bool> {
        self.supports_action("Next")
    }

    pub fn supports_previous(&self) -> Option<bool> {
        self.supports_action("Previous")
    }

    pub fn supports_seek(&self) -> Option<bool> {
        self.supports_action("Seek")
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

pub fn fetch_service_actions(scpd_url: &str) -> io::Result<Vec<String>> {
    Ok(fetch_service_action_descriptions(scpd_url)?
        .into_iter()
        .map(|action| action.name)
        .collect())
}

pub fn fetch_service_action_descriptions(scpd_url: &str) -> io::Result<Vec<UpnpActionDescription>> {
    let response = http_request("GET", scpd_url, &[("Accept", "application/xml")], None)?;
    if response.status_code != 200 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "service description request failed with HTTP {} {}",
                response.status_code, response.reason_phrase
            ),
        ));
    }

    let body = String::from_utf8(response.body).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("service description body was not valid UTF-8: {error}"),
        )
    })?;

    Ok(parse_service_action_descriptions(&body))
}

pub fn set_av_transport_uri(control_url: &str, resource: &StreamResource) -> io::Result<()> {
    let body = build_set_av_transport_uri_envelope(
        0,
        &resource.stream_url,
        &resource.mime_type,
        Some(&resource.title),
        resource.album_art_url.as_deref(),
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
        resource.album_art_url.as_deref(),
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

pub fn clear_next_av_transport_uri(control_url: &str) -> io::Result<()> {
    let body = build_clear_next_av_transport_uri_envelope(0);
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

pub fn seek(control_url: &str, position_seconds: u64) -> io::Result<()> {
    let body = build_seek_envelope(0, position_seconds);
    let response = av_transport_action(control_url, "Seek", body.as_bytes())?;
    expect_successful_soap("Seek", response)
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

pub fn query_av_transport_action(
    control_url: &str,
    action: &str,
) -> io::Result<UpnpActionResponse> {
    let body = build_instance_id_only_envelope(action, 0);
    let response = av_transport_action(control_url, action, body.as_bytes())?;
    expect_successful_soap(action, response.clone())?;
    let raw_xml = String::from_utf8(response.body).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{action} response body was not valid UTF-8: {error}"),
        )
    })?;
    Ok(UpnpActionResponse {
        action: action.to_string(),
        values: extract_action_values(action, &raw_xml),
        raw_xml,
    })
}

pub fn sm_search_service(renderer: &RendererDescription) -> Option<&UpnpService> {
    find_sm_search_service(&renderer.services)
}

pub fn query_sm_search_action(
    control_url: &str,
    action: &str,
    args: &[(&str, &str)],
) -> io::Result<UpnpActionResponse> {
    query_upnp_service_action(SM_SEARCH_SERVICE, control_url, action, args)
}

pub fn query_upnp_service_action(
    service_type: &str,
    control_url: &str,
    action: &str,
    args: &[(&str, &str)],
) -> io::Result<UpnpActionResponse> {
    let body = build_action_envelope(service_type, action, args);
    let response = service_action(service_type, control_url, action, body.as_bytes())?;
    expect_successful_soap(action, response.clone())?;
    let raw_xml = response_body_utf8(action, response.body)?;
    Ok(UpnpActionResponse {
        action: action.to_string(),
        values: extract_generic_action_values(action, &raw_xml),
        raw_xml,
    })
}

pub fn query_playlist_extension_queue(control_url: &str) -> io::Result<RendererPlaylist> {
    let id_array = playlist_extension_action(
        control_url,
        "IdArray",
        build_no_arg_envelope(PLAYLIST_EXTENSION_SERVICE, "IdArray").as_bytes(),
    )?;
    expect_successful_soap("IdArray", id_array.clone())?;
    let id_array_xml = response_body_utf8("IdArray", id_array.body)?;
    let id_array_token = extract_first_tag(&id_array_xml, "aIdArrayToken")
        .and_then(|value| value.trim().parse::<u32>().ok());
    let ids = extract_first_tag(&id_array_xml, "aIdArray")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(decode_playlist_id_array)
        .transpose()?
        .unwrap_or_default();

    let mut entries = if ids.is_empty() {
        Vec::new()
    } else {
        query_playlist_extension_read_list(control_url, &ids)?
    };
    for id in &ids {
        if entries.iter().any(|entry| entry.id == *id) {
            continue;
        }
        if let Ok(entry) = query_playlist_extension_read(control_url, *id) {
            entries.push(entry);
        }
    }

    Ok(RendererPlaylist {
        id_array_token,
        ids,
        entries,
    })
}

pub fn clear_playlist_extension_queue(renderer_location: &str) -> io::Result<bool> {
    let renderer = inspect_renderer(renderer_location)?;
    let Some(service) = renderer
        .services
        .iter()
        .find(|service| service.service_type == PLAYLIST_EXTENSION_SERVICE)
    else {
        return Ok(false);
    };

    let response = playlist_extension_action(
        &service.control_url,
        "DeleteAll",
        build_no_arg_envelope(PLAYLIST_EXTENSION_SERVICE, "DeleteAll").as_bytes(),
    )?;
    expect_successful_soap("DeleteAll", response)?;
    Ok(true)
}

pub fn sync_playlist_extension_queue_after_current(
    control_url: &str,
    current: &StreamResource,
    successors: &[StreamResource],
) -> io::Result<bool> {
    let queue = query_playlist_extension_queue(control_url)?;
    let Some(current_id) = queue
        .entries
        .iter()
        .find(|entry| entry.uri == current.stream_url)
        .map(|entry| entry.id)
    else {
        return Ok(false);
    };

    for id in queue.ids.iter().copied().filter(|id| *id != current_id) {
        delete_playlist_extension_entry(control_url, id)?;
    }

    let mut after_id = current_id;
    for resource in successors {
        after_id = insert_playlist_extension_entry(control_url, after_id, resource)?;
    }

    Ok(true)
}

pub fn insert_playlist_extension_entry(
    control_url: &str,
    after_id: u32,
    resource: &StreamResource,
) -> io::Result<u32> {
    let after_id_value = after_id.to_string();
    let metadata = didl_lite_metadata(
        &resource.stream_url,
        &resource.mime_type,
        &resource.title,
        resource.album_art_url.as_deref(),
    );
    let body = build_action_envelope(
        PLAYLIST_EXTENSION_SERVICE,
        "Insert",
        &[
            ("aAfterId", after_id_value.as_str()),
            ("aUri", resource.stream_url.as_str()),
            ("aMetaData", metadata.as_str()),
        ],
    );
    let response = playlist_extension_action(control_url, "Insert", body.as_bytes())?;
    expect_successful_soap("Insert", response.clone())?;
    let raw_xml = response_body_utf8("Insert", response.body)?;
    extract_first_tag(&raw_xml, "aNewId")
        .and_then(|value| value.trim().parse::<u32>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Insert response missing aNewId"))
}

pub fn delete_playlist_extension_entry(control_url: &str, id: u32) -> io::Result<()> {
    let id_value = id.to_string();
    let body = build_action_envelope(
        PLAYLIST_EXTENSION_SERVICE,
        "Delete",
        &[("aId", id_value.as_str())],
    );
    let response = playlist_extension_action(control_url, "Delete", body.as_bytes())?;
    expect_successful_soap("Delete", response)
}

fn query_playlist_extension_read_list(
    control_url: &str,
    ids: &[u32],
) -> io::Result<Vec<RendererPlaylistEntry>> {
    let id_list = ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let body = build_action_envelope(
        PLAYLIST_EXTENSION_SERVICE,
        "ReadList",
        &[("aIdList", &id_list)],
    );
    let response = playlist_extension_action(control_url, "ReadList", body.as_bytes())?;
    expect_successful_soap("ReadList", response.clone())?;
    let raw_xml = response_body_utf8("ReadList", response.body)?;
    let Some(metadata_list) = extract_first_tag(&raw_xml, "aMetaDataList") else {
        return Ok(Vec::new());
    };
    Ok(parse_playlist_metadata_list(&xml_unescape(metadata_list)))
}

fn query_playlist_extension_read(control_url: &str, id: u32) -> io::Result<RendererPlaylistEntry> {
    let id_value = id.to_string();
    let body = build_action_envelope(
        PLAYLIST_EXTENSION_SERVICE,
        "Read",
        &[("aId", id_value.as_str())],
    );
    let response = playlist_extension_action(control_url, "Read", body.as_bytes())?;
    expect_successful_soap("Read", response.clone())?;
    let raw_xml = response_body_utf8("Read", response.body)?;
    let uri = extract_first_tag(&raw_xml, "aUri")
        .map(xml_unescape)
        .unwrap_or_default();
    let metadata = extract_first_tag(&raw_xml, "aMetaData")
        .map(xml_unescape)
        .filter(|value| !value.trim().is_empty());
    let title = metadata
        .as_deref()
        .and_then(|metadata| extract_first_tag(metadata, "dc:title"))
        .map(xml_unescape);
    Ok(RendererPlaylistEntry {
        id,
        uri,
        title,
        metadata,
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
    album_art_url: Option<&str>,
) -> String {
    let metadata = didl_lite_metadata(
        stream_url,
        mime_type,
        title.unwrap_or("Unknown title"),
        album_art_url,
    );

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
    album_art_url: Option<&str>,
) -> String {
    let metadata = didl_lite_metadata(
        stream_url,
        mime_type,
        title.unwrap_or("Unknown title"),
        album_art_url,
    );
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:SetNextAVTransportURI xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID><NextURI>{}</NextURI><NextURIMetaData>{}</NextURIMetaData></u:SetNextAVTransportURI></s:Body></s:Envelope>"#,
        xml_escape(stream_url),
        xml_escape(&metadata),
    )
}

pub fn build_clear_next_av_transport_uri_envelope(instance_id: u32) -> String {
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:SetNextAVTransportURI xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID><NextURI></NextURI><NextURIMetaData></NextURIMetaData></u:SetNextAVTransportURI></s:Body></s:Envelope>"#,
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

pub fn build_seek_envelope(instance_id: u32, position_seconds: u64) -> String {
    let target = format_upnp_time(position_seconds);
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:Seek xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID><Unit>REL_TIME</Unit><Target>{target}</Target></u:Seek></s:Body></s:Envelope>"#
    )
}

fn format_upnp_time(total_seconds: u64) -> String {
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    format!("{hours}:{minutes:02}:{seconds:02}")
}

// These two envelopes are sent on every queue-worker poll tick. The InstanceID
// field is always 0 in practice, so we precompute the rendered envelope and
// skip the format-string parse on the hot path. Non-zero instance IDs (kept
// for spec compatibility) fall through to the original `format!` path.
const GET_TRANSPORT_INFO_ENVELOPE_INSTANCE_0: &str = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetTransportInfo xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>0</InstanceID></u:GetTransportInfo></s:Body></s:Envelope>"#;
const GET_POSITION_INFO_ENVELOPE_INSTANCE_0: &str = r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetPositionInfo xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>0</InstanceID></u:GetPositionInfo></s:Body></s:Envelope>"#;

pub fn build_get_transport_info_envelope(instance_id: u32) -> String {
    if instance_id == 0 {
        return GET_TRANSPORT_INFO_ENVELOPE_INSTANCE_0.to_string();
    }
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetTransportInfo xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID></u:GetTransportInfo></s:Body></s:Envelope>"#
    )
}

pub fn build_get_position_info_envelope(instance_id: u32) -> String {
    if instance_id == 0 {
        return GET_POSITION_INFO_ENVELOPE_INSTANCE_0.to_string();
    }
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetPositionInfo xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID></u:GetPositionInfo></s:Body></s:Envelope>"#
    )
}

fn build_instance_id_only_envelope(action: &str, instance_id: u32) -> String {
    format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="urn:schemas-upnp-org:service:AVTransport:1"><InstanceID>{instance_id}</InstanceID></u:{action}></s:Body></s:Envelope>"#
    )
}

fn build_no_arg_envelope(service_type: &str, action: &str) -> String {
    build_action_envelope(service_type, action, &[])
}

fn build_action_envelope(service_type: &str, action: &str, args: &[(&str, &str)]) -> String {
    let mut body = format!(
        r#"<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:{action} xmlns:u="{service_type}">"#
    );
    for (name, value) in args {
        body.push('<');
        body.push_str(name);
        body.push('>');
        body.push_str(&xml_escape(value));
        body.push_str("</");
        body.push_str(name);
        body.push('>');
    }
    body.push_str("</u:");
    body.push_str(action);
    body.push_str("></s:Body></s:Envelope>");
    body
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

fn response_body_utf8(action: &str, body: Vec<u8>) -> io::Result<String> {
    String::from_utf8(body).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{action} response body was not valid UTF-8: {error}"),
        )
    })
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

fn playlist_extension_action(
    control_url: &str,
    action: &str,
    body: &[u8],
) -> io::Result<HttpResponse> {
    service_action(PLAYLIST_EXTENSION_SERVICE, control_url, action, body)
}

fn service_action(
    service_type: &str,
    control_url: &str,
    action: &str,
    body: &[u8],
) -> io::Result<HttpResponse> {
    let soap_action = format!("\"{service_type}#{action}\"");
    http_request(
        "POST",
        control_url,
        &[
            ("Content-Type", "text/xml; charset=\"utf-8\""),
            ("SOAPACTION", &soap_action),
        ],
        Some(body),
    )
}

fn find_sm_search_service(services: &[UpnpService]) -> Option<&UpnpService> {
    services.iter().find(|service| {
        service.service_type == SM_SEARCH_SERVICE
            || service
                .service_type
                .to_ascii_lowercase()
                .contains("smsearch")
            || service
                .service_id
                .as_deref()
                .map(|service_id| service_id.to_ascii_lowercase().contains("smsearch"))
                .unwrap_or(false)
    })
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

fn extract_action_values(action: &str, xml: &str) -> Vec<(String, String)> {
    known_action_response_fields(action)
        .iter()
        .filter_map(|field| {
            extract_first_tag(xml, field)
                .map(str::trim)
                .map(|value| ((*field).to_string(), value.to_string()))
        })
        .collect()
}

fn extract_generic_action_values(action: &str, xml: &str) -> Vec<(String, String)> {
    let response_tag = format!("{action}Response");
    let response_xml = extract_first_namespaced_tag(xml, &response_tag).unwrap_or(xml);
    extract_direct_text_elements(response_xml)
}

fn known_action_response_fields(action: &str) -> &'static [&'static str] {
    match action {
        "GetTransportInfo" => &[
            "CurrentTransportState",
            "CurrentTransportStatus",
            "CurrentSpeed",
        ],
        "GetPositionInfo" => &[
            "Track",
            "TrackDuration",
            "TrackMetaData",
            "TrackURI",
            "RelTime",
            "AbsTime",
            "RelCount",
            "AbsCount",
        ],
        "GetMediaInfo" => &[
            "NrTracks",
            "MediaDuration",
            "CurrentURI",
            "CurrentURIMetaData",
            "NextURI",
            "NextURIMetaData",
            "PlayMedium",
            "RecordMedium",
            "WriteStatus",
        ],
        "GetDeviceCapabilities" => &["PlayMedia", "RecMedia", "RecQualityModes"],
        "GetTransportSettings" => &["PlayMode", "RecQualityMode"],
        "GetCurrentTransportActions" => &["Actions"],
        _ => &[],
    }
}

fn parse_playlist_metadata_list(xml: &str) -> Vec<RendererPlaylistEntry> {
    extract_all_tag_blocks(xml, "Entry")
        .into_iter()
        .filter_map(parse_playlist_metadata_entry)
        .collect()
}

fn parse_playlist_metadata_entry(xml: &str) -> Option<RendererPlaylistEntry> {
    let id =
        extract_first_tag(xml, "Id").and_then(|value| parse_playlist_entry_id(value.trim()))?;
    let uri = extract_first_tag(xml, "Uri")
        .map(xml_unescape)
        .unwrap_or_default();
    let metadata = extract_first_tag(xml, "MetaData")
        .map(xml_unescape)
        .filter(|value| !value.trim().is_empty());
    let title = metadata
        .as_deref()
        .and_then(|metadata| extract_first_tag(metadata, "dc:title"))
        .map(xml_unescape);
    Some(RendererPlaylistEntry {
        id,
        uri,
        title,
        metadata,
    })
}

fn parse_playlist_entry_id(value: &str) -> Option<u32> {
    let digits = value
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse::<u32>().ok()
}

fn decode_playlist_id_array(value: &str) -> io::Result<Vec<u32>> {
    let bytes = decode_base64(value)?;
    if bytes.len() % 4 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "playlist IdArray length {} was not a multiple of 4",
                bytes.len()
            ),
        ));
    }
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

fn decode_base64(value: &str) -> io::Result<Vec<u8>> {
    let mut bits = 0u32;
    let mut bit_count = 0u8;
    let mut output = Vec::new();
    for byte in value.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        if byte == b'=' {
            break;
        }
        let Some(six_bits) = base64_value(byte) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid base64 byte 0x{byte:02x}"),
            ));
        };
        bits = (bits << 6) | u32::from(six_bits);
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            output.push(((bits >> bit_count) & 0xff) as u8);
        }
    }
    Ok(output)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn parse_device_description(location: &str, xml: &str) -> io::Result<DeviceDescription> {
    let url_base = extract_first_tag(xml, "URLBase")
        .map(str::to_string)
        .unwrap_or_else(|| base_url(location));

    let root_device_section = extract_first_balanced_tag(xml, "device").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "device description was missing a <device> section",
        )
    })?;
    let device_section = select_renderer_device_section(root_device_section);

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

fn select_renderer_device_section<'a>(root_device_section: &'a str) -> &'a str {
    if device_section_is_media_renderer(root_device_section) {
        return root_device_section;
    }

    let device_list = extract_first_tag(root_device_section, "deviceList").unwrap_or("");
    extract_all_balanced_tag_blocks(device_list, "device")
        .into_iter()
        .find(|device_section| device_section_is_media_renderer(device_section))
        .unwrap_or(root_device_section)
}

fn device_section_is_media_renderer(device_section: &str) -> bool {
    extract_first_tag(device_section, "deviceType")
        .map(str::trim)
        .map(|device_type| device_type == MEDIA_RENDERER_ST)
        .unwrap_or(false)
}

#[cfg(test)]
fn parse_service_actions(xml: &str) -> Vec<String> {
    let mut actions = parse_service_action_descriptions(xml)
        .into_iter()
        .map(|action| action.name)
        .collect::<Vec<_>>();
    actions.sort();
    actions.dedup();
    actions
}

fn parse_service_action_descriptions(xml: &str) -> Vec<UpnpActionDescription> {
    let action_list = extract_first_tag(xml, "actionList").unwrap_or(xml);
    let mut actions = extract_all_tag_blocks(action_list, "action")
        .into_iter()
        .filter_map(parse_service_action_description)
        .collect::<Vec<_>>();
    actions.sort_by(|left, right| left.name.cmp(&right.name));
    actions.dedup_by(|left, right| left.name == right.name);
    actions
}

fn parse_service_action_description(xml: &str) -> Option<UpnpActionDescription> {
    let name = extract_first_tag(xml, "name")?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let argument_list = extract_first_tag(xml, "argumentList").unwrap_or("");
    let arguments = extract_all_tag_blocks(argument_list, "argument")
        .into_iter()
        .filter_map(parse_service_action_argument)
        .collect();
    Some(UpnpActionDescription { name, arguments })
}

fn parse_service_action_argument(xml: &str) -> Option<UpnpActionArgument> {
    let name = extract_first_tag(xml, "name")?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some(UpnpActionArgument {
        name,
        direction: extract_first_tag(xml, "direction")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        related_state_variable: extract_first_tag(xml, "relatedStateVariable")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn shared_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent("musicd/0.1")
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(8))
            .pool_idle_timeout(Some(Duration::from_secs(60)))
            .build()
            .expect("failed to build reqwest client for upnp")
    })
}

fn http_request(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> io::Result<HttpResponse> {
    let method = match method {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "HEAD" => Method::HEAD,
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsupported HTTP method: {other}"),
            ));
        }
    };

    let mut header_map = HeaderMap::with_capacity(headers.len());
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid header name {name}: {error}"),
            )
        })?;
        let value = HeaderValue::from_str(value).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid header value: {error}"),
            )
        })?;
        header_map.append(name, value);
    }

    let mut request = shared_client().request(method, url).headers(header_map);
    if let Some(body) = body {
        request = request.body(body.to_vec());
    }

    let response = request
        .send()
        .map_err(|error| io::Error::other(format!("HTTP request to {url} failed: {error}")))?;

    let status_code = response.status().as_u16();
    let reason_phrase = response
        .status()
        .canonical_reason()
        .unwrap_or("")
        .to_string();
    let mut response_headers = HashMap::with_capacity(response.headers().len());
    for (name, value) in response.headers().iter() {
        if let Ok(value) = value.to_str() {
            response_headers.insert(name.as_str().to_ascii_lowercase(), value.to_string());
        }
    }
    let body = response
        .bytes()
        .map_err(|error| io::Error::other(format!("HTTP body read from {url} failed: {error}")))?
        .to_vec();

    Ok(HttpResponse {
        status_code,
        reason_phrase,
        headers: response_headers,
        body,
    })
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

fn didl_lite_metadata(
    stream_url: &str,
    mime_type: &str,
    title: &str,
    album_art_url: Option<&str>,
) -> String {
    let album_art_xml = album_art_url.map_or_else(String::new, |url| {
        format!("<upnp:albumArtURI>{}</upnp:albumArtURI>", xml_escape(url))
    });
    format!(
        r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/"><item id="track-0" parentID="library" restricted="1"><dc:title>{}</dc:title><upnp:class>object.item.audioItem.musicTrack</upnp:class>{}<res protocolInfo="http-get:*:{}:*">{}</res></item></DIDL-Lite>"#,
        xml_escape(title),
        album_art_xml,
        xml_escape(mime_type),
        xml_escape(stream_url),
    )
}

/// Stack-allocated `<tag>` / `</tag>` patterns. Avoids the per-call
/// `format!` allocation that the previous implementations paid on every
/// `extract_first_tag` and friends.
struct TagPattern {
    /// Bytes 0..open_len hold `<tag>`; bytes 64..64+close_len hold `</tag>`.
    buffer: [u8; 128],
    open_len: usize,
    close_len: usize,
}

impl TagPattern {
    fn new(tag: &str) -> Self {
        let tb = tag.as_bytes();
        let open_len = tb.len() + 2;
        let close_len = tb.len() + 3;
        assert!(
            close_len <= 64,
            "tag {tag:?} too long for the upnp tag-pattern buffer"
        );

        let mut buffer = [0u8; 128];
        buffer[0] = b'<';
        buffer[1..1 + tb.len()].copy_from_slice(tb);
        buffer[1 + tb.len()] = b'>';

        buffer[64] = b'<';
        buffer[65] = b'/';
        buffer[66..66 + tb.len()].copy_from_slice(tb);
        buffer[66 + tb.len()] = b'>';

        Self {
            buffer,
            open_len,
            close_len,
        }
    }

    fn open(&self) -> &str {
        // Safety: built from valid UTF-8 (`<` + ASCII tag + `>`).
        std::str::from_utf8(&self.buffer[..self.open_len]).expect("tag pattern is valid utf-8")
    }

    fn close(&self) -> &str {
        std::str::from_utf8(&self.buffer[64..64 + self.close_len])
            .expect("tag pattern is valid utf-8")
    }
}

fn extract_first_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let pattern = TagPattern::new(tag);
    let open_tag = pattern.open();
    let close_tag = pattern.close();
    let start = xml.find(open_tag)? + open_tag.len();
    let end = xml[start..].find(close_tag)? + start;
    Some(&xml[start..end])
}

fn extract_first_balanced_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    find_balanced_tag_content_range(xml, tag, 0).map(|(start, end, _)| &xml[start..end])
}

fn extract_first_namespaced_tag<'a>(xml: &'a str, local_tag: &str) -> Option<&'a str> {
    let mut search_start = 0usize;
    loop {
        let open_index = xml[search_start..].find('<')? + search_start;
        if xml[open_index + 1..].starts_with('/') {
            search_start = open_index + 1;
            continue;
        }
        let name_start = open_index + 1;
        let name_end = xml[name_start..]
            .find(|ch: char| ch == '>' || ch.is_whitespace())
            .map(|offset| name_start + offset)?;
        let name = &xml[name_start..name_end];
        if name.rsplit_once(':').map(|(_, name)| name).unwrap_or(name) != local_tag {
            search_start = name_end;
            continue;
        }
        let open_end = xml[name_end..].find('>').map(|offset| name_end + offset)? + 1;
        let close_tag = format!("</{name}>");
        let close_index = xml[open_end..]
            .find(&close_tag)
            .map(|offset| open_end + offset)?;
        return Some(&xml[open_end..close_index]);
    }
}

fn extract_all_tag_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let pattern = TagPattern::new(tag);
    let open_tag = pattern.open();
    let close_tag = pattern.close();
    let mut blocks = Vec::new();
    let mut remainder = xml;

    while let Some(start_index) = remainder.find(open_tag) {
        let content_start = start_index + open_tag.len();
        let after_open = &remainder[content_start..];
        let Some(end_offset) = after_open.find(close_tag) else {
            break;
        };
        blocks.push(&after_open[..end_offset]);
        remainder = &after_open[end_offset + close_tag.len()..];
    }

    blocks
}

fn extract_direct_text_elements(xml: &str) -> Vec<(String, String)> {
    let mut values = Vec::new();
    let mut search_start = 0usize;
    while let Some(relative_open_index) = xml[search_start..].find('<') {
        let open_index = search_start + relative_open_index;
        if xml[open_index + 1..].starts_with('/') {
            search_start = open_index + 1;
            continue;
        }

        let name_start = open_index + 1;
        let Some(name_end) = xml[name_start..]
            .find(|ch: char| ch == '>' || ch.is_whitespace())
            .map(|offset| name_start + offset)
        else {
            break;
        };
        let name = &xml[name_start..name_end];
        let Some(open_end) = xml[name_end..]
            .find('>')
            .map(|offset| name_end + offset + 1)
        else {
            break;
        };
        let close_tag = format!("</{name}>");
        let Some(close_index) = xml[open_end..]
            .find(&close_tag)
            .map(|offset| open_end + offset)
        else {
            search_start = open_end;
            continue;
        };
        let value = &xml[open_end..close_index];
        if !value.contains('<') {
            values.push((
                name.rsplit_once(':')
                    .map(|(_, local_name)| local_name)
                    .unwrap_or(name)
                    .to_string(),
                xml_unescape(value.trim()),
            ));
        }
        search_start = close_index + close_tag.len();
    }
    values
}

fn extract_all_balanced_tag_blocks<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let mut blocks = Vec::new();
    let mut search_start = 0;

    while let Some((content_start, content_end, next_search_start)) =
        find_balanced_tag_content_range(xml, tag, search_start)
    {
        blocks.push(&xml[content_start..content_end]);
        search_start = next_search_start;
    }

    blocks
}

fn find_balanced_tag_content_range(
    xml: &str,
    tag: &str,
    search_start: usize,
) -> Option<(usize, usize, usize)> {
    let pattern = TagPattern::new(tag);
    let open_tag = pattern.open();
    let close_tag = pattern.close();
    let open_len = open_tag.len();
    let close_len = close_tag.len();

    let open_index = xml[search_start..].find(open_tag)? + search_start;
    let content_start = open_index + open_len;
    let mut cursor = content_start;
    let mut depth = 1usize;

    while depth > 0 {
        let next_open = xml[cursor..].find(open_tag).map(|index| index + cursor);
        let next_close = xml[cursor..].find(close_tag).map(|index| index + cursor)?;

        if let Some(next_open_index) = next_open {
            if next_open_index < next_close {
                depth += 1;
                cursor = next_open_index + open_len;
                continue;
            }
        }

        depth -= 1;
        if depth == 0 {
            return Some((content_start, next_close, next_close + close_len));
        }
        cursor = next_close + close_len;
    }

    None
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
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
        if let Some(has_playlist_extension_service) =
            self.capabilities.has_playlist_extension_service
        {
            writeln!(
                formatter,
                "Playlist extension: {}",
                if has_playlist_extension_service {
                    "available"
                } else {
                    "not advertised"
                }
            )?;
        }
        if let Some(actions) = &self.capabilities.av_transport_actions {
            writeln!(formatter, "AVTransport actions: {}", actions.join(", "))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_clear_next_av_transport_uri_envelope, build_get_position_info_envelope,
        build_get_transport_info_envelope, build_next_envelope, build_pause_envelope,
        build_play_envelope, build_previous_envelope, build_seek_envelope,
        build_set_av_transport_uri_envelope, build_set_next_av_transport_uri_envelope,
        build_stop_envelope, decode_playlist_id_array, format_upnp_time,
        is_transition_not_available_fault, parse_device_description, parse_http_url,
        parse_playlist_metadata_list, parse_position_info_response,
        parse_service_action_descriptions, parse_service_actions, parse_ssdp_response,
        parse_transport_info_response, resolve_url, select_renderer_device_section,
    };
    use std::collections::HashMap;

    #[test]
    fn set_transport_uri_contains_escaped_values() {
        let body = build_set_av_transport_uri_envelope(
            0,
            "http://server.local/stream/this&that.flac",
            "audio/flac",
            Some("Fish & Chips"),
            Some("http://server.local/art/cover one&two.jpg"),
        );

        assert!(body.contains("SetAVTransportURI"));
        assert!(body.contains("this&amp;that.flac"));
        assert!(body.contains("&lt;DIDL-Lite"));
        assert!(body.contains("Fish &amp;amp; Chips"));
        assert!(body.contains("audio/flac"));
        assert!(body.contains("albumArtURI"));
        assert!(body.contains("cover one&amp;amp;two.jpg"));
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
    fn seek_envelope_uses_rel_time_target() {
        let body = build_seek_envelope(0, 71);
        assert!(body.contains("<u:Seek"));
        assert!(body.contains("<Unit>REL_TIME</Unit>"));
        assert!(body.contains("<Target>0:01:11</Target>"));
    }

    #[test]
    fn format_upnp_time_pads_minutes_and_seconds() {
        assert_eq!(format_upnp_time(0), "0:00:00");
        assert_eq!(format_upnp_time(9), "0:00:09");
        assert_eq!(format_upnp_time(3661), "1:01:01");
        assert_eq!(format_upnp_time(3600 * 12 + 34 * 60 + 56), "12:34:56");
    }

    #[test]
    fn set_next_transport_uri_contains_escaped_values() {
        let body = build_set_next_av_transport_uri_envelope(
            0,
            "http://server.local/stream/next&track.flac",
            "audio/flac",
            Some("Next & Best"),
            Some("http://server.local/art/next one&two.jpg"),
        );

        assert!(body.contains("SetNextAVTransportURI"));
        assert!(body.contains("next&amp;track.flac"));
        assert!(body.contains("&lt;DIDL-Lite"));
        assert!(body.contains("Next &amp;amp; Best"));
        assert!(body.contains("audio/flac"));
        assert!(body.contains("albumArtURI"));
        assert!(body.contains("next one&amp;amp;two.jpg"));
    }

    #[test]
    fn clear_next_transport_uri_sends_empty_next_slot() {
        let body = build_clear_next_av_transport_uri_envelope(0);

        assert!(body.contains("SetNextAVTransportURI"));
        assert!(body.contains("<NextURI></NextURI>"));
        assert!(body.contains("<NextURIMetaData></NextURIMetaData>"));
        assert!(!body.contains("DIDL-Lite"));
    }

    #[test]
    fn transport_info_envelope_contains_action() {
        let body = build_get_transport_info_envelope(0);
        assert!(body.contains("GetTransportInfo"));
    }

    #[test]
    fn decodes_playlist_extension_id_array() {
        let ids = decode_playlist_id_array(
            "AMgnGADFVbgAciBoAMnSmADE33ABMTcAAFfFCAEFsoAAw5Z4APVUIADIWdgAxYOgAL9/SADEHxA=",
        )
        .expect("id array should decode");

        assert_eq!(
            ids,
            vec![
                13117208, 12932536, 7479400, 13226648, 12902256, 20002560, 5752072, 17150592,
                12818040, 16077856, 13130200, 12944288, 12549960, 12853008
            ]
        );
    }

    #[test]
    fn parses_playlist_extension_metadata_entries() {
        let xml = r#"<MetaDataList><Entry><Id>7479400l</Id>
<Uri>http://server.local/stream/track/one</Uri>
<MetaData>&lt;DIDL-Lite xmlns:dc="http://purl.org/dc/elements/1.1/"&gt;&lt;item&gt;&lt;dc:title&gt;The Opposite of Hallelujah&lt;/dc:title&gt;&lt;/item&gt;&lt;/DIDL-Lite&gt;</MetaData>
</Entry></MetaDataList>"#;

        let entries = parse_playlist_metadata_list(xml);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, 7479400);
        assert_eq!(entries[0].uri, "http://server.local/stream/track/one");
        assert_eq!(
            entries[0].title.as_deref(),
            Some("The Opposite of Hallelujah")
        );
    }

    #[test]
    fn playlist_extension_insert_envelope_escapes_values() {
        let body = super::build_action_envelope(
            super::PLAYLIST_EXTENSION_SERVICE,
            "Insert",
            &[
                ("aAfterId", "123"),
                ("aUri", "http://server.local/stream/track/one&two"),
                (
                    "aMetaData",
                    "<DIDL-Lite><dc:title>A & B</dc:title></DIDL-Lite>",
                ),
            ],
        );

        assert!(body.contains("<u:Insert"));
        assert!(body.contains("<aAfterId>123</aAfterId>"));
        assert!(body.contains("one&amp;two"));
        assert!(body.contains("&lt;DIDL-Lite&gt;"));
        assert!(body.contains("A &amp; B"));
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
    fn prefers_nested_media_renderer_device_when_present() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <device>
    <deviceType>urn:schemas-upnp-org:device:ZonePlayer:1</deviceType>
    <friendlyName>Top Level ZonePlayer</friendlyName>
    <manufacturer>Sonos, Inc.</manufacturer>
    <modelName>SYMFONISK Table lamp</modelName>
    <deviceList>
      <device>
        <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
        <friendlyName>Bedroom - SYMFONISK Table lamp Media Renderer</friendlyName>
        <manufacturer>Sonos, Inc.</manufacturer>
        <modelName>SYMFONISK Table lamp</modelName>
        <serviceList>
          <service>
            <serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>
            <serviceId>urn:upnp-org:serviceId:AVTransport</serviceId>
            <controlURL>/MediaRenderer/AVTransport/Control</controlURL>
            <eventSubURL>/MediaRenderer/AVTransport/Event</eventSubURL>
            <SCPDURL>/xml/AVTransport1.xml</SCPDURL>
          </service>
          <service>
            <serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>
            <serviceId>urn:upnp-org:serviceId:RenderingControl</serviceId>
            <controlURL>/MediaRenderer/RenderingControl/Control</controlURL>
            <eventSubURL>/MediaRenderer/RenderingControl/Event</eventSubURL>
            <SCPDURL>/xml/RenderingControl1.xml</SCPDURL>
          </service>
        </serviceList>
      </device>
    </deviceList>
  </device>
</root>"#;

        let description =
            parse_device_description("http://192.168.1.251:1400/xml/device_description.xml", xml)
                .unwrap();
        assert_eq!(
            description.friendly_name,
            "Bedroom - SYMFONISK Table lamp Media Renderer"
        );
        assert_eq!(
            description.device_type,
            "urn:schemas-upnp-org:device:MediaRenderer:1"
        );
        assert_eq!(
            description.services[0].control_url,
            "http://192.168.1.251:1400/MediaRenderer/AVTransport/Control"
        );
    }

    #[test]
    fn parses_service_actions_sorted_and_unique() {
        let xml = r#"
<scpd>
  <actionList>
    <action><name>Pause</name></action>
    <action><name>SetNextAVTransportURI</name></action>
    <action><name>Pause</name></action>
    <action><name>Next</name></action>
  </actionList>
</scpd>
        "#;

        assert_eq!(
            parse_service_actions(xml),
            vec![
                "Next".to_string(),
                "Pause".to_string(),
                "SetNextAVTransportURI".to_string()
            ]
        );
    }

    #[test]
    fn parses_service_action_arguments() {
        let xml = r#"
<scpd>
  <actionList>
    <action>
      <name>Search</name>
      <argumentList>
        <argument>
          <name>aSearchString</name>
          <direction>in</direction>
          <relatedStateVariable>A_ARG_TYPE_String</relatedStateVariable>
        </argument>
        <argument>
          <name>aResult</name>
          <direction>out</direction>
          <relatedStateVariable>A_ARG_TYPE_Result</relatedStateVariable>
        </argument>
      </argumentList>
    </action>
  </actionList>
</scpd>
        "#;

        let actions = parse_service_action_descriptions(xml);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].name, "Search");
        assert_eq!(actions[0].arguments.len(), 2);
        assert_eq!(actions[0].arguments[0].name, "aSearchString");
        assert_eq!(actions[0].arguments[0].direction.as_deref(), Some("in"));
        assert_eq!(
            actions[0].arguments[1].related_state_variable.as_deref(),
            Some("A_ARG_TYPE_Result")
        );
    }

    #[test]
    fn extracts_generic_namespaced_action_response_values() {
        let xml = r#"
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:SearchResponse xmlns:u="urn:UuVol-com:service:SMSearch:1">
      <aResult>Fish &amp; Chips</aResult>
      <aCount>2</aCount>
    </u:SearchResponse>
  </s:Body>
</s:Envelope>"#;

        assert_eq!(
            super::extract_generic_action_values("Search", xml),
            vec![
                ("aResult".to_string(), "Fish & Chips".to_string()),
                ("aCount".to_string(), "2".to_string())
            ]
        );
    }

    #[test]
    fn selects_nested_renderer_section() {
        let xml = r#"
<device>
  <deviceType>urn:schemas-upnp-org:device:ZonePlayer:1</deviceType>
  <deviceList>
    <device>
      <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>
      <friendlyName>Renderer Child</friendlyName>
    </device>
  </deviceList>
</device>
        "#;

        let section = select_renderer_device_section(xml);
        assert_eq!(
            super::extract_first_tag(section, "friendlyName"),
            Some("Renderer Child")
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
