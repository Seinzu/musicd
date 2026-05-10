use crate::assets;
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
  <link rel="stylesheet" href="/assets/album_detail.css?v={asset_version}">
</head>
<body>
  <main>
    <header>
      <h1>{}</h1>
      <p> {} </p>
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
        html_escape(&album.metadata.release_date.unwrap_or("".to_string())),
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
        asset_version = assets::VERSION,
    )
}
