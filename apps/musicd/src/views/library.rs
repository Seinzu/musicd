use std::fmt::Write;

use crate::http::HttpRequest;
use crate::service::ServiceState;
use crate::util::{EscapeHtml, html_escape, url_encode};

use super::layout::{LayoutContext, PageTab, render_layout, renderer_location_input};

pub(crate) fn render_library_page(state: &ServiceState, request: &HttpRequest) -> String {
    let ctx = LayoutContext::from_request(state, request);
    let library = state.library_snapshot();
    let renderer_location_url_encoded = url_encode(&ctx.renderer_location);
    let renderer_input = renderer_location_input(&ctx.renderer_location);

    let artists_html = render_artists_section(&library, &renderer_location_url_encoded);
    let albums_html =
        render_albums_section(&library, &ctx.renderer_location, &renderer_input, &renderer_location_url_encoded);
    let tracks_html = render_tracks_section(&library, &ctx.renderer_location, &renderer_input, &renderer_location_url_encoded);

    let library_empty = if library.tracks.is_empty() {
        "<p class=\"empty\">No supported audio files were found under the library path. Add music or verify the volume mapping, then rescan.</p>"
    } else {
        ""
    };

    let body = format!(
        r#"<section class="card library-toolbar">
  <div class="card-header">
    <h1 class="library-heading">Library</h1>
    <p class="meta">{track_count} tracks · {album_count} albums · {artist_count} artists</p>
  </div>
  <div class="library-controls">
    <input id="library_filter" class="library-filter" type="text" placeholder="Search artists, albums, or tracks" oninput="filterLibrary()">
    <div class="filter-chips" role="tablist" aria-label="Library facet">
      <button type="button" class="chip active" data-facet="all" role="tab" aria-selected="true">All</button>
      <button type="button" class="chip" data-facet="artists" role="tab" aria-selected="false">Artists</button>
      <button type="button" class="chip" data-facet="albums" role="tab" aria-selected="false">Albums</button>
      <button type="button" class="chip" data-facet="tracks" role="tab" aria-selected="false">Tracks</button>
    </div>
  </div>
</section>
{library_empty}
<form id="playback_form" action="/play" method="get" class="hidden-form">
  {renderer_input}
</form>
{artists_html}
{albums_html}
<section class="library-tracks-grid">
  {tracks_html}
  <aside id="track_detail_panel" class="track-sidebar">
    <h3>Track Tags</h3>
    <p class="meta">Select a track to inspect its embedded tags, artwork source, and file metadata here.</p>
  </aside>
</section>"#,
        track_count = ctx.track_count,
        album_count = ctx.album_count,
        artist_count = ctx.artist_count,
        library_empty = library_empty,
        renderer_input = renderer_input,
    );

    render_layout(PageTab::Library, &body, &ctx)
}

fn render_artists_section(library: &crate::library::Library, renderer_qs: &str) -> String {
    if library.artists.is_empty() {
        return r#"<section class="card library-section" data-section="artists"></section>"#.to_string();
    }

    let mut rows = String::new();
    for artist in library.artists.iter() {
        let mut search = artist.name.to_lowercase();
        search.push(' ');
        let first_album_url = format!(
            "/album/{}?renderer_location={}",
            url_encode(&artist.first_album_id),
            renderer_qs,
        );
        let artwork = match artist.artwork_url.as_ref() {
            Some(url) => format!(
                "<img loading=\"lazy\" class=\"cover-thumb round\" src=\"{}\" alt=\"Artwork for {}\">",
                EscapeHtml(url),
                EscapeHtml(&artist.name)
            ),
            None => format!(
                "<div class=\"cover-thumb round placeholder\">{}</div>",
                EscapeHtml(initials(&artist.name).as_str())
            ),
        };
        let _ = write!(
            rows,
            r#"<li class="artist-row" data-search="{search}">
  <div class="artist-row-art">{artwork}</div>
  <div class="artist-row-meta">
    <a class="artist-name" href="{first_album_url}">{name}</a>
    <p class="meta">{album_count} albums · {track_count} tracks</p>
  </div>
</li>"#,
            search = EscapeHtml(&search),
            artwork = artwork,
            first_album_url = EscapeHtml(&first_album_url),
            name = EscapeHtml(&artist.name),
            album_count = artist.album_count,
            track_count = artist.track_count,
        );
    }

    format!(
        r#"<section class="card library-section" data-section="artists">
  <div class="card-header"><h2>Artists</h2><span class="meta">{count}</span></div>
  <ul class="artist-list">{rows}</ul>
</section>"#,
        count = library.artists.len(),
    )
}

fn render_albums_section(
    library: &crate::library::Library,
    renderer_location: &str,
    renderer_input: &str,
    renderer_qs: &str,
) -> String {
    if library.albums.is_empty() {
        return r#"<section class="card library-section" data-section="albums">
  <div class="card-header"><h2>Albums</h2></div>
  <p class="empty">No albums have been grouped yet. Add music or rescan the library.</p>
</section>"#
            .to_string();
    }

    let renderer_location_escaped = html_escape(renderer_location);
    let mut rows = String::new();
    let mut albums: Vec<_> = library.albums.iter().collect();
    albums.sort_by(|a, b| a.title.cmp(&b.title));
    for album in albums {
        let mut search = album.title.to_lowercase();
        search.push(' ');
        search.push_str(&album.artist.to_lowercase());

        let album_url = format!(
            "/album/{}?renderer_location={}",
            url_encode(&album.id),
            renderer_qs
        );
        let artwork = match album.artwork_url.as_ref() {
            Some(url) => format!(
                "<img loading=\"lazy\" class=\"cover-thumb\" src=\"{}\" alt=\"Artwork for {}\">",
                EscapeHtml(url),
                EscapeHtml(&album.title)
            ),
            None => "<div class=\"cover-thumb placeholder\">No Art</div>".to_string(),
        };
        let _ = write!(
            rows,
            r#"<tr data-search="{search}">
  <td data-label="Cover">{artwork}</td>
  <td data-label="Album"><a class="album-link" href="{album_url}">{title}</a></td>
  <td data-label="Artist">{artist}</td>
  <td data-label="Tracks">{tracks}</td>
  <td data-label="Actions" class="actions-cell">
    <form class="inline-form" action="/play-album" method="get">
      <input type="hidden" name="album_id" value="{album_id}">
      {renderer_input}
      <button type="submit">Play</button>
    </form>
    <form class="inline-form" action="/queue/play-next-album" method="get">
      <input type="hidden" name="album_id" value="{album_id}">
      <input type="hidden" name="return_to" value="/library">
      {renderer_input}
      <button type="submit" class="secondary">Play Next</button>
    </form>
    <form class="inline-form" action="/queue/append-album" method="get">
      <input type="hidden" name="album_id" value="{album_id}">
      <input type="hidden" name="return_to" value="/library">
      {renderer_input}
      <button type="submit" class="secondary">Queue</button>
    </form>
    <a class="text-link" href="{album_url}">View</a>
  </td>
</tr>"#,
            search = EscapeHtml(&search),
            artwork = artwork,
            album_url = EscapeHtml(&album_url),
            title = EscapeHtml(&album.title),
            artist = EscapeHtml(&album.artist),
            tracks = album.track_count,
            album_id = EscapeHtml(&album.id),
            renderer_input = renderer_input,
        );
    }
    let _ = renderer_location_escaped;

    format!(
        r#"<section class="card library-section" data-section="albums">
  <div class="card-header"><h2>Albums</h2><span class="meta">{count}</span></div>
  <div class="table-wrap">
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
      <tbody id="album_table">{rows}</tbody>
    </table>
  </div>
</section>"#,
        count = library.albums.len(),
    )
}

fn render_tracks_section(
    library: &crate::library::Library,
    _renderer_location: &str,
    renderer_input: &str,
    renderer_qs: &str,
) -> String {
    if library.tracks.is_empty() {
        return r#"<section class="card library-section" data-section="tracks">
  <div class="card-header"><h2>Tracks</h2></div>
  <p class="empty">No tracks have been indexed yet.</p>
</section>"#
            .to_string();
    }

    let mut rows = String::new();
    let mut search = String::new();
    for track in library.tracks.iter() {
        search.clear();
        search.reserve(track.title.len() + track.artist.len() + track.album.len() + 3);
        search.push_str(&track.title);
        search.push(' ');
        search.push_str(&track.artist);
        search.push(' ');
        search.push_str(&track.album);
        search.make_ascii_lowercase();

        let artwork = match track.artwork.as_ref() {
            Some(_) => format!(
                "<img loading=\"lazy\" class=\"cover-thumb\" src=\"/artwork/track/{}\" alt=\"Artwork for {}\">",
                EscapeHtml(&track.id),
                EscapeHtml(&track.album)
            ),
            None => "<div class=\"cover-thumb placeholder\">No Art</div>".to_string(),
        };
        let _ = write!(
            rows,
            r#"<tr data-search="{search}">
  <td data-label="Play"><input type="radio" form="playback_form" name="track_id" value="{track_id}"></td>
  <td data-label="Cover">{artwork}</td>
  <td data-label="Title">{title}</td>
  <td data-label="Artist">{artist}</td>
  <td data-label="Album"><a class="album-link" href="/album/{album_id_encoded}?renderer_location={renderer_qs}">{album}</a></td>
  <td data-label="Actions" class="actions-cell">
    <form class="inline-form" action="/queue/play-next-track" method="get">
      <input type="hidden" name="track_id" value="{track_id}">
      <input type="hidden" name="return_to" value="/library">
      {renderer_input}
      <button type="submit" class="secondary">Play Next</button>
    </form>
    <form class="inline-form" action="/queue/append-track" method="get">
      <input type="hidden" name="track_id" value="{track_id}">
      <input type="hidden" name="return_to" value="/library">
      {renderer_input}
      <button type="submit" class="secondary">Queue</button>
    </form>
    <a class="text-link" href="/stream/track/{track_id}" target="_blank" rel="noreferrer">Preview</a>
    <a class="text-link" href="/track/{track_id}" target="_blank" rel="noreferrer">Inspect</a>
  </td>
</tr>"#,
            search = EscapeHtml(&search),
            artwork = artwork,
            track_id = EscapeHtml(&track.id),
            title = EscapeHtml(&track.title),
            artist = EscapeHtml(&track.artist),
            album = EscapeHtml(&track.album),
            album_id_encoded = url_encode(&track.album_id),
            renderer_qs = renderer_qs,
            renderer_input = renderer_input,
        );
    }

    format!(
        r#"<section class="card library-section" data-section="tracks">
  <div class="card-header"><h2>Tracks</h2><span class="meta">{count}</span></div>
  <div class="control-row" style="padding: 0 0 0.5rem;">
    <button type="submit" form="playback_form" class="secondary">Play Selected Track</button>
  </div>
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
      <tbody id="track_table">{rows}</tbody>
    </table>
  </div>
</section>"#,
        count = library.tracks.len(),
    )
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}
