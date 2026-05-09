use std::fmt::Write;

use crate::http::HttpRequest;
use crate::service::ServiceState;
use crate::util::{EscapeHtml, html_escape, url_encode};

use super::queue_panel::render_queue_panel;

pub(crate) fn render_home_page(state: &ServiceState, request: &HttpRequest) -> String {
    let library = state.library_snapshot();
    let tracks = &library.tracks;
    let albums = &library.albums;
    let library_path = state.config.library_path.display().to_string();
    let renderer_location = state
        .preferred_renderer_location(request.query.get("renderer_location").map(String::as_str));
    let queue_html = render_queue_panel(state, &renderer_location, &library);
    let known_renderers = state.renderer_snapshot();
    let message = request.query.get("message").cloned().unwrap_or_default();
    let error = request.query.get("error").cloned().unwrap_or_default();

    // Pre-escape values that are constant across rows.
    let renderer_location_escaped = html_escape(&renderer_location);
    let renderer_location_url_encoded = url_encode(&renderer_location);

    let mut album_rows = String::with_capacity(albums.len() * 1024);
    let mut search_text = String::new();
    for album in albums.iter() {
        search_text.clear();
        search_text.reserve(album.title.len() + 1 + album.artist.len());
        search_text.push_str(&album.title);
        search_text.push(' ');
        search_text.push_str(&album.artist);
        search_text.make_ascii_lowercase();

        let album_url = format!(
            "/album/{}?renderer_location={}",
            url_encode(&album.id),
            renderer_location_url_encoded
        );

        write!(
            album_rows,
            "<tr data-search=\"{}\"><td data-label=\"Cover\">",
            EscapeHtml(&search_text)
        )
        .unwrap();
        match album.artwork_url.as_ref() {
            Some(artwork_url) => write!(
                album_rows,
                "<img loading=\"lazy\" class=\"cover-thumb\" src=\"{}\" alt=\"Artwork for {}\">",
                EscapeHtml(artwork_url),
                EscapeHtml(&album.title)
            )
            .unwrap(),
            None => album_rows.push_str("<div class=\"cover-thumb placeholder\">No Art</div>"),
        }
        write!(
            album_rows,
            "</td><td data-label=\"Album\"><a class=\"album-link\" href=\"{album_url}\">{}</a></td>\
             <td data-label=\"Artist\">{}</td><td data-label=\"Tracks\">{tracks_count}</td>\
             <td data-label=\"Actions\" class=\"actions-cell\">\
             <form class=\"inline-form\" action=\"/play-album\" method=\"get\">\
             <input type=\"hidden\" name=\"album_id\" value=\"{album_id}\">\
             <input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{renderer_location_escaped}\">\
             <button type=\"submit\" class=\"secondary\">Play Album</button></form> \
             <span class=\"muted-sep\">|</span> \
             <form class=\"inline-form\" action=\"/queue/play-next-album\" method=\"get\">\
             <input type=\"hidden\" name=\"album_id\" value=\"{album_id}\">\
             <input type=\"hidden\" name=\"return_to\" value=\"/\">\
             <input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{renderer_location_escaped}\">\
             <button type=\"submit\" class=\"secondary\">Play Next</button></form> \
             <span class=\"muted-sep\">|</span> \
             <form class=\"inline-form\" action=\"/queue/append-album\" method=\"get\">\
             <input type=\"hidden\" name=\"album_id\" value=\"{album_id}\">\
             <input type=\"hidden\" name=\"return_to\" value=\"/\">\
             <input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{renderer_location_escaped}\">\
             <button type=\"submit\" class=\"secondary\">Queue Album</button></form> \
             <span class=\"muted-sep\">|</span> \
             <a href=\"{album_url}\">View Album</a></td></tr>",
            EscapeHtml(&album.title),
            EscapeHtml(&album.artist),
            album_url = EscapeHtml(&album_url),
            tracks_count = album.track_count,
            album_id = EscapeHtml(&album.id),
            renderer_location_escaped = renderer_location_escaped,
        )
        .unwrap();
    }

    let mut rows = String::with_capacity(tracks.len() * 1024);
    for track in tracks.iter() {
        search_text.clear();
        search_text.reserve(
            track.title.len()
                + track.artist.len()
                + track.album.len()
                + track.relative_path.len()
                + 3,
        );
        search_text.push_str(&track.title);
        search_text.push(' ');
        search_text.push_str(&track.artist);
        search_text.push(' ');
        search_text.push_str(&track.album);
        search_text.push(' ');
        search_text.push_str(&track.relative_path);
        search_text.make_ascii_lowercase();

        write!(
            rows,
            "<tr data-search=\"{}\">\
             <td data-label=\"Play\"><input type=\"radio\" form=\"playback_form\" name=\"track_id\" value=\"{track_id}\"></td>\
             <td data-label=\"Cover\">",
            EscapeHtml(&search_text),
            track_id = EscapeHtml(&track.id),
        )
        .unwrap();
        match track.artwork.as_ref() {
            Some(_) => write!(
                rows,
                "<img loading=\"lazy\" class=\"cover-thumb\" src=\"/artwork/track/{}\" alt=\"Artwork for {}\">",
                EscapeHtml(&track.id),
                EscapeHtml(&track.album)
            )
            .unwrap(),
            None => rows.push_str("<div class=\"cover-thumb placeholder\">No Art</div>"),
        }
        write!(
            rows,
            "</td><td data-label=\"Title\">{}</td><td data-label=\"Artist\">{}</td>\
             <td data-label=\"Album\"><a class=\"album-link\" href=\"/album/{}?renderer_location={}\">{}</a></td>\
             <td data-label=\"Actions\" class=\"actions-cell\">\
             <form class=\"inline-form\" action=\"/queue/play-next-track\" method=\"get\">\
             <input type=\"hidden\" name=\"track_id\" value=\"{track_id}\">\
             <input type=\"hidden\" name=\"return_to\" value=\"/\">\
             <input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{renderer_location_escaped}\">\
             <button type=\"submit\" class=\"secondary\">Play Next</button></form> \
             <span class=\"muted-sep\">|</span> \
             <form class=\"inline-form\" action=\"/queue/append-track\" method=\"get\">\
             <input type=\"hidden\" name=\"track_id\" value=\"{track_id}\">\
             <input type=\"hidden\" name=\"return_to\" value=\"/\">\
             <input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{renderer_location_escaped}\">\
             <button type=\"submit\" class=\"secondary\">Queue</button></form> \
             <span class=\"muted-sep\">|</span> \
             <a href=\"/stream/track/{track_id}\" target=\"_blank\" rel=\"noreferrer\">Preview</a> \
             <span class=\"muted-sep\">|</span> \
             <a href=\"/track/{track_id}\" target=\"_blank\" rel=\"noreferrer\">Inspect</a>\
             </td></tr>",
            EscapeHtml(&track.title),
            EscapeHtml(&track.artist),
            url_encode(&track.album_id),
            renderer_location_url_encoded,
            EscapeHtml(&track.album),
            track_id = EscapeHtml(&track.id),
            renderer_location_escaped = renderer_location_escaped,
        )
        .unwrap();
    }

    let empty_state = if tracks.is_empty() {
        "<p class=\"empty\">No supported audio files were found under the library path. Add music or verify the volume mapping, then rescan.</p>"
    } else {
        ""
    };

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
    let album_empty_state = if albums.is_empty() {
        "<p class=\"empty\">No albums have been grouped yet. Add music or rescan the library.</p>"
    } else {
        ""
    };
    let renderer_options = if known_renderers.is_empty() {
        "<option value=\"\">Discovered renderers appear here</option>".to_string()
    } else {
        known_renderers
            .iter()
            .map(|renderer| {
                let selected = if renderer.location == renderer_location {
                    " selected"
                } else {
                    ""
                };
                format!(
                    "<option value=\"{}\"{}>{}</option>",
                    html_escape(&renderer.location),
                    selected,
                    html_escape(&renderer.name)
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{}</title>
  <style>
    :root {{
      color-scheme: light;
      --bg: #f5f1e8;
      --panel: rgba(255, 252, 245, 0.92);
      --ink: #1f1a17;
      --muted: #6f665f;
      --accent: #166534;
      --accent-2: #b45309;
      --line: rgba(31, 26, 23, 0.12);
      --danger: #991b1b;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      font-family: Georgia, "Iowan Old Style", serif;
      color: var(--ink);
      background:
        radial-gradient(circle at top left, rgba(22, 101, 52, 0.18), transparent 28rem),
        linear-gradient(160deg, #f8f4eb 0%, #efe5d4 55%, #f7f1e6 100%);
      min-height: 100vh;
    }}
    main {{
      width: min(1100px, calc(100vw - 2rem));
      margin: 2rem auto 3rem;
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 24px;
      box-shadow: 0 24px 60px rgba(31, 26, 23, 0.12);
      overflow: hidden;
    }}
    header {{
      padding: 2rem;
      border-bottom: 1px solid var(--line);
      background: linear-gradient(135deg, rgba(22, 101, 52, 0.12), rgba(180, 83, 9, 0.08));
    }}
    h1 {{
      margin: 0 0 0.4rem;
      font-size: clamp(2rem, 4vw, 3rem);
      line-height: 1;
    }}
    p {{
      margin: 0.25rem 0;
    }}
    .meta {{
      color: var(--muted);
      font-size: 0.98rem;
    }}
    .banner {{
      margin: 1rem 2rem 0;
      padding: 0.9rem 1rem;
      border-radius: 14px;
      font-size: 0.96rem;
    }}
    .banner.success {{
      background: rgba(22, 101, 52, 0.1);
      color: var(--accent);
    }}
    .banner.error {{
      background: rgba(153, 27, 27, 0.08);
      color: var(--danger);
    }}
    .controls {{
      padding: 1.5rem 2rem 0.5rem;
      display: grid;
      gap: 1rem;
    }}
    .control-row {{
      display: flex;
      gap: 0.75rem;
      flex-wrap: wrap;
      align-items: center;
    }}
    label {{
      font-weight: 600;
      min-width: 8rem;
    }}
    input[type="text"] {{
      flex: 1 1 24rem;
      min-width: 18rem;
      border: 1px solid var(--line);
      background: #fffdfa;
      border-radius: 12px;
      padding: 0.8rem 0.9rem;
      font: inherit;
    }}
    button, .button-link {{
      border: 0;
      border-radius: 999px;
      padding: 0.75rem 1.05rem;
      font: inherit;
      cursor: pointer;
      background: var(--ink);
      color: white;
      text-decoration: none;
    }}
    button.secondary {{
      background: #e8dcc9;
      color: var(--ink);
    }}
    select {{
      min-width: 20rem;
      border: 1px solid var(--line);
      background: #fffdfa;
      border-radius: 12px;
      padding: 0.8rem 0.9rem;
      font: inherit;
    }}
    .table-wrap {{
      padding: 0 1rem 1.5rem;
      overflow-x: auto;
    }}
    .library-grid {{
      display: grid;
      grid-template-columns: minmax(0, 1.8fr) minmax(18rem, 0.95fr);
      gap: 1rem;
      align-items: start;
      padding: 0 1rem 1.5rem;
    }}
    .library-grid .table-wrap {{
      padding: 0;
      overflow-x: auto;
    }}
    .track-sidebar {{
      position: sticky;
      top: 1rem;
      border: 1px solid var(--line);
      border-radius: 18px;
      background: rgba(255, 255, 255, 0.74);
      padding: 1rem;
      box-shadow: 0 10px 24px rgba(31, 26, 23, 0.08);
    }}
    .track-sidebar h3 {{
      margin: 0 0 0.35rem;
      font-size: 1.15rem;
    }}
    .track-sidebar p {{
      color: var(--muted);
    }}
    .track-sidebar .sidebar-artwork {{
      width: 100%;
      aspect-ratio: 1;
      object-fit: cover;
      border-radius: 16px;
      border: 1px solid var(--line);
      background: rgba(31, 26, 23, 0.06);
      margin-bottom: 0.9rem;
    }}
    .track-sidebar .sidebar-artwork.placeholder {{
      display: flex;
      align-items: center;
      justify-content: center;
      color: var(--muted);
      font-size: 0.82rem;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }}
    .track-sidebar-meta,
    .track-sidebar-tags {{
      display: grid;
      gap: 0.6rem;
      margin-top: 0.85rem;
    }}
    .track-sidebar-meta-row,
    .track-sidebar-tag {{
      display: grid;
      gap: 0.22rem;
      padding-top: 0.6rem;
      border-top: 1px solid var(--line);
    }}
    .track-sidebar-label {{
      color: var(--muted);
      font-size: 0.78rem;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }}
    .track-sidebar-value,
    .track-sidebar-tag-value {{
      word-break: break-word;
      font-size: 0.95rem;
    }}
    .track-sidebar-tag-value code,
    .track-sidebar-meta-row code {{
      font-family: "SFMono-Regular", Menlo, monospace;
      font-size: 0.92em;
    }}
    .track-sidebar-actions {{
      display: flex;
      gap: 0.6rem;
      flex-wrap: wrap;
      margin-top: 1rem;
    }}
    .track-sidebar-note-list {{
      margin: 0.75rem 0 0;
      padding-left: 1.1rem;
      color: var(--muted);
    }}
    .section-heading {{
      margin: 0;
      padding: 0 2rem 1rem;
      font-size: 1.3rem;
    }}
    .section-note {{
      margin: 0;
      padding: 0 2rem 1rem;
      color: var(--muted);
      font-size: 0.95rem;
    }}
    table {{
      width: 100%;
      border-collapse: collapse;
      margin-top: 0.5rem;
    }}
    th, td {{
      padding: 0.9rem 1rem;
      border-top: 1px solid var(--line);
      text-align: left;
      vertical-align: top;
    }}
    thead th {{
      color: var(--muted);
      font-size: 0.92rem;
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }}
    tbody tr:hover {{
      background: rgba(22, 101, 52, 0.04);
    }}
    .empty {{
      margin: 1rem 2rem 2rem;
      padding: 1rem 1.1rem;
      background: rgba(180, 83, 9, 0.1);
      border-radius: 16px;
      color: #7c4210;
    }}
    .muted-sep {{
      color: var(--muted);
      margin: 0 0.2rem;
    }}
    .cover-thumb {{
      width: 3rem;
      height: 3rem;
      display: block;
      border-radius: 12px;
      object-fit: cover;
      background: rgba(31, 26, 23, 0.08);
      border: 1px solid var(--line);
    }}
    .cover-thumb.placeholder {{
      display: flex;
      align-items: center;
      justify-content: center;
      font-size: 0.68rem;
      color: var(--muted);
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }}
    .album-link {{
      color: inherit;
      text-decoration-thickness: 1px;
      text-underline-offset: 0.15em;
    }}
    .inline-form {{
      display: inline;
    }}
    .actions-cell {{
      line-height: 1.9;
    }}
    @media (max-width: 720px) {{
      main {{
        width: calc(100vw - 1rem);
        margin: 0.5rem auto 1rem;
        border-radius: 18px;
      }}
      header, .controls, .table-wrap {{
        padding-left: 1rem;
        padding-right: 1rem;
      }}
      .section-heading, .section-note, .banner {{
        margin-left: 1rem;
        margin-right: 1rem;
        padding-left: 0;
        padding-right: 0;
      }}
      .control-row {{
        align-items: stretch;
      }}
      .control-row > button,
      .control-row > .button-link,
      .control-row > select,
      .control-row > input[type="text"] {{
        width: 100%;
      }}
      button, .button-link {{
        min-height: 2.9rem;
      }}
      label {{
        min-width: auto;
        width: 100%;
      }}
      input[type="text"], select {{
        min-width: 0;
        width: 100%;
      }}
      .table-wrap {{
        overflow-x: visible;
      }}
      .library-grid {{
        grid-template-columns: 1fr;
        padding-left: 1rem;
        padding-right: 1rem;
      }}
      .track-sidebar {{
        position: static;
        order: -1;
      }}
      table {{
        display: block;
        margin-top: 0;
      }}
      thead {{
        display: none;
      }}
      tbody {{
        display: grid;
        gap: 0.9rem;
      }}
      tbody tr {{
        display: grid;
        gap: 0.65rem;
        padding: 0.95rem;
        border: 1px solid var(--line);
        border-radius: 18px;
        background: rgba(255, 255, 255, 0.7);
      }}
      tbody tr:hover {{
        background: rgba(255, 255, 255, 0.82);
      }}
      td {{
        display: grid;
        grid-template-columns: 5.8rem minmax(0, 1fr);
        gap: 0.75rem;
        padding: 0;
        border: 0;
        align-items: start;
      }}
      td::before {{
        content: attr(data-label);
        color: var(--muted);
        font-size: 0.82rem;
        text-transform: uppercase;
        letter-spacing: 0.05em;
      }}
      td[data-label="Cover"]::before,
      td[data-label="Play"]::before {{
        align-self: center;
      }}
      .cover-thumb {{
        width: 3.5rem;
        height: 3.5rem;
      }}
      .actions-cell {{
        line-height: 1.6;
      }}
      .actions-cell .inline-form {{
        display: block;
        margin-bottom: 0.45rem;
      }}
      .actions-cell button,
      .actions-cell a {{
        width: 100%;
        display: inline-flex;
        justify-content: center;
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
      <p class="meta">Library path: {}</p>
      <p class="meta">Indexed tracks: {}</p>
      <p class="meta">Stream base URL: {}</p>
    </header>
    {}{}
    <section class="controls">
      <form id="playback_form" class="control-row" action="/play" method="get">
        <label for="renderer_location">Renderer LOCATION</label>
        <input id="renderer_location" name="renderer_location" type="text" value="{}" placeholder="http://192.168.1.55:49152/description.xml" oninput="syncRendererFields(this.value)" onchange="refreshQueuePanel()">
        <button type="submit">Play Selected Track</button>
      </form>
      <div class="control-row">
        <button type="button" class="secondary" onclick="discoverRenderers()">Discover Renderers</button>
        <select id="renderer_discovery">
          {}
        </select>
        <button type="button" class="secondary" onclick="applySelectedRenderer()">Use Selected Renderer</button>
      </div>
      <form class="control-row" action="/rescan" method="get">
        <input id="rescan_renderer_location" class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
        <label for="track_filter">Search Tracks</label>
        <input id="track_filter" type="text" placeholder="Filter by title, artist, album, or path" oninput="filterTracks()">
        <button type="submit" class="secondary">Rescan Library</button>
      </form>
    </section>
    {}
    <div id="queue_panel_host">
      {}
    </div>
    <h2 class="section-heading">Albums</h2>
    <p class="section-note">Album playback fills the queue in disc/track order and will advance automatically. Use the queue controls to pause, stop, or skip.</p>
    {}
    <section class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>Cover</th>
            <th>Album</th>
            <th>Artist</th>
            <th>Tracks</th>
            <th>Actions</th>
          </tr>
        </thead>
        <tbody id="album_table">
          {}
        </tbody>
      </table>
    </section>
    <h2 class="section-heading">Tracks</h2>
    <section class="library-grid">
      <div class="table-wrap">
        <table>
          <thead>
            <tr>
              <th>Play</th>
              <th>Cover</th>
              <th>Title</th>
              <th>Artist</th>
              <th>Album</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody id="track_table">
            {}
          </tbody>
        </table>
      </div>
      <aside id="track_detail_panel" class="track-sidebar">
        <h3>Track Tags</h3>
        <p>Select a track to inspect its embedded tags, artwork source, and file metadata here.</p>
      </aside>
    </section>
  </main>
  <script>
    function escapeHtml(value) {{
      return String(value)
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
    }}

    function formatDuration(seconds) {{
      if (seconds === null || seconds === undefined || Number.isNaN(Number(seconds))) {{
        return 'Unknown';
      }}
      const total = Number(seconds);
      const hours = Math.floor(total / 3600);
      const minutes = Math.floor((total % 3600) / 60);
      const secs = total % 60;
      if (hours > 0) {{
        return `${{hours}}:${{String(minutes).padStart(2, '0')}}:${{String(secs).padStart(2, '0')}}`;
      }}
      return `${{minutes}}:${{String(secs).padStart(2, '0')}}`;
    }}

    function renderTrackDetailPanel(track) {{
      const host = document.getElementById('track_detail_panel');
      if (!host) {{
        return;
      }}
      if (!track || track.error) {{
        host.innerHTML = `<h3>Track Tags</h3><p>${{escapeHtml(track?.error || 'Track details are unavailable.')}}</p>`;
        return;
      }}

      const artworkHtml = track.artwork
        ? `<img class="sidebar-artwork" src="${{escapeHtml(track.artwork.url)}}" alt="Artwork for ${{escapeHtml(track.album)}}">`
        : '<div class="sidebar-artwork placeholder">No Art</div>';

      const metaRows = [
        {{ label: 'Artist', value: track.artist || 'Unknown' }},
        {{ label: 'Album', value: track.album || 'Unknown' }},
        {{ label: 'Disc / Track', value: `${{track.disc_number ?? '?'}} / ${{track.track_number ?? '?'}}` }},
        {{ label: 'Duration', value: formatDuration(track.duration_seconds) }},
        {{ label: 'Format', value: track.mime_type || 'Unknown' }},
        {{ label: 'Parser', value: track.embedded_metadata?.parser || 'Unknown' }},
        {{ label: 'Path', value: `<code>${{escapeHtml(track.relative_path || track.absolute_path || '')}}</code>`, isHtml: true }},
      ]
        .map((item) => `
          <div class="track-sidebar-meta-row">
            <div class="track-sidebar-label">${{escapeHtml(item.label)}}</div>
            <div class="track-sidebar-value">${{item.isHtml ? item.value : escapeHtml(item.value)}}</div>
          </div>
        `)
        .join('');

      const tagRows = (track.embedded_metadata?.fields || []).length
        ? track.embedded_metadata.fields
            .map((field) => `
              <div class="track-sidebar-tag">
                <div class="track-sidebar-label">${{escapeHtml(field.key)}}</div>
                <div class="track-sidebar-tag-value"><code>${{escapeHtml(field.value)}}</code></div>
              </div>
            `)
            .join('')
        : '<div class="track-sidebar-tag"><div class="track-sidebar-value">No embedded tag fields were parsed for this file.</div></div>';

      const notesHtml = (track.embedded_metadata?.notes || []).length
        ? `<ul class="track-sidebar-note-list">${{track.embedded_metadata.notes.map((note) => `<li>${{escapeHtml(note)}}</li>`).join('')}}</ul>`
        : '';

      host.innerHTML = `
        <h3>${{escapeHtml(track.title || 'Track Tags')}}</h3>
        <p>${{escapeHtml(track.artist || 'Unknown artist')}} • ${{escapeHtml(track.album || 'Unknown album')}}</p>
        ${{artworkHtml}}
        <div class="track-sidebar-actions">
          <a class="button-link secondary" href="/track/${{encodeURIComponent(track.id)}}" target="_blank" rel="noreferrer">Full Inspector</a>
          <a class="button-link secondary" href="/stream/track/${{encodeURIComponent(track.id)}}" target="_blank" rel="noreferrer">Preview</a>
        </div>
        <div class="track-sidebar-meta">${{metaRows}}</div>
        <div class="track-sidebar-tags">${{tagRows}}</div>
        ${{notesHtml}}
      `;
    }}

    async function loadTrackDetails(trackId) {{
      if (!trackId) {{
        return;
      }}
      try {{
        const response = await fetch(`/api/tracks/${{encodeURIComponent(trackId)}}`);
        const payload = await response.json();
        renderTrackDetailPanel(payload);
      }} catch (_error) {{
        renderTrackDetailPanel({{ error: 'Failed to load track details.' }});
      }}
    }}

    function syncSelectedTrackSidebar() {{
      const selected = document.querySelector('input[name="track_id"]:checked');
      if (selected) {{
        loadTrackDetails(selected.value);
      }}
    }}

    async function discoverRenderers() {{
      const select = document.getElementById('renderer_discovery');
      select.innerHTML = '<option value="">Discovering renderers...</option>';
      try {{
        const response = await fetch('/api/renderers/discover');
        const items = await response.json();
        select.innerHTML = '';
        if (!items.length) {{
          select.innerHTML = '<option value="">No renderers discovered</option>';
          return;
        }}
        for (const item of items) {{
          const option = document.createElement('option');
          option.value = item.location;
          option.textContent = item.name || item.location;
          select.appendChild(option);
        }}
      }} catch (error) {{
        select.innerHTML = '<option value="">Discovery failed</option>';
      }}
    }}

    function applySelectedRenderer() {{
      const select = document.getElementById('renderer_discovery');
      const input = document.getElementById('renderer_location');
      if (select.value) {{
        input.value = select.value;
        syncRendererFields(select.value);
        refreshQueuePanel();
      }}
    }}

    function syncRendererFields(value) {{
      const hidden = document.getElementById('rescan_renderer_location');
      if (hidden) {{
        hidden.value = value;
      }}
      const proxies = document.querySelectorAll('.renderer-location-proxy');
      for (const proxy of proxies) {{
        proxy.value = value;
      }}
    }}

    let queueRefreshTimer = null;
    let queueRefreshInFlight = false;

    async function refreshQueuePanel() {{
      const rendererInput = document.getElementById('renderer_location');
      const host = document.getElementById('queue_panel_host');
      if (!rendererInput || !host) {{
        return;
      }}
      const rendererLocation = rendererInput.value.trim();
      if (queueRefreshInFlight) {{
        return;
      }}
      queueRefreshInFlight = true;
      try {{
        const url = rendererLocation
          ? `/queue/panel?renderer_location=${{encodeURIComponent(rendererLocation)}}`
          : '/queue/panel';
        const response = await fetch(url, {{
          headers: {{
            'X-Requested-With': 'musicd-live-refresh'
          }}
        }});
        if (!response.ok) {{
          return;
        }}
        host.innerHTML = await response.text();
        syncRendererFields(rendererLocation);
      }} catch (_error) {{
      }} finally {{
        queueRefreshInFlight = false;
      }}
    }}

    function startQueueRefresh() {{
      if (queueRefreshTimer !== null) {{
        clearInterval(queueRefreshTimer);
      }}
      document.addEventListener('visibilitychange', () => {{
        if (!document.hidden) {{
          refreshQueuePanel();
        }}
      }});
      queueRefreshTimer = setInterval(() => {{
        if (document.hidden) {{
          return;
        }}
        refreshQueuePanel();
      }}, 2500);
    }}

    function filterTracks() {{
      const needle = document.getElementById('track_filter').value.trim().toLowerCase();
      const rows = document.querySelectorAll('#track_table tr');
      for (const row of rows) {{
        row.style.display = !needle || row.dataset.search.includes(needle) ? '' : 'none';
      }}
      const albumRows = document.querySelectorAll('#album_table tr');
      for (const row of albumRows) {{
        row.style.display = !needle || row.dataset.search.includes(needle) ? '' : 'none';
      }}
    }}

    document.addEventListener('change', (event) => {{
      if (event.target instanceof HTMLInputElement && event.target.name === 'track_id') {{
        loadTrackDetails(event.target.value);
      }}
    }});

    refreshQueuePanel();
    startQueueRefresh();
    syncSelectedTrackSidebar();
  </script>
</body>
</html>"#,
        html_escape(&state.config.instance_name),
        html_escape(&state.config.instance_name),
        html_escape(&library_path),
        tracks.len(),
        html_escape(&state.config.resolved_base_url()),
        message_html,
        error_html,
        html_escape(&renderer_location),
        renderer_options,
        html_escape(&renderer_location),
        empty_state,
        queue_html,
        album_empty_state,
        album_rows,
        rows,
    )
}
