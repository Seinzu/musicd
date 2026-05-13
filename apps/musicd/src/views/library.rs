use std::fmt::Write;

use crate::http::HttpRequest;
use crate::service::ServiceState;
use crate::types::{AlbumSummary, ArtistSummary, LibraryTrack};
use crate::util::{EscapeHtml, html_escape, json_escape, url_encode};

use super::layout::{LayoutContext, PageTab, render_layout, renderer_location_input};

const LIBRARY_PAGE_SIZE: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq)]
enum LibraryFacet {
    All,
    Artists,
    Albums,
    Tracks,
}

impl LibraryFacet {
    fn from_request(request: &HttpRequest) -> Self {
        match request.query.get("facet").map(String::as_str) {
            Some("artists") => Self::Artists,
            Some("albums") => Self::Albums,
            Some("tracks") => Self::Tracks,
            _ => Self::All,
        }
    }

    fn slug(self) -> Option<&'static str> {
        match self {
            Self::All => None,
            Self::Artists => Some("artists"),
            Self::Albums => Some("albums"),
            Self::Tracks => Some("tracks"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Artists => "Artists",
            Self::Albums => "Albums",
            Self::Tracks => "Tracks",
        }
    }

    fn from_rows_request(value: Option<&String>) -> Option<Self> {
        match value.map(String::as_str) {
            Some("artists") => Some(Self::Artists),
            Some("albums") => Some(Self::Albums),
            Some("tracks") => Some(Self::Tracks),
            _ => None,
        }
    }
}

pub(crate) fn render_library_page(state: &ServiceState, request: &HttpRequest) -> String {
    let ctx = LayoutContext::from_request(state, request);
    let library = state.library_snapshot();
    let renderer_location_url_encoded = url_encode(&ctx.renderer_location);
    let renderer_input = renderer_location_input(&ctx.renderer_location);
    let active_facet = LibraryFacet::from_request(request);
    let search_query = request
        .query
        .get("q")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();

    let artists_html = if matches!(active_facet, LibraryFacet::All | LibraryFacet::Artists) {
        render_artists_section(
            &library,
            &renderer_location_url_encoded,
            &ctx.renderer_location,
            &search_query,
        )
    } else {
        String::new()
    };
    let albums_html = if matches!(active_facet, LibraryFacet::All | LibraryFacet::Albums) {
        render_albums_section(
            &library,
            &ctx.renderer_location,
            &renderer_input,
            &renderer_location_url_encoded,
            &search_query,
        )
    } else {
        String::new()
    };
    let tracks_html = if matches!(active_facet, LibraryFacet::All | LibraryFacet::Tracks) {
        render_tracks_grid(
            &library,
            &ctx.renderer_location,
            &renderer_input,
            &renderer_location_url_encoded,
            &search_query,
        )
    } else {
        String::new()
    };
    let facet_chips = render_facet_chips(active_facet, &ctx.renderer_location, &search_query);

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
    <input id="library_filter" class="library-filter" type="text" value="{search_value}" placeholder="Search artists, albums, or tracks" oninput="filterLibrary()">
    <div class="filter-chips" role="tablist" aria-label="Library facet">
      {facet_chips}
    </div>
  </div>
</section>
{library_empty}
<form id="playback_form" action="/play" method="get" class="hidden-form">
  {renderer_input}
</form>
{artists_html}
{albums_html}
{tracks_html}"#,
        track_count = ctx.track_count,
        album_count = ctx.album_count,
        artist_count = ctx.artist_count,
        library_empty = library_empty,
        renderer_input = renderer_input,
        facet_chips = facet_chips,
        search_value = html_escape(&search_query),
    );

    render_layout(PageTab::Library, &body, &ctx)
}

fn render_facet_chips(active: LibraryFacet, renderer_location: &str, search_query: &str) -> String {
    let mut out = String::new();
    for facet in [
        LibraryFacet::All,
        LibraryFacet::Artists,
        LibraryFacet::Albums,
        LibraryFacet::Tracks,
    ] {
        let active_class = if facet == active { " active" } else { "" };
        let selected = if facet == active { "true" } else { "false" };
        let _ = write!(
            out,
            r#"<a class="chip{active_class}" href="{href}" data-facet="{facet_value}" role="tab" aria-selected="{selected}">{label}</a>"#,
            active_class = active_class,
            href = EscapeHtml(&facet_href(facet, renderer_location, search_query)),
            facet_value = facet.slug().unwrap_or("all"),
            selected = selected,
            label = facet.label(),
        );
    }
    out
}

fn facet_href(facet: LibraryFacet, renderer_location: &str, search_query: &str) -> String {
    let mut params = Vec::new();
    if let Some(slug) = facet.slug() {
        params.push(format!("facet={slug}"));
    }
    if !search_query.is_empty() {
        params.push(format!("q={}", url_encode(search_query)));
    }
    if !renderer_location.is_empty() {
        params.push(format!(
            "renderer_location={}",
            url_encode(renderer_location)
        ));
    }
    if params.is_empty() {
        "/library".to_string()
    } else {
        format!("/library?{}", params.join("&"))
    }
}

fn render_artists_section(
    library: &crate::library::Library,
    renderer_qs: &str,
    renderer_location: &str,
    search_query: &str,
) -> String {
    if library.artists.is_empty() {
        return r#"<section class="card library-section" data-section="artists"></section>"#
            .to_string();
    }

    let (rows, next_offset, total) =
        render_artist_rows(library, renderer_qs, search_query, 0, LIBRARY_PAGE_SIZE);
    let load_more = render_load_more(
        LibraryFacet::Artists,
        "artist_list",
        next_offset,
        total,
        renderer_location,
        search_query,
    );
    if total == 0 {
        return r#"<section class="card library-section" data-section="artists">
  <div class="card-header"><h2>Artists</h2><span class="meta">0</span></div>
  <p class="empty">No artists match the current search.</p>
</section>"#
            .to_string();
    }

    format!(
        r#"<section class="card library-section" data-section="artists">
  <div class="card-header"><h2>Artists</h2><span class="meta">{count}</span></div>
  <ul id="artist_list" class="artist-list">{rows}</ul>
  {load_more}
</section>"#,
        count = total,
    )
}

fn render_artist_rows(
    library: &crate::library::Library,
    renderer_qs: &str,
    search_query: &str,
    offset: usize,
    limit: usize,
) -> (String, usize, usize) {
    let mut rows = String::new();
    let query = search_query.to_ascii_lowercase();
    let mut total = 0;
    let mut rendered = 0;
    for artist in library.artists.iter() {
        if !query.is_empty() && !artist.name.to_lowercase().contains(&query) {
            continue;
        }
        if total >= offset && rendered < limit {
            render_artist_row(&mut rows, artist, renderer_qs);
            rendered += 1;
        }
        total += 1;
    }
    (rows, offset + rendered, total)
}

fn render_artist_row(rows: &mut String, artist: &ArtistSummary, renderer_qs: &str) {
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

fn render_albums_section(
    library: &crate::library::Library,
    renderer_location: &str,
    renderer_input: &str,
    renderer_qs: &str,
    search_query: &str,
) -> String {
    if library.albums.is_empty() {
        return r#"<section class="card library-section" data-section="albums">
  <div class="card-header"><h2>Albums</h2></div>
  <p class="empty">No albums have been grouped yet. Add music or rescan the library.</p>
</section>"#
            .to_string();
    }

    let (rows, next_offset, total) = render_album_rows(
        library,
        renderer_input,
        renderer_qs,
        search_query,
        0,
        LIBRARY_PAGE_SIZE,
    );
    let load_more = render_load_more(
        LibraryFacet::Albums,
        "album_table",
        next_offset,
        total,
        renderer_location,
        search_query,
    );
    if total == 0 {
        return r#"<section class="card library-section" data-section="albums">
  <div class="card-header"><h2>Albums</h2><span class="meta">0</span></div>
  <p class="empty">No albums match the current search.</p>
</section>"#
            .to_string();
    }

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
  {load_more}
</section>"#,
        count = total,
    )
}

fn render_album_rows(
    library: &crate::library::Library,
    renderer_input: &str,
    renderer_qs: &str,
    search_query: &str,
    offset: usize,
    limit: usize,
) -> (String, usize, usize) {
    let mut rows = String::new();
    let mut albums: Vec<_> = library.albums.iter().collect();
    albums.sort_by(|a, b| a.title.cmp(&b.title));
    let query = search_query.to_ascii_lowercase();
    let mut total = 0;
    let mut rendered = 0;
    for album in albums {
        if !query.is_empty() && !album_matches_query(album, &query) {
            continue;
        }
        if total >= offset && rendered < limit {
            render_album_row(&mut rows, album, renderer_input, renderer_qs);
            rendered += 1;
        }
        total += 1;
    }
    (rows, offset + rendered, total)
}

fn album_matches_query(album: &AlbumSummary, query: &str) -> bool {
    album.title.to_lowercase().contains(query) || album.artist.to_lowercase().contains(query)
}

fn render_album_row(
    rows: &mut String,
    album: &AlbumSummary,
    renderer_input: &str,
    renderer_qs: &str,
) {
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

fn render_tracks_section(
    library: &crate::library::Library,
    renderer_location: &str,
    renderer_input: &str,
    renderer_qs: &str,
    search_query: &str,
) -> String {
    if library.tracks.is_empty() {
        return r#"<section class="card library-section" data-section="tracks">
  <div class="card-header"><h2>Tracks</h2></div>
  <p class="empty">No tracks have been indexed yet.</p>
</section>"#
            .to_string();
    }

    let (rows, next_offset, total) = render_track_rows(
        library,
        renderer_input,
        renderer_qs,
        search_query,
        0,
        LIBRARY_PAGE_SIZE,
    );
    let load_more = render_load_more(
        LibraryFacet::Tracks,
        "track_table",
        next_offset,
        total,
        renderer_location,
        search_query,
    );
    if total == 0 {
        return r#"<section class="card library-section" data-section="tracks">
  <div class="card-header"><h2>Tracks</h2><span class="meta">0</span></div>
  <p class="empty">No tracks match the current search.</p>
</section>"#
            .to_string();
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
  {load_more}
</section>"#,
        count = total,
    )
}

fn render_track_rows(
    library: &crate::library::Library,
    renderer_input: &str,
    renderer_qs: &str,
    search_query: &str,
    offset: usize,
    limit: usize,
) -> (String, usize, usize) {
    let mut rows = String::new();
    let mut search = String::new();
    let query = search_query.to_ascii_lowercase();
    let mut total = 0;
    let mut rendered = 0;
    for track in library.tracks.iter() {
        search.clear();
        search.reserve(track.title.len() + track.artist.len() + track.album.len() + 3);
        search.push_str(&track.title);
        search.push(' ');
        search.push_str(&track.artist);
        search.push(' ');
        search.push_str(&track.album);
        search.make_ascii_lowercase();

        if !query.is_empty() && !search.contains(&query) {
            continue;
        }
        if total >= offset && rendered < limit {
            render_track_row(&mut rows, track, &search, renderer_input, renderer_qs);
            rendered += 1;
        }
        total += 1;
    }
    (rows, offset + rendered, total)
}

fn render_track_row(
    rows: &mut String,
    track: &LibraryTrack,
    search: &str,
    renderer_input: &str,
    renderer_qs: &str,
) {
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

fn render_tracks_grid(
    library: &crate::library::Library,
    renderer_location: &str,
    renderer_input: &str,
    renderer_qs: &str,
    search_query: &str,
) -> String {
    let tracks_html = render_tracks_section(
        library,
        renderer_location,
        renderer_input,
        renderer_qs,
        search_query,
    );
    format!(
        r#"<section class="library-tracks-grid">
  {tracks_html}
  <aside id="track_detail_panel" class="track-sidebar">
    <h3>Track Tags</h3>
    <p class="meta">Select a track to inspect its embedded tags, artwork source, and file metadata here.</p>
  </aside>
</section>"#,
        tracks_html = tracks_html,
    )
}

fn render_load_more(
    facet: LibraryFacet,
    target_id: &str,
    next_offset: usize,
    total: usize,
    renderer_location: &str,
    search_query: &str,
) -> String {
    if next_offset >= total {
        return String::new();
    }
    format!(
        r#"<div class="library-load-more" data-library-loader data-facet="{facet}" data-target-id="{target_id}" data-offset="{next_offset}" data-total="{total}" data-renderer-location="{renderer_location}" data-q="{search_query}">
  <span class="meta">Loading more...</span>
</div>"#,
        facet = facet.slug().unwrap_or("all"),
        target_id = EscapeHtml(target_id),
        renderer_location = EscapeHtml(renderer_location),
        search_query = EscapeHtml(search_query),
    )
}

pub(crate) fn render_library_rows_json(state: &ServiceState, request: &HttpRequest) -> String {
    let Some(facet) = LibraryFacet::from_rows_request(request.query.get("facet")) else {
        return r#"{"ok":false,"error":"invalid facet"}"#.to_string();
    };
    let ctx = LayoutContext::from_request(state, request);
    let library = state.library_snapshot();
    let renderer_qs = url_encode(&ctx.renderer_location);
    let renderer_input = renderer_location_input(&ctx.renderer_location);
    let search_query = request
        .query
        .get("q")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    let offset = request
        .query
        .get("offset")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);

    let (rows, next_offset, total) = match facet {
        LibraryFacet::Artists => render_artist_rows(
            &library,
            &renderer_qs,
            &search_query,
            offset,
            LIBRARY_PAGE_SIZE,
        ),
        LibraryFacet::Albums => render_album_rows(
            &library,
            &renderer_input,
            &renderer_qs,
            &search_query,
            offset,
            LIBRARY_PAGE_SIZE,
        ),
        LibraryFacet::Tracks => render_track_rows(
            &library,
            &renderer_input,
            &renderer_qs,
            &search_query,
            offset,
            LIBRARY_PAGE_SIZE,
        ),
        LibraryFacet::All => (String::new(), offset, 0),
    };

    format!(
        r#"{{"ok":true,"rows":"{}","next_offset":{},"total":{},"has_more":{}}}"#,
        json_escape(&rows),
        next_offset,
        total,
        if next_offset < total { "true" } else { "false" },
    )
}

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}
