use std::fmt::Write;

use crate::http::HttpRequest;
use crate::library::Library;
use crate::service::{ServiceState, next_queue_entry_after, previous_queue_entry_before};
use crate::types::LibraryTrack;
use crate::util::{EscapeHtml, format_duration_seconds, html_escape};

use super::layout::{
    LayoutContext, PageTab, render_layout, render_now_playing_card, renderer_location_input,
};

pub(crate) fn render_queue_page(state: &ServiceState, request: &HttpRequest) -> String {
    let ctx = LayoutContext::from_request(state, request);
    let library = state.library_snapshot();
    let now_playing_html =
        render_now_playing_card(state, &ctx.renderer_location, &library, "queue", "/queue");
    let panel_html = render_queue_fragment(state, &ctx.renderer_location, &library, "/queue");

    let body = format!(
        r#"<section class="card queue-card">
  <div class="card-header">
    <h1>Queue</h1>
    <p class="meta">Manage what's lined up on your renderer.</p>
  </div>
  {now_playing_html}
</section>
<section id="queue_panel_host" class="queue-host" data-renderer-location="{renderer_escaped}">
  {panel_html}
</section>"#,
        renderer_escaped = html_escape(&ctx.renderer_location),
    );

    render_layout(PageTab::Queue, &body, &ctx)
}

/// Server-side fragment used by both `/queue` and the JS poll on
/// `/queue/panel`. Returns just the queue table + transport row, no chrome.
pub(crate) fn render_queue_fragment(
    state: &ServiceState,
    renderer_location: &str,
    library: &Library,
    return_to: &str,
) -> String {
    if renderer_location.trim().is_empty() {
        return r#"<div class="card queue-empty"><p class="empty">Pick a renderer on the Welcome page to inspect or build a queue.</p></div>"#
            .to_string();
    }

    let queue = state.queue_snapshot(renderer_location);
    let session = state.playback_session(renderer_location);
    let lookup_track = |track_id: &str| -> Option<&LibraryTrack> {
        library
            .track_index
            .get(track_id)
            .map(|&idx| &library.tracks[idx])
    };

    let session_meta = match session.as_ref() {
        Some(session) => {
            let progress = session
                .position_seconds
                .map(|pos| {
                    let dur = session
                        .duration_seconds
                        .map(format_duration_seconds)
                        .unwrap_or_else(|| "—".to_string());
                    format!("{} / {}", format_duration_seconds(pos), dur)
                })
                .unwrap_or_else(|| "—".to_string());
            let error = session
                .last_error
                .as_ref()
                .map(|e| format!(" · Error: {}", html_escape(e)))
                .unwrap_or_default();
            format!(
                "<p class=\"meta\">{} · {}{}</p>",
                html_escape(&session.transport_state),
                html_escape(&progress),
                error
            )
        }
        None => "<p class=\"meta\">No playback session has been recorded yet.</p>".to_string(),
    };

    let renderer_input = renderer_location_input(renderer_location);
    let return_to_input = format!(
        "<input type=\"hidden\" name=\"return_to\" value=\"{}\">",
        html_escape(return_to)
    );

    let transport_row = format!(
        r#"<div class="transport-row">
  <form class="inline-form" action="/transport/previous" method="get">{renderer_input}{return_to_input}<button type="submit" class="secondary">Previous</button></form>
  <form class="inline-form" action="/transport/play" method="get">{renderer_input}{return_to_input}<button type="submit">Play</button></form>
  <form class="inline-form" action="/transport/pause" method="get">{renderer_input}{return_to_input}<button type="submit" class="secondary">Pause</button></form>
  <form class="inline-form" action="/transport/stop" method="get">{renderer_input}{return_to_input}<button type="submit" class="secondary">Stop</button></form>
  <form class="inline-form" action="/transport/next" method="get">{renderer_input}{return_to_input}<button type="submit" class="secondary">Next</button></form>
  <form class="inline-form" action="/queue/clear" method="get">{renderer_input}{return_to_input}<button type="submit" class="secondary danger">Clear Queue</button></form>
</div>"#,
        renderer_input = renderer_input,
        return_to_input = return_to_input,
    );

    let Some(queue) = queue else {
        return format!(
            r#"<div class="card queue-card-body">
  <div class="card-header"><h2>Queue</h2></div>
  {session_meta}
  {transport_row}
  <p class="empty">No queue has been saved for this renderer yet.</p>
</div>"#
        );
    };

    let mut rows = String::new();
    for entry in queue.entries.iter() {
        let track = lookup_track(&entry.track_id);
        let title = track
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "Missing track".to_string());
        let album = track
            .map(|t| t.album.clone())
            .unwrap_or_else(|| "Unknown album".to_string());
        let duration = track
            .and_then(|t| t.duration_seconds)
            .map(format_duration_seconds)
            .unwrap_or_else(|| "—".to_string());
        let is_current = Some(entry.id) == queue.current_entry_id;
        let marker_class = if is_current {
            "queue-entry current"
        } else {
            "queue-entry"
        };
        let actions = if is_current {
            "<span class=\"meta\">Current entry</span>".to_string()
        } else {
            let mut actions = Vec::with_capacity(3);
            if previous_queue_entry_before(&queue, entry.id).is_some() {
                actions.push(format!(
                    "<form class=\"inline-form\" action=\"/queue/move-up\" method=\"get\"><input type=\"hidden\" name=\"entry_id\" value=\"{}\">{renderer_input}{return_to_input}<button type=\"submit\" class=\"secondary\">↑</button></form>",
                    entry.id
                ));
            }
            if next_queue_entry_after(&queue, entry.id).is_some() {
                actions.push(format!(
                    "<form class=\"inline-form\" action=\"/queue/move-down\" method=\"get\"><input type=\"hidden\" name=\"entry_id\" value=\"{}\">{renderer_input}{return_to_input}<button type=\"submit\" class=\"secondary\">↓</button></form>",
                    entry.id
                ));
            }
            actions.push(format!(
                "<form class=\"inline-form\" action=\"/queue/remove-entry\" method=\"get\"><input type=\"hidden\" name=\"entry_id\" value=\"{}\">{renderer_input}{return_to_input}<button type=\"submit\" class=\"secondary danger\">Remove</button></form>",
                entry.id
            ));
            actions.join("")
        };

        let _ = write!(
            rows,
            r#"<li class="{marker_class}">
  <span class="queue-position">{position}</span>
  <div class="queue-meta">
    <p class="queue-title">{title}{current_badge}</p>
    <p class="meta">{album} · {duration}</p>
  </div>
  <div class="queue-actions">{actions}</div>
</li>"#,
            position = entry.position,
            title = EscapeHtml(&title),
            current_badge = if is_current {
                " <span class=\"chip mini\">Now</span>"
            } else {
                ""
            },
            album = EscapeHtml(&album),
            duration = EscapeHtml(&duration),
        );
    }

    if rows.is_empty() {
        rows = "<li class=\"queue-entry empty-row\"><p class=\"empty\">The queue is empty.</p></li>".to_string();
    }

    format!(
        r#"<div class="card queue-card-body">
  <div class="card-header">
    <h2>Up next</h2>
    <span class="meta">v{version} · {status}</span>
  </div>
  {session_meta}
  {transport_row}
  <ul class="queue-list">{rows}</ul>
</div>"#,
        version = queue.version,
        status = html_escape(&queue.status),
    )
}

/// Endpoint used by the JS poll. Reads `renderer_location` and `return_to`
/// from the request query, falls back to the current preferred renderer and
/// `/queue` respectively.
pub(crate) fn render_queue_panel_html(state: &ServiceState, request: &HttpRequest) -> String {
    let renderer_location = state
        .preferred_renderer_location(request.query.get("renderer_location").map(String::as_str));
    let return_to = request
        .query
        .get("return_to")
        .cloned()
        .unwrap_or_else(|| "/queue".to_string());
    let library = state.library_snapshot();
    render_queue_fragment(state, &renderer_location, &library, &return_to)
}
