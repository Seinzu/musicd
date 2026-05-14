use std::io;
use std::time::Duration;

use musicd_upnp::StreamResource;
use reqwest::Url;
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::renderer::{RendererKind, renderer_kind_for_location};
use crate::types::RadioStation;
use crate::util::url_encode;

use super::ServiceState;

const RADIO_BROWSER_TIMEOUT: Duration = Duration::from_secs(8);
const RADIO_BROWSER_USER_AGENT: &str = "musicd/0.1 (+https://github.com/musicd)";

#[derive(Debug, Deserialize)]
struct RadioBrowserStation {
    #[serde(default)]
    stationuuid: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    url_resolved: String,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    favicon: Option<String>,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    countrycode: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    codec: Option<String>,
    #[serde(default)]
    bitrate: Option<u32>,
    #[serde(default)]
    votes: Option<u32>,
    #[serde(default)]
    clickcount: Option<u32>,
}

impl ServiceState {
    pub(crate) fn search_radio_stations(
        &self,
        query: Option<&str>,
        country_code: Option<&str>,
        tag: Option<&str>,
        limit: usize,
    ) -> io::Result<Vec<RadioStation>> {
        let base_url = self.radio_browser_base_url()?;
        let limit = limit.clamp(1, 50).to_string();
        let has_filters = query.is_some_and(|value| !value.trim().is_empty())
            || country_code.is_some_and(|value| !value.trim().is_empty())
            || tag.is_some_and(|value| !value.trim().is_empty());
        let mut url = if has_filters {
            base_url
                .join("/json/stations/search")
                .map_err(radio_browser_url_error)?
        } else {
            base_url
                .join(&format!("/json/stations/topclick/{limit}"))
                .map_err(radio_browser_url_error)?
        };

        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("hidebroken", "true");
            pairs.append_pair("limit", &limit);
            if has_filters {
                pairs.append_pair("order", "clickcount");
                pairs.append_pair("reverse", "true");
            }
            if let Some(query) = query.map(str::trim).filter(|value| !value.is_empty()) {
                pairs.append_pair("name", query);
            }
            if let Some(country_code) = country_code
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                pairs.append_pair("countrycode", &country_code.to_ascii_uppercase());
            }
            if let Some(tag) = tag.map(str::trim).filter(|value| !value.is_empty()) {
                pairs.append_pair("tag", tag);
            }
        }

        let client = radio_browser_client()?;
        let stations = client
            .get(url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(radio_browser_request_error)?
            .json::<Vec<RadioBrowserStation>>()
            .map_err(radio_browser_request_error)?;

        Ok(stations
            .into_iter()
            .filter_map(RadioStation::from_radio_browser)
            .take(limit.parse::<usize>().unwrap_or(25))
            .collect())
    }

    pub(crate) fn play_radio_stream(
        &self,
        renderer_location: &str,
        stream_url: &str,
        title: &str,
        codec: Option<&str>,
        artwork_url: Option<&str>,
        station_id: Option<&str>,
    ) -> io::Result<String> {
        let stream_url = normalized_http_stream_url(stream_url)?;
        let title = title.trim();
        let title = if title.is_empty() {
            stream_url.as_str()
        } else {
            title
        };
        let resource = StreamResource {
            stream_url: stream_url.clone(),
            mime_type: mime_type_for_radio_codec(codec),
            title: title.to_string(),
            album_art_url: artwork_url.and_then(|value| normalized_http_stream_url(value).ok()),
        };

        if matches!(
            renderer_kind_for_location(renderer_location),
            RendererKind::Group
        ) {
            let group = self.load_renderer_group_for_queue(renderer_location)?;
            let fanout = self.play_stream_on_group_members(&group, &resource)?;
            self.database.mark_direct_stream_play_started(
                renderer_location,
                &stream_url,
                title,
                resource.album_art_url.as_deref(),
            )?;
            self.record_group_session_warning(renderer_location, "radio-start", &fanout);
            self.events.touch(renderer_location);
            self.count_radio_station_click(station_id);
            let renderer_name = if fanout.succeeded_count() == fanout.total_count() {
                format!("{} ({} renderers)", group.name, fanout.succeeded_count())
            } else {
                format!(
                    "{} ({} of {} renderers)",
                    group.name,
                    fanout.succeeded_count(),
                    fanout.total_count()
                )
            };
            return Ok(renderer_name);
        }

        let renderer = self.resolve_renderer(renderer_location)?;
        match self
            .renderer_backend(renderer_location)?
            .play_stream(&renderer, &resource)
        {
            Ok(()) => {
                let _ = self.mark_renderer_reachable(&renderer);
                self.database.mark_direct_stream_play_started(
                    renderer_location,
                    &stream_url,
                    title,
                    resource.album_art_url.as_deref(),
                )?;
                self.events.touch(renderer_location);
                self.count_radio_station_click(station_id);
                Ok(renderer.name)
            }
            Err(error) => {
                let _ = self.mark_renderer_unreachable(renderer_location, &error);
                let _ = self.database.mark_queue_play_error(
                    renderer_location,
                    None,
                    &error.to_string(),
                );
                Err(error)
            }
        }
    }

    fn radio_browser_base_url(&self) -> io::Result<Url> {
        Url::parse(&self.config.radio_browser_base_url)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
    }

    fn count_radio_station_click(&self, station_id: Option<&str>) {
        let Some(station_id) = station_id.map(str::trim).filter(|value| !value.is_empty()) else {
            return;
        };
        let Ok(base_url) = self.radio_browser_base_url() else {
            return;
        };
        let Ok(url) = base_url.join(&format!("/json/url/{}", url_encode(station_id))) else {
            return;
        };
        std::thread::spawn(move || {
            let _ = radio_browser_client().and_then(|client| {
                client
                    .get(url)
                    .send()
                    .and_then(|response| response.error_for_status())
                    .map(|_| ())
                    .map_err(radio_browser_request_error)
            });
        });
    }
}

impl RadioStation {
    fn from_radio_browser(station: RadioBrowserStation) -> Option<Self> {
        let stream_url = if station.url_resolved.trim().is_empty() {
            station.url
        } else {
            station.url_resolved
        };
        let stream_url = normalized_http_stream_url(&stream_url).ok()?;
        let name = station.name.trim();
        if name.is_empty() {
            return None;
        }
        let tags = station
            .tags
            .split(',')
            .map(str::trim)
            .filter(|tag| !tag.is_empty())
            .take(8)
            .map(ToString::to_string)
            .collect();

        Some(Self {
            id: station.stationuuid,
            name: name.to_string(),
            stream_url,
            homepage_url: station
                .homepage
                .and_then(|value| normalized_http_stream_url(&value).ok()),
            artwork_url: station
                .favicon
                .and_then(|value| normalized_http_stream_url(&value).ok()),
            tags,
            country_code: station.countrycode,
            language: station.language,
            codec: station.codec,
            bitrate: station.bitrate,
            votes: station.votes,
            click_count: station.clickcount,
        })
    }
}

fn radio_browser_client() -> io::Result<Client> {
    Client::builder()
        .timeout(RADIO_BROWSER_TIMEOUT)
        .user_agent(RADIO_BROWSER_USER_AGENT)
        .build()
        .map_err(radio_browser_request_error)
}

fn normalized_http_stream_url(value: &str) -> io::Result<String> {
    let parsed = Url::parse(value.trim())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed.to_string()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "radio streams must use http or https",
        )),
    }
}

fn mime_type_for_radio_codec(codec: Option<&str>) -> String {
    match codec
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "aac" | "aac+" | "heaac" | "he-aac" => "audio/aac",
        "ogg" | "vorbis" => "audio/ogg",
        "opus" => "audio/ogg; codecs=opus",
        "flac" => "audio/flac",
        "mp3" | "mpeg" => "audio/mpeg",
        _ => "audio/mpeg",
    }
    .to_string()
}

fn radio_browser_url_error(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, error.to_string())
}

fn radio_browser_request_error(error: reqwest::Error) -> io::Error {
    io::Error::other(format!("radio browser request failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{mime_type_for_radio_codec, normalized_http_stream_url};

    #[test]
    fn accepts_http_radio_stream_urls() {
        assert_eq!(
            normalized_http_stream_url(" https://example.com/live.mp3 ").unwrap(),
            "https://example.com/live.mp3"
        );
        assert!(normalized_http_stream_url("file:///tmp/live.mp3").is_err());
    }

    #[test]
    fn maps_common_radio_codecs_to_audio_mime_types() {
        assert_eq!(mime_type_for_radio_codec(Some("MP3")), "audio/mpeg");
        assert_eq!(mime_type_for_radio_codec(Some("AAC+")), "audio/aac");
        assert_eq!(
            mime_type_for_radio_codec(Some("opus")),
            "audio/ogg; codecs=opus"
        );
        assert_eq!(mime_type_for_radio_codec(None), "audio/mpeg");
    }
}
