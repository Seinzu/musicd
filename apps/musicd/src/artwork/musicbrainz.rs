use std::collections::HashSet;
use std::io;
use std::sync::OnceLock;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT, CONTENT_TYPE};

use crate::types::{
    AlbumArtworkSearchCandidate, CoverArtArchiveResponse, MusicBrainzArtistCredit,
    MusicBrainzSearchResponse,
};

use super::DownloadedArtwork;
use super::mime::{infer_image_mime_from_bytes, infer_image_mime_from_url};

/// Shared `reqwest::blocking::Client` for MusicBrainz, Cover Art Archive, and
/// remote artwork downloads. Built once on first use; subsequent calls reuse
/// the connection pool (and tokio runtime that backs blocking reqwest).
pub(crate) fn musicbrainz_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(musicbrainz_user_agent())
            .redirect(reqwest::redirect::Policy::limited(10))
            .timeout(Duration::from_secs(12))
            .pool_idle_timeout(Some(Duration::from_secs(60)))
            .build()
            .expect("failed to build MusicBrainz reqwest client")
    })
}

fn musicbrainz_user_agent() -> String {
    format!(
        "musicd/{} (self-hosted local music library app)",
        env!("CARGO_PKG_VERSION")
    )
}

pub(crate) fn search_musicbrainz_album_artwork(
    client: &Client,
    artist: &str,
    album: &str,
) -> io::Result<Vec<AlbumArtworkSearchCandidate>> {
    let query = format!(
        "release:\"{}\" AND artist:\"{}\"",
        lucene_escape_phrase(album),
        lucene_escape_phrase(artist),
    );
    let response = client
        .get("https://musicbrainz.org/ws/2/release")
        .query(&[("query", query.as_str()), ("fmt", "json"), ("limit", "8")])
        .header(ACCEPT, "application/json")
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(io::Error::other)?;

    let search: MusicBrainzSearchResponse = response.json().map_err(io::Error::other)?;
    let mut seen_release_ids = HashSet::new();
    let mut candidates = Vec::new();

    for release in search.releases {
        if !seen_release_ids.insert(release.id.clone()) {
            continue;
        }
        if let Some(cover) = fetch_musicbrainz_cover_art_for_release(client, &release.id)? {
            candidates.push(AlbumArtworkSearchCandidate {
                release_id: release.id,
                release_group_id: release.release_group.map(|group| group.id),
                title: release.title,
                artist: artist_credit_name(&release.artist_credit),
                date: release.date,
                country: release.country,
                score: release.score.unwrap_or_default(),
                thumbnail_url: cover.thumbnail_url,
                image_url: cover.image_url,
                source: cover.source,
            });
        }
        if candidates.len() >= 3 {
            break;
        }
    }

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.date.cmp(&right.date))
            .then_with(|| left.country.cmp(&right.country))
    });
    Ok(candidates)
}

pub(crate) fn fetch_musicbrainz_cover_art_for_release(
    client: &Client,
    release_id: &str,
) -> io::Result<Option<AlbumArtworkSearchCandidate>> {
    let response = client
        .get(format!("https://coverartarchive.org/release/{release_id}/"))
        .header(ACCEPT, "application/json")
        .send()
        .map_err(io::Error::other)?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    let response = response.error_for_status().map_err(io::Error::other)?;
    let archive: CoverArtArchiveResponse = response.json().map_err(io::Error::other)?;
    let Some(image) = archive
        .images
        .into_iter()
        .find(|image| image.front || image.approved)
    else {
        return Ok(None);
    };

    let thumbnail_url = image
        .thumbnails
        .get("250")
        .or_else(|| image.thumbnails.get("small"))
        .cloned()
        .unwrap_or_else(|| image.image.clone());

    Ok(Some(AlbumArtworkSearchCandidate {
        release_id: release_id.to_string(),
        release_group_id: None,
        title: String::new(),
        artist: String::new(),
        date: None,
        country: None,
        score: 0,
        thumbnail_url,
        image_url: image.image,
        source: archive
            .release
            .unwrap_or_else(|| format!("MusicBrainz release {release_id}")),
    }))
}

pub(crate) fn download_artwork_candidate(
    client: &Client,
    image_url: &str,
) -> io::Result<DownloadedArtwork> {
    let response = client
        .get(image_url)
        .header(ACCEPT, "image/*")
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(io::Error::other)?;

    let mime_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| value.starts_with("image/"))
        .map(ToString::to_string)
        .or_else(|| infer_image_mime_from_url(image_url).map(ToString::to_string));
    let bytes = response.bytes().map_err(io::Error::other)?.to_vec();
    let mime_type = mime_type
        .or_else(|| infer_image_mime_from_bytes(&bytes).map(ToString::to_string))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown artwork MIME type"))?;

    Ok(DownloadedArtwork { bytes, mime_type })
}

fn artist_credit_name(credits: &[MusicBrainzArtistCredit]) -> String {
    credits
        .iter()
        .filter_map(|credit| credit.name.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn lucene_escape_phrase(value: &str) -> String {
    value.replace('\\', r#"\\"#).replace('"', r#"\""#)
}
