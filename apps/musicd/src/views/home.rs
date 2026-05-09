use std::fmt::Write;

use crate::assets;
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
  <link rel="stylesheet" href="/assets/home.css?v={asset_version}">
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
  <script src="/assets/home.js?v={asset_version}" defer></script>
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
        asset_version = assets::VERSION,
    )
}
