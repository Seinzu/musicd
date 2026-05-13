use crate::assets;
use crate::http::HttpRequest;
use crate::library::inspect_embedded_metadata;
use crate::service::ServiceState;
use crate::types::EmbeddedMetadata;
use crate::util::{format_duration_seconds, format_track_position, html_escape, url_encode};

use super::error::render_detail_error_page;
use super::library::render_like_button;

pub(crate) fn render_track_detail_page(state: &ServiceState, request: &HttpRequest) -> String {
    let track_id = request.path.trim_start_matches("/track/");
    let renderer_location = request
        .query
        .get("renderer_location")
        .cloned()
        .or_else(|| state.config.default_renderer_location.clone())
        .unwrap_or_default();

    let Some(track) = state.find_track(track_id) else {
        return render_detail_error_page("Track not found");
    };
    let like_count = state
        .track_like_counts()
        .get(&track.id)
        .copied()
        .unwrap_or(0);

    let metadata =
        inspect_embedded_metadata(&track.path).unwrap_or_else(|error| EmbeddedMetadata {
            format_name: "Unreadable".to_string(),
            fields: Vec::new(),
            notes: vec![format!("Failed to inspect embedded metadata: {error}")],
        });
    let artwork_url = state.relative_artwork_url_for_track(&track);

    let inferred_rows = [
        ("Track ID", track.id.clone()),
        ("Album ID", track.album_id.clone()),
        ("Title", track.title.clone()),
        ("Artist", track.artist.clone()),
        ("Album", track.album.clone()),
        (
            "Disc / Track",
            format_track_position(track.disc_number, track.track_number),
        ),
        (
            "Duration",
            track
                .duration_seconds
                .map(format_duration_seconds)
                .unwrap_or_else(|| "Unknown".to_string()),
        ),
        ("Relative path", track.relative_path.clone()),
        ("Absolute path", track.path.display().to_string()),
        ("MIME type", track.mime_type.clone()),
        ("File size", format!("{} bytes", track.file_size)),
        (
            "Artwork URL",
            artwork_url.clone().unwrap_or_else(|| "None".to_string()),
        ),
        (
            "Artwork source",
            track
                .artwork
                .as_ref()
                .map(|artwork| artwork.source.clone())
                .unwrap_or_else(|| "None".to_string()),
        ),
        (
            "Artwork MIME type",
            track
                .artwork
                .as_ref()
                .map(|artwork| artwork.mime_type.clone())
                .unwrap_or_else(|| "None".to_string()),
        ),
        (
            "Artwork cache key",
            track
                .artwork
                .as_ref()
                .map(|artwork| artwork.cache_key.clone())
                .unwrap_or_else(|| "None".to_string()),
        ),
    ]
    .into_iter()
    .map(|(label, value)| {
        format!(
            "<tr><th>{}</th><td><code>{}</code></td></tr>",
            html_escape(label),
            html_escape(&value)
        )
    })
    .collect::<Vec<_>>()
    .join("");

    let embedded_rows = if metadata.fields.is_empty() {
        "<tr><th>Embedded fields</th><td><em>No parsed embedded fields for this file yet.</em></td></tr>".to_string()
    } else {
        metadata
            .fields
            .iter()
            .map(|(label, value)| {
                format!(
                    "<tr><th>{}</th><td><code>{}</code></td></tr>",
                    html_escape(label),
                    html_escape(value)
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };

    let notes_html = if metadata.notes.is_empty() {
        String::new()
    } else {
        let items = metadata
            .notes
            .iter()
            .map(|note| format!("<li>{}</li>", html_escape(note)))
            .collect::<Vec<_>>()
            .join("");
        format!("<ul>{items}</ul>")
    };

    let play_url = format!(
        "/play?track_id={}&renderer_location={}",
        url_encode(&track.id),
        url_encode(&renderer_location)
    );
    let queue_url = format!(
        "/queue/append-track?track_id={}&renderer_location={}&return_to={}",
        url_encode(&track.id),
        url_encode(&renderer_location),
        url_encode(&format!("/track/{}", track.id))
    );
    let artwork_html = artwork_url
        .map(|url| {
            format!(
                "<section><h2>Artwork</h2><img class=\"detail-artwork\" src=\"{}\" alt=\"Artwork for {}\"></section>",
                html_escape(&url),
                html_escape(&track.album)
            )
        })
        .unwrap_or_default();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Inspect {}</title>
  <link rel="stylesheet" href="/assets/track_detail.css?v={asset_version}">
</head>
<body>
  <main>
    <header>
      <h1>{}</h1>
      <p>{} • {}</p>
      <div class="actions">
        <a href="/">Back to Library</a>
        <a class="secondary" href="/album/{}?renderer_location={}">View Album</a>
        <a class="secondary" href="/stream/track/{}" target="_blank" rel="noreferrer">Preview Stream</a>
        <a class="secondary" href="{}">Queue Track</a>
        <a href="{}">Play On Renderer</a>
        {}
      </div>
    </header>
    <section>
      <h2>Inferred Library Metadata</h2>
      <table class="metadata-table"><tbody>{}</tbody></table>
    </section>
    {}
    <section>
      <h2>Embedded File Metadata</h2>
      <p>Parser: {}</p>
      <table class="metadata-table"><tbody>{}</tbody></table>
      {}
    </section>
  </main>
  <script src="/assets/home.js?v={asset_version}" defer></script>
</body>
</html>"#,
        html_escape(&track.title),
        html_escape(&track.title),
        html_escape(&track.artist),
        html_escape(&track.album),
        html_escape(&track.album_id),
        html_escape(&renderer_location),
        html_escape(&track.id),
        html_escape(&queue_url),
        html_escape(&play_url),
        render_like_button("track", &track.id, like_count),
        inferred_rows,
        artwork_html,
        html_escape(&metadata.format_name),
        embedded_rows,
        notes_html,
        asset_version = assets::VERSION,
    )
}
