use crate::http::HttpRequest;
use crate::service::ServiceState;
use crate::util::{format_track_position, html_escape, url_encode};

use super::error::render_detail_error_page;
use super::layout::{LayoutContext, PageTab, render_layout, renderer_location_input};
use super::library::render_like_button;

pub(crate) fn render_album_detail_page(state: &ServiceState, request: &HttpRequest) -> String {
    let album_id = request.path.trim_start_matches("/album/");
    let ctx = LayoutContext::from_request(state, request);
    let _message = request.query.get("message").cloned().unwrap_or_default();
    let _error = request.query.get("error").cloned().unwrap_or_default();

    let Some(album) = state.find_album(album_id) else {
        return render_detail_error_page("Album not found");
    };
    let album_like_count = state
        .album_like_counts()
        .get(&album.id)
        .copied()
        .unwrap_or(0);
    let track_like_counts = state.track_like_counts();

    let tracks = state.tracks_for_album(&album.id);
    let artwork_html = album
        .artwork_url
        .as_ref()
        .map(|artwork_url| {
            format!(
                "<img loading=\"lazy\" class=\"cover-thumb large\" src=\"{}\" alt=\"Artwork for {}\">",
                html_escape(artwork_url),
                html_escape(&album.title)
            )
        })
        .unwrap_or_else(|| {
            "<div class=\"cover-thumb large placeholder\">No Art</div>".to_string()
        });

    let track_rows = tracks
        .iter()
        .map(|track| {
            let play_url = format!(
                "/play?track_id={}&renderer_location={}",
                url_encode(&track.id),
                url_encode(&ctx.renderer_location)
            );
            let play_next_url = format!(
                "/queue/play-next-track?track_id={}&renderer_location={}&return_to=%2Falbum%2F{}",
                url_encode(&track.id),
                url_encode(&ctx.renderer_location),
                url_encode(&album.id)
            );
            let queue_url = format!(
                "/queue/append-track?track_id={}&renderer_location={}&return_to=%2Falbum%2F{}",
                url_encode(&track.id),
                url_encode(&ctx.renderer_location),
                url_encode(&album.id)
            );
            format!(
                "<tr><td data-label=\"Position\">{}</td><td data-label=\"Title\">{}</td><td data-label=\"Artist\">{}</td><td data-label=\"Likes\">{}</td><td data-label=\"Actions\" class=\"actions-cell\"><a href=\"{}\">Play Track</a> <span class=\"muted-sep\">|</span> <a href=\"{}\">Play Next</a> <span class=\"muted-sep\">|</span> <a href=\"{}\">Queue</a> <span class=\"muted-sep\">|</span> <a href=\"/track/{}?renderer_location={}\" target=\"_blank\" rel=\"noreferrer\">Inspect</a></td></tr>",
                html_escape(&format_track_position(track.disc_number, track.track_number)),
                html_escape(&track.title),
                html_escape(&track.artist),
                render_like_button(
                    "track",
                    &track.id,
                    track_like_counts.get(&track.id).copied().unwrap_or(0)
                ),
                html_escape(&play_url),
                html_escape(&play_next_url),
                html_escape(&queue_url),
                html_escape(&track.id),
                html_escape(&ctx.renderer_location),
            )
        })
        .collect::<Vec<_>>()
        .join("");

    let renderer_input = renderer_location_input(&ctx.renderer_location);

    let body = format!(
        r#"<section class="card album-detail">
  <div class="album-header">
    <h1>{}</h1>
    <p class="meta">{}</p>
    <p class="meta small">{} tracks</p>
    <div class="album-like">{}</div>
  </div>
  {}
  <div class="album-actions">
    <form action="/play-album" method="get">
      {renderer_input}
      <input type="hidden" name="album_id" value="{}">
      <label for="renderer_location" style="display:block; font-weight:600; margin-bottom:0.5rem;">Renderer LOCATION</label>
      <input id="renderer_location" name="renderer_location" type="text" value="{}" placeholder="http://192.168.1.55:49152/description.xml">
      <div class="control-row">
        <button type="submit">Play Album</button>
        <button type="submit" formaction="/queue/play-next-album">Play Next</button>
        <button type="submit" formaction="/queue/append-album">Queue Album</button>
      </div>
    </form>
    <a class="text-link" href="/stream/track/{}" target="_blank" rel="noreferrer">Preview First Track</a>
  </div>
  <section class="album-tracks">
    <h2>Tracks</h2>
    <table>
      <thead>
        <tr>
          <th>Position</th>
          <th>Title</th>
          <th>Artist</th>
          <th>Likes</th>
          <th>Actions</th>
        </tr>
      </thead>
      <tbody>{}</tbody>
    </table>
  </section>
</section>"#,
        html_escape(&album.title),
        html_escape(&album.metadata.release_date.unwrap_or("".to_string())),
        album.track_count,
        render_like_button("album", &album.id, album_like_count),
        artwork_html,
        html_escape(&album.id),
        html_escape(&ctx.renderer_location),
        html_escape(&album.first_track_id),
        track_rows,
    );

    render_layout(PageTab::Library, &body, &ctx)
}
