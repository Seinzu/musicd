use std::fmt::Write;

use crate::http::HttpRequest;
use crate::service::ServiceState;
use crate::util::{EscapeHtml, html_escape, url_encode};

use super::layout::{
    LayoutContext, PageTab, render_layout, render_now_playing_card, renderer_location_input,
};

pub(crate) fn render_welcome_page(state: &ServiceState, request: &HttpRequest) -> String {
    let ctx = LayoutContext::from_request(state, request);
    let library = state.library_snapshot();

    let now_playing_html =
        render_now_playing_card(state, &ctx.renderer_location, &library, "welcome", "/");

    let renderer_input_hidden = renderer_location_input(&ctx.renderer_location);
    let renderer_options_html = ctx.renderer_options_html.clone();
    let renderer_location_escaped = html_escape(&ctx.renderer_location);
    let renderer_card_html = format!(
        r#"<section class="card renderer-card">
  <div class="card-header">
    <h2>Renderer</h2>
    <p class="meta">Choose where playback should land. Selection is remembered across pages.</p>
  </div>
  <form class="control-row" id="renderer_form" action="/" method="get">
    <label for="renderer_location" class="visually-hidden">Renderer LOCATION URL</label>
    <input id="renderer_location" name="renderer_location" type="text" value="{renderer_location_escaped}" placeholder="http://192.168.1.55:49152/description.xml" oninput="syncRendererFields(this.value)">
    <button type="submit">Use This</button>
  </form>
  <div class="control-row">
    <button type="button" class="secondary" onclick="discoverRenderers()">Discover Renderers</button>
    <select id="renderer_discovery">{renderer_options_html}</select>
    <button type="button" class="secondary" onclick="applySelectedRenderer()">Use Selected</button>
  </div>
</section>"#,
        renderer_location_escaped = renderer_location_escaped,
        renderer_options_html = renderer_options_html,
    );

    let spotlight_html = render_spotlight(&library, &ctx.renderer_location, &renderer_input_hidden);

    let stats_card_html = format!(
        r#"<section class="card stats-card">
  <div class="card-header">
    <h2>{instance}</h2>
    <p class="meta">{base_url}</p>
  </div>
  <ul class="stats-list">
    <li><span class="stats-value">{tracks}</span><span class="stats-label">Tracks</span></li>
    <li><span class="stats-value">{albums}</span><span class="stats-label">Albums</span></li>
    <li><span class="stats-value">{artists}</span><span class="stats-label">Artists</span></li>
  </ul>
  <p class="meta">Library path: {library_path}</p>
  <form class="control-row" action="/rescan" method="get" id="rescan_form" data-progress-url="/rescan-progress">
    {renderer_input_hidden}
    <input type="hidden" name="return_to" value="/">
    <button type="submit" id="rescan_button" class="secondary">Rescan Library</button>
    <span id="rescan_status" class="visually-hidden" aria-live="polite"></span>
  </form>
  <div id="progress_bar_container">
    <progress id="rescan_progress_bar" value="0" max="100" aria-label="Library rescan progress"></progress>
  </div>
</section>"#,
        instance = html_escape(&ctx.instance_name),
        base_url = html_escape(&ctx.base_url),
        tracks = ctx.track_count,
        albums = ctx.album_count,
        artists = ctx.artist_count,
        library_path = html_escape(&ctx.library_path),
        renderer_input_hidden = renderer_input_hidden,
    );

    let body = format!(
        r#"<section class="welcome-hero">
  <p class="welcome-eyebrow">{greeting}</p>
  <h1 class="welcome-headline">What shall we listen to?</h1>
</section>
{now_playing_html}
<div class="welcome-grid">
  {renderer_card_html}
  {stats_card_html}
</div>
{spotlight_html}"#,
        greeting = html_escape(&time_of_day_greeting()),
    );

    render_layout(PageTab::Welcome, &body, &ctx)
}

fn render_spotlight(
    library: &crate::library::Library,
    renderer_location: &str,
    renderer_input_hidden: &str,
) -> String {
    let mut eligible: Vec<_> = library
        .albums
        .iter()
        .filter(|album| album.track_count > 3)
        .collect();
    if eligible.is_empty() {
        eligible = library.albums.iter().collect();
    }
    let count = eligible.len().min(5);
    if count == 0 {
        return r#"<section class="card spotlight-card">
  <div class="card-header"><h2>Library spotlight</h2></div>
  <p class="empty">Add some music to see suggestions here.</p>
</section>"#
            .to_string();
    }

    let renderer_qs = url_encode(renderer_location);
    let mut cards = String::new();
    for album in eligible.iter().take(count) {
        let album_url = format!(
            "/album/{}?renderer_location={}",
            url_encode(&album.id),
            renderer_qs
        );
        let artwork = match album.artwork_url.as_ref() {
            Some(url) => format!(
                "<img loading=\"lazy\" class=\"spotlight-art\" src=\"{}\" alt=\"Artwork for {}\">",
                EscapeHtml(url),
                EscapeHtml(&album.title)
            ),
            None => "<div class=\"spotlight-art placeholder\">No Art</div>".to_string(),
        };
        let _ = write!(
            cards,
            r#"<article class="spotlight-tile">
  <a class="spotlight-link" href="{album_url}">{artwork}</a>
  <div class="spotlight-meta">
    <a class="spotlight-title" href="{album_url}">{title}</a>
    <p class="meta">{artist}</p>
    <p class="meta small">{tracks} tracks</p>
  </div>
  <div class="spotlight-actions">
    <form class="inline-form" action="/play-album" method="get">
      <input type="hidden" name="album_id" value="{album_id}">
      {renderer_input_hidden}
      <button type="submit">Play</button>
    </form>
    <form class="inline-form" action="/queue/append-album" method="get">
      <input type="hidden" name="album_id" value="{album_id}">
      <input type="hidden" name="return_to" value="/">
      {renderer_input_hidden}
      <button type="submit" class="secondary">Queue</button>
    </form>
    <form class="inline-form" action="/queue/play-next-album" method="get">
      <input type="hidden" name="album_id" value="{album_id}">
      <input type="hidden" name="return_to" value="/">
      {renderer_input_hidden}
      <button type="submit" class="secondary">Play Next</button>
    </form>
  </div>
</article>"#,
            album_url = EscapeHtml(&album_url),
            artwork = artwork,
            title = EscapeHtml(&album.title),
            artist = EscapeHtml(&album.artist),
            tracks = album.track_count,
            album_id = EscapeHtml(&album.id),
            renderer_input_hidden = renderer_input_hidden,
        );
    }

    format!(
        r#"<section class="card spotlight-card">
  <div class="card-header">
    <h2>Library spotlight</h2>
    <p class="meta">A handful of albums from your collection.</p>
  </div>
  <div class="spotlight-grid">{cards}</div>
</section>"#
    )
}

fn time_of_day_greeting() -> &'static str {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Local-time greeting without external deps: read the system "tm_hour"
    // by shelling out is overkill; instead use UTC offset of 0 as a coarse
    // proxy. Users in non-UTC zones will still get a roughly sensible label
    // because the greeting bands are wide.
    let hour = ((secs / 3600) % 24) as u32;
    match hour {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    }
}
