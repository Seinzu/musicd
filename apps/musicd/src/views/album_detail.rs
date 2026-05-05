use crate::http::HttpRequest;
use crate::service::ServiceState;
use crate::util::{format_track_position, html_escape, url_encode};

use super::error::render_detail_error_page;

pub(crate) fn render_album_detail_page(state: &ServiceState, request: &HttpRequest) -> String {
    let album_id = request.path.trim_start_matches("/album/");
    let renderer_location = state
        .preferred_renderer_location(request.query.get("renderer_location").map(String::as_str));
    let message = request.query.get("message").cloned().unwrap_or_default();
    let error = request.query.get("error").cloned().unwrap_or_default();

    let Some(album) = state.find_album(album_id) else {
        return render_detail_error_page("Album not found");
    };

    let tracks = state.tracks_for_album(&album.id);
    let artwork_html = album
        .artwork_url
        .as_ref()
        .map(|artwork_url| {
            format!(
                "<img class=\"detail-artwork\" src=\"{}\" alt=\"Artwork for {}\">",
                html_escape(artwork_url),
                html_escape(&album.title)
            )
        })
        .unwrap_or_else(|| {
            "<div class=\"detail-artwork placeholder\">No album artwork found yet.</div>"
                .to_string()
        });
    let message_html = if message.is_empty() {
        String::new()
    } else {
        format!("<p class=\"banner success\">{}</p>", html_escape(&message))
    };
    let error_html = if error.is_empty() {
        String::new()
    } else {
        format!("<p class=\"banner error\">{}</p>", html_escape(&error))
    };
    let track_rows = tracks
        .iter()
        .map(|track| {
            let play_url = format!(
                "/play?track_id={}&renderer_location={}",
                url_encode(&track.id),
                url_encode(&renderer_location)
            );
            let play_next_url = format!(
                "/queue/play-next-track?track_id={}&renderer_location={}&return_to=%2Falbum%2F{}",
                url_encode(&track.id),
                url_encode(&renderer_location),
                url_encode(&album.id)
            );
            let queue_url = format!(
                "/queue/append-track?track_id={}&renderer_location={}&return_to=%2Falbum%2F{}",
                url_encode(&track.id),
                url_encode(&renderer_location),
                url_encode(&album.id)
            );
            format!(
                "<tr><td data-label=\"Position\">{}</td><td data-label=\"Title\">{}</td><td data-label=\"Artist\">{}</td><td data-label=\"Actions\" class=\"actions-cell\"><a href=\"{}\">Play Track</a> <span class=\"muted-sep\">|</span> <a href=\"{}\">Play Next</a> <span class=\"muted-sep\">|</span> <a href=\"{}\">Queue</a> <span class=\"muted-sep\">|</span> <a href=\"/track/{}?renderer_location={}\" target=\"_blank\" rel=\"noreferrer\">Inspect</a></td></tr>",
                html_escape(&format_track_position(track.disc_number, track.track_number)),
                html_escape(&track.title),
                html_escape(&track.artist),
                html_escape(&play_url),
                html_escape(&play_next_url),
                html_escape(&queue_url),
                html_escape(&track.id),
                html_escape(&renderer_location),
            )
        })
        .collect::<Vec<_>>()
        .join("");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{}</title>
  <style>
    :root {{
      --bg: #f8f4eb;
      --panel: #fffdf8;
      --ink: #1f1a17;
      --muted: #6f665f;
      --line: rgba(31, 26, 23, 0.12);
      --accent: #166534;
      --danger: #991b1b;
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
    .layout {{
      display: grid;
      grid-template-columns: minmax(0, 18rem) minmax(0, 1fr);
      gap: 1.5rem;
      align-items: start;
    }}
    .detail-artwork {{
      width: 100%;
      display: block;
      border-radius: 18px;
      border: 1px solid var(--line);
      box-shadow: 0 14px 28px rgba(31, 26, 23, 0.12);
      background: rgba(31, 26, 23, 0.05);
      min-height: 18rem;
      object-fit: cover;
    }}
    .detail-artwork.placeholder {{
      display: flex;
      align-items: center;
      justify-content: center;
      padding: 1rem;
      text-align: center;
    }}
    .actions {{
      display: flex;
      gap: 0.75rem;
      flex-wrap: wrap;
      margin-top: 1rem;
    }}
    .actions-cell {{
      line-height: 1.8;
    }}
    .actions a, .actions button {{
      text-decoration: none;
      color: white;
      background: #1f1a17;
      padding: 0.75rem 1rem;
      border-radius: 999px;
      border: 0;
      font: inherit;
      cursor: pointer;
    }}
    .actions a.secondary {{
      color: #1f1a17;
      background: #eadfce;
    }}
    input[type="text"] {{
      width: 100%;
      border: 1px solid var(--line);
      background: #fffdfa;
      border-radius: 12px;
      padding: 0.8rem 0.9rem;
      font: inherit;
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
      color: var(--muted);
      font-weight: 600;
    }}
    .banner {{
      margin: 0 1.5rem 1rem;
      padding: 0.9rem 1rem;
      border-radius: 14px;
    }}
    .banner.success {{
      background: rgba(22, 101, 52, 0.1);
      color: var(--accent);
    }}
    .banner.error {{
      background: rgba(153, 27, 27, 0.08);
      color: var(--danger);
    }}
    @media (max-width: 760px) {{
      .layout {{
        grid-template-columns: 1fr;
      }}
      main {{
        width: calc(100vw - 1rem);
        margin: 0.5rem auto 1rem;
        border-radius: 18px;
      }}
      header, section {{
        padding: 1rem;
      }}
      .actions a, .actions button {{
        width: 100%;
        justify-content: center;
        text-align: center;
      }}
      table {{
        display: block;
      }}
      thead {{
        display: none;
      }}
      tbody {{
        display: grid;
        gap: 0.8rem;
      }}
      tbody tr {{
        display: grid;
        gap: 0.65rem;
        padding: 0.9rem;
        border: 1px solid var(--line);
        border-radius: 16px;
        background: rgba(255, 255, 255, 0.72);
      }}
      th, td {{
        padding: 0;
        border: 0;
      }}
      td {{
        display: grid;
        grid-template-columns: 5.3rem minmax(0, 1fr);
        gap: 0.7rem;
      }}
      td::before {{
        content: attr(data-label);
        color: var(--muted);
        font-size: 0.82rem;
        text-transform: uppercase;
        letter-spacing: 0.05em;
      }}
      .actions-cell a {{
        display: block;
        padding: 0.65rem 0.8rem;
        border-radius: 12px;
        margin-bottom: 0.4rem;
        background: rgba(31, 26, 23, 0.08);
      }}
      .muted-sep {{
        display: none;
      }}
    }}
  </style>
</head>
<body>
  <main>
    <header>
      <h1>{}</h1>
      <p>{} tracks • {}</p>
      <div class="actions">
        <a href="/">Back to Library</a>
      </div>
    </header>
    {}{}
    <section class="layout">
      <div>{}</div>
      <div>
        <h2>Play Album</h2>
        <p>Play the album now, place it next in line, or add it to the end of the queue.</p>
        <form action="/play-album" method="get">
          <input type="hidden" name="album_id" value="{}">
          <label for="renderer_location" style="display:block; font-weight:600; margin-bottom:0.5rem;">Renderer LOCATION</label>
          <input id="renderer_location" name="renderer_location" type="text" value="{}" placeholder="http://192.168.1.55:49152/description.xml">
          <div class="actions">
            <button type="submit">Play Album</button>
            <button type="submit" formaction="/queue/play-next-album">Play Next</button>
            <button type="submit" formaction="/queue/append-album">Queue Album</button>
            <input type="hidden" name="return_to" value="/album/{}">
            <a class="secondary" href="/stream/track/{}" target="_blank" rel="noreferrer">Preview First Track</a>
          </div>
        </form>
      </div>
    </section>
    <section>
      <h2>Tracks</h2>
      <table>
        <thead>
          <tr>
            <th>Position</th>
            <th>Title</th>
            <th>Artist</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody>{}</tbody>
      </table>
    </section>
  </main>
</body>
</html>"#,
        html_escape(&album.title),
        html_escape(&album.title),
        album.track_count,
        html_escape(&album.artist),
        message_html,
        error_html,
        artwork_html,
        html_escape(&album.id),
        html_escape(&renderer_location),
        html_escape(&album.id),
        html_escape(&album.first_track_id),
        track_rows,
    )
}

