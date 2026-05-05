use crate::http::HttpRequest;
use crate::library::inspect_embedded_metadata;
use crate::service::ServiceState;
use crate::types::EmbeddedMetadata;
use crate::util::{format_duration_seconds, format_track_position, html_escape, url_encode};

use super::error::render_detail_error_page;

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
  <style>
    :root {{
      --bg: #f8f4eb;
      --panel: #fffdf8;
      --ink: #1f1a17;
      --muted: #6f665f;
      --line: rgba(31, 26, 23, 0.12);
    }}
    body {{
      margin: 0;
      font-family: Georgia, "Iowan Old Style", serif;
      background: linear-gradient(180deg, #f7f0e2 0%, #fdfaf2 100%);
      color: var(--ink);
    }}
    main {{
      width: min(980px, calc(100vw - 2rem));
      margin: 1.5rem auto 3rem;
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 20px;
      overflow: hidden;
      box-shadow: 0 18px 42px rgba(31, 26, 23, 0.1);
    }}
    header, section {{
      padding: 1.4rem 1.5rem;
    }}
    header {{
      border-bottom: 1px solid var(--line);
      background: rgba(22, 101, 52, 0.06);
    }}
    h1, h2 {{
      margin: 0 0 0.6rem;
    }}
    p {{
      margin: 0.25rem 0;
      color: var(--muted);
    }}
    table {{
      width: 100%;
      border-collapse: collapse;
    }}
    th, td {{
      text-align: left;
      vertical-align: top;
      border-top: 1px solid var(--line);
      padding: 0.85rem 0.9rem;
    }}
    th {{
      width: 14rem;
      color: var(--muted);
      font-weight: 600;
    }}
    code {{
      font-family: "SFMono-Regular", Menlo, monospace;
      font-size: 0.95em;
      word-break: break-word;
    }}
    .actions {{
      display: flex;
      gap: 0.75rem;
      flex-wrap: wrap;
    }}
    .metadata-table td {{
      word-break: break-word;
    }}
    .actions a {{
      text-decoration: none;
      color: white;
      background: #1f1a17;
      padding: 0.75rem 1rem;
      border-radius: 999px;
    }}
    .actions a.secondary {{
      color: #1f1a17;
      background: #eadfce;
    }}
    .detail-artwork {{
      width: min(18rem, 100%);
      display: block;
      border-radius: 18px;
      border: 1px solid var(--line);
      box-shadow: 0 14px 28px rgba(31, 26, 23, 0.12);
    }}
    @media (max-width: 760px) {{
      main {{
        width: calc(100vw - 1rem);
        margin: 0.5rem auto 1rem;
        border-radius: 18px;
      }}
      header, section {{
        padding: 1rem;
      }}
      .actions a {{
        width: 100%;
        text-align: center;
      }}
      table {{
        display: block;
      }}
      tbody {{
        display: grid;
        gap: 0.8rem;
      }}
      tr {{
        display: grid;
        gap: 0.4rem;
        padding: 0.9rem;
        border: 1px solid var(--line);
        border-radius: 16px;
        background: rgba(255, 255, 255, 0.72);
      }}
      th, td {{
        display: block;
        width: auto;
        padding: 0;
        border: 0;
      }}
      th {{
        font-size: 0.82rem;
        text-transform: uppercase;
        letter-spacing: 0.05em;
      }}
      code {{
        white-space: pre-wrap;
      }}
    }}
  </style>
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
        inferred_rows,
        artwork_html,
        html_escape(&metadata.format_name),
        embedded_rows,
        notes_html,
    )
}

