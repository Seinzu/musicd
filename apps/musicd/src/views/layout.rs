use std::fmt::Write;

use crate::assets;
use crate::http::HttpRequest;
use crate::library::Library;
use crate::service::ServiceState;
use crate::types::LibraryTrack;
use crate::util::{EscapeHtml, format_duration_seconds, html_escape};
use crate::views::json::current_track_for_renderer;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageTab {
    Welcome,
    Library,
    Queue,
}

impl PageTab {
    fn label(self) -> &'static str {
        match self {
            PageTab::Welcome => "Welcome",
            PageTab::Library => "Library",
            PageTab::Queue => "Queue",
        }
    }

    fn href(self) -> &'static str {
        match self {
            PageTab::Welcome => "/",
            PageTab::Library => "/library",
            PageTab::Queue => "/queue",
        }
    }

    fn icon(self) -> &'static str {
        // Tiny inline SVG icons keep the chrome dependency-free.
        match self {
            PageTab::Welcome => {
                r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3 3 11h2v9h5v-6h4v6h5v-9h2z"/></svg>"#
            }
            PageTab::Library => {
                r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zm0 4.5a4.5 4.5 0 1 1 0 9 4.5 4.5 0 0 1 0-9zm0 3a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z"/></svg>"#
            }
            PageTab::Queue => {
                r#"<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6h13v2H3zm0 5h13v2H3zm0 5h9v2H3zm15-6 5 4-5 4z"/></svg>"#
            }
        }
    }
}

pub(crate) struct LayoutContext {
    pub(crate) instance_name: String,
    pub(crate) library_path: String,
    pub(crate) base_url: String,
    pub(crate) track_count: usize,
    pub(crate) album_count: usize,
    pub(crate) artist_count: usize,
    pub(crate) renderer_location: String,
    pub(crate) renderer_options_html: String,
    pub(crate) selected_renderer_label: String,
    pub(crate) message: Option<String>,
    pub(crate) error: Option<String>,
}

impl LayoutContext {
    pub(crate) fn from_request(state: &ServiceState, request: &HttpRequest) -> Self {
        let library = state.library_snapshot();
        let renderer_location = state
            .preferred_renderer_location(request.query.get("renderer_location").map(String::as_str));
        let known_renderers = state.renderer_snapshot();

        let mut renderer_options_html = String::new();
        if known_renderers.is_empty() {
            renderer_options_html
                .push_str("<option value=\"\">Discovered renderers appear here</option>");
        } else {
            for renderer in known_renderers.iter() {
                let selected = if renderer.location == renderer_location {
                    " selected"
                } else {
                    ""
                };
                let _ = write!(
                    renderer_options_html,
                    "<option value=\"{}\"{}>{}</option>",
                    EscapeHtml(&renderer.location),
                    selected,
                    EscapeHtml(&renderer.name),
                );
            }
        }

        let selected_renderer_label = known_renderers
            .iter()
            .find(|renderer| renderer.location == renderer_location)
            .map(|renderer| renderer.name.clone())
            .unwrap_or_else(|| {
                if renderer_location.is_empty() {
                    "No renderer selected".to_string()
                } else {
                    renderer_location.clone()
                }
            });

        Self {
            instance_name: state.config.instance_name.clone(),
            library_path: state.config.library_path.display().to_string(),
            base_url: state.config.resolved_base_url(),
            track_count: library.tracks.len(),
            album_count: library.albums.len(),
            artist_count: library.artists.len(),
            renderer_location,
            renderer_options_html,
            selected_renderer_label,
            message: request.query.get("message").cloned().filter(|s| !s.is_empty()),
            error: request.query.get("error").cloned().filter(|s| !s.is_empty()),
        }
    }
}

pub(crate) fn render_layout(active: PageTab, body_html: &str, ctx: &LayoutContext) -> String {
    let title = format!("{} · {}", active.label(), ctx.instance_name);
    let banners = render_banners(ctx);
    let nav = render_bottom_nav(active);
    let renderer_chip_label = if ctx.renderer_location.is_empty() {
        "Choose renderer".to_string()
    } else {
        ctx.selected_renderer_label.clone()
    };

    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title_escaped}</title>
  <link rel="stylesheet" href="/assets/home.css?v={asset_version}">
</head>
<body data-renderer-location="{renderer_location_escaped}">
  <a class="skip-link" href="#main-content">Skip to content</a>
  <header class="app-bar">
    <div class="app-bar-row">
      <div class="app-bar-titles">
        <span class="app-bar-eyebrow">{instance_escaped}</span>
        <h1 class="app-bar-title">{page_label}</h1>
      </div>
      <div class="app-bar-actions">
        <span class="renderer-chip" title="{renderer_location_escaped}">
          <span class="renderer-chip-dot" aria-hidden="true"></span>
          <span class="renderer-chip-label">{renderer_chip_label_escaped}</span>
        </span>
      </div>
    </div>
  </header>
  <main id="main-content" class="page-shell">
    {banners}
    {body_html}
  </main>
  <nav class="bottom-nav" aria-label="Primary">
    {nav}
  </nav>
  <script src="/assets/home.js?v={asset_version}" defer></script>
</body>
</html>"##,
        title_escaped = html_escape(&title),
        instance_escaped = html_escape(&ctx.instance_name),
        page_label = html_escape(active.label()),
        renderer_location_escaped = html_escape(&ctx.renderer_location),
        renderer_chip_label_escaped = html_escape(&renderer_chip_label),
        asset_version = assets::VERSION,
        banners = banners,
        body_html = body_html,
        nav = nav,
    )
}

fn render_banners(ctx: &LayoutContext) -> String {
    let mut out = String::new();
    if let Some(message) = ctx.message.as_deref() {
        let _ = write!(
            out,
            "<p class=\"banner success\">{}</p>",
            EscapeHtml(message)
        );
    }
    if let Some(error) = ctx.error.as_deref() {
        let _ = write!(out, "<p class=\"banner error\">{}</p>", EscapeHtml(error));
    }
    out
}

fn render_bottom_nav(active: PageTab) -> String {
    [PageTab::Welcome, PageTab::Library, PageTab::Queue]
        .into_iter()
        .map(|tab| {
            let is_active = tab == active;
            let class = if is_active {
                "bottom-nav-item active"
            } else {
                "bottom-nav-item"
            };
            format!(
                "<a class=\"{class}\" href=\"{href}\" aria-current=\"{aria}\">\
                   <span class=\"bottom-nav-icon\">{icon}</span>\
                   <span class=\"bottom-nav-label\">{label}</span>\
                 </a>",
                class = class,
                href = tab.href(),
                aria = if is_active { "page" } else { "false" },
                icon = tab.icon(),
                label = tab.label(),
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Renders a hidden `<input>` carrying the renderer location so action forms
/// keep operating on the user's chosen renderer when JS is disabled.
pub(crate) fn renderer_location_input(renderer_location: &str) -> String {
    format!(
        "<input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\">",
        html_escape(renderer_location)
    )
}

/// Pretty-printed HTML for the now-playing card. `variant_class` is appended
/// to the root element so the welcome dashboard and the queue header can
/// share the same markup with different surrounding styles.
pub(crate) fn render_now_playing_card(
    state: &ServiceState,
    renderer_location: &str,
    library: &Library,
    variant_class: &str,
    return_to: &str,
) -> String {
    if renderer_location.trim().is_empty() {
        return format!(
            "<section class=\"now-playing {variant}\"><div class=\"now-playing-empty\">\
               <strong>No renderer selected.</strong>\
               <p>Choose a renderer below to start streaming.</p>\
             </div></section>",
            variant = html_escape(variant_class)
        );
    }

    let session = state.playback_session(renderer_location);
    let queue = state.queue_snapshot(renderer_location);
    let current_track: Option<LibraryTrack> = current_track_for_renderer(state, renderer_location);

    let (title_html, artist_html, album_html, artwork_html) = match current_track.as_ref() {
        Some(track) => {
            let title = html_escape(&track.title);
            let artist = html_escape(&track.artist);
            let album = html_escape(&track.album);
            let artwork = if track.artwork.is_some() {
                format!(
                    "<img class=\"now-playing-art\" loading=\"lazy\" src=\"/artwork/track/{}\" alt=\"Artwork for {}\">",
                    EscapeHtml(&track.id),
                    EscapeHtml(&track.album)
                )
            } else if let Some(album_art) = library
                .album_index
                .get(&track.album_id)
                .and_then(|&idx| library.albums[idx].artwork_url.clone())
            {
                format!(
                    "<img class=\"now-playing-art\" loading=\"lazy\" src=\"{}\" alt=\"Artwork for {}\">",
                    EscapeHtml(&album_art),
                    EscapeHtml(&track.album)
                )
            } else {
                "<div class=\"now-playing-art placeholder\">No Art</div>".to_string()
            };
            (title, artist, album, artwork)
        }
        None => (
            "Nothing playing".to_string(),
            String::new(),
            String::new(),
            "<div class=\"now-playing-art placeholder\">Idle</div>".to_string(),
        ),
    };

    let transport_state = session
        .as_ref()
        .map(|s| s.transport_state.clone())
        .unwrap_or_else(|| "STOPPED".to_string());

    let progress = session.as_ref().and_then(|s| {
        s.position_seconds.map(|pos| {
            let dur = s
                .duration_seconds
                .map(format_duration_seconds)
                .unwrap_or_else(|| "—".to_string());
            (pos, format_duration_seconds(pos), dur)
        })
    });
    let progress_html = match progress.as_ref() {
        Some((_, pos, dur)) => format!(
            "<div class=\"now-playing-progress\"><span>{pos}</span><span class=\"muted\">/</span><span>{dur}</span></div>",
            pos = html_escape(pos),
            dur = html_escape(dur)
        ),
        None => String::new(),
    };

    let queue_summary = queue
        .as_ref()
        .map(|q| format!("{} entries · {}", q.entries.len(), html_escape(&q.status)))
        .unwrap_or_else(|| "No queue".to_string());

    let renderer_input = renderer_location_input(renderer_location);
    let return_to_input = format!(
        "<input type=\"hidden\" name=\"return_to\" value=\"{}\">",
        html_escape(return_to)
    );

    let is_playing = transport_state == "PLAYING";
    let primary_button = if is_playing {
        format!(
            "<form class=\"inline-form\" action=\"/transport/pause\" method=\"get\">{renderer_input}{return_to_input}<button type=\"submit\" class=\"icon-button primary\" aria-label=\"Pause\">⏸</button></form>"
        )
    } else {
        format!(
            "<form class=\"inline-form\" action=\"/transport/play\" method=\"get\">{renderer_input}{return_to_input}<button type=\"submit\" class=\"icon-button primary\" aria-label=\"Play\">▶</button></form>"
        )
    };

    format!(
        r#"<section class="now-playing {variant}">
  <div class="now-playing-art-wrap">{artwork_html}</div>
  <div class="now-playing-meta">
    <p class="now-playing-status">{state_label}</p>
    <h2 class="now-playing-title">{title_html}</h2>
    <p class="now-playing-subtitle">{artist_html}{sep}{album_html}</p>
    {progress_html}
    <p class="now-playing-queue meta">{queue_summary}</p>
  </div>
  <div class="now-playing-controls">
    <form class="inline-form" action="/transport/previous" method="get">{renderer_input}{return_to_input}<button type="submit" class="icon-button" aria-label="Previous">⏮</button></form>
    {primary_button}
    <form class="inline-form" action="/transport/next" method="get">{renderer_input}{return_to_input}<button type="submit" class="icon-button" aria-label="Next">⏭</button></form>
    <form class="inline-form" action="/transport/stop" method="get">{renderer_input}{return_to_input}<button type="submit" class="icon-button" aria-label="Stop">⏹</button></form>
  </div>
</section>"#,
        variant = html_escape(variant_class),
        artwork_html = artwork_html,
        state_label = html_escape(&humanize_transport_state(&transport_state)),
        title_html = title_html,
        artist_html = artist_html,
        sep = if !artist_html.is_empty() && !album_html.is_empty() {
            " · "
        } else {
            ""
        },
        album_html = album_html,
        progress_html = progress_html,
        queue_summary = queue_summary,
        renderer_input = renderer_input,
        return_to_input = return_to_input,
        primary_button = primary_button,
    )
}

pub(crate) fn humanize_transport_state(state: &str) -> String {
    match state.to_ascii_uppercase().as_str() {
        "PLAYING" => "Playing".to_string(),
        "PAUSED_PLAYBACK" | "PAUSED" => "Paused".to_string(),
        "STOPPED" => "Stopped".to_string(),
        "TRANSITIONING" => "Transitioning".to_string(),
        "NO_MEDIA_PRESENT" => "Idle".to_string(),
        _ if state.is_empty() => "Idle".to_string(),
        other => other.to_string(),
    }
}
