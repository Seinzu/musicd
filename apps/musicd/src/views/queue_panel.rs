use crate::http::HttpRequest;
use crate::library::Library;
use crate::service::{ServiceState, next_queue_entry_after, previous_queue_entry_before};
use crate::types::LibraryTrack;
use crate::util::{format_duration_seconds, html_escape};

pub(crate) fn render_queue_panel(
    state: &ServiceState,
    renderer_location: &str,
    library: &Library,
) -> String {
    if renderer_location.trim().is_empty() {
        return "<section class=\"table-wrap\"><p class=\"empty\">Enter a renderer LOCATION URL to inspect or build a queue.</p></section>".to_string();
    }

    let lookup_track = |track_id: &str| -> Option<&LibraryTrack> {
        library
            .track_index
            .get(track_id)
            .map(|&idx| &library.tracks[idx])
    };

    let queue = state.queue_snapshot(renderer_location);
    let session = state.playback_session(renderer_location);
    let current_track = queue.as_ref().and_then(|queue| {
        queue.current_entry_id.and_then(|current_entry_id| {
            queue
                .entries
                .iter()
                .find(|entry| entry.id == current_entry_id)
                .and_then(|entry| lookup_track(&entry.track_id))
        })
    });
    let progress_note = session
        .as_ref()
        .and_then(|session| {
            session.position_seconds.map(|position| {
                let duration = session
                    .duration_seconds
                    .map(format_duration_seconds)
                    .unwrap_or_else(|| "Unknown".to_string());
                format!("{} / {}", format_duration_seconds(position), duration)
            })
        })
        .unwrap_or_else(|| "Unknown progress".to_string());

    let session_meta = session
        .as_ref()
        .map(|session| {
            let error_note = session
                .last_error
                .as_ref()
                .map(|error| format!(" Error: {}.", html_escape(error)))
                .unwrap_or_default();
            let current_note = current_track
                .map(|track| {
                    format!(
                        " Current track: '{}' by {} from {}.",
                        html_escape(&track.title),
                        html_escape(&track.artist),
                        html_escape(&track.album)
                    )
                })
                .unwrap_or_default();
            format!(
                "<p class=\"section-note\">Renderer session: {}. Progress: {}. Last observed: {}.{}{} </p>",
                html_escape(&session.transport_state),
                html_escape(&progress_note),
                session.last_observed_unix,
                error_note,
                current_note
            )
        })
        .unwrap_or_else(|| {
            "<p class=\"section-note\">No playback session has been recorded for this renderer yet.</p>"
                .to_string()
        });

    let Some(queue) = queue else {
        return format!(
            "<h2 class=\"section-heading\">Queue</h2>{session_meta}<section class=\"table-wrap\"><p class=\"empty\">No queue has been saved for this renderer yet.</p></section>"
        );
    };

    let rows = queue
        .entries
        .iter()
        .map(|entry| {
            let track = lookup_track(&entry.track_id);
            let title = track
                .map(|track| track.title.clone())
                .unwrap_or_else(|| "Missing track".to_string());
            let album = track
                .map(|track| track.album.clone())
                .unwrap_or_else(|| "Unknown album".to_string());
            let marker = if Some(entry.id) == queue.current_entry_id {
                "Current"
            } else {
                ""
            };
            let duration = track
                .and_then(|track| track.duration_seconds)
                .map(format_duration_seconds)
                .unwrap_or_else(|| "Unknown".to_string());
            let actions = if Some(entry.id) == queue.current_entry_id {
                "<span class=\"meta\">Current entry</span>".to_string()
            } else {
                let mut actions = Vec::new();
                if previous_queue_entry_before(&queue, entry.id).is_some() {
                    actions.push(format!(
                        "<form class=\"inline-form\" action=\"/queue/move-up\" method=\"get\"><input type=\"hidden\" name=\"entry_id\" value=\"{}\"><input type=\"hidden\" name=\"return_to\" value=\"/\"><input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\"><button type=\"submit\" class=\"secondary\">Move Up</button></form>",
                        entry.id,
                        html_escape(renderer_location)
                    ));
                }
                if next_queue_entry_after(&queue, entry.id).is_some() {
                    actions.push(format!(
                        "<form class=\"inline-form\" action=\"/queue/move-down\" method=\"get\"><input type=\"hidden\" name=\"entry_id\" value=\"{}\"><input type=\"hidden\" name=\"return_to\" value=\"/\"><input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\"><button type=\"submit\" class=\"secondary\">Move Down</button></form>",
                        entry.id,
                        html_escape(renderer_location)
                    ));
                }
                actions.push(format!(
                    "<form class=\"inline-form\" action=\"/queue/remove-entry\" method=\"get\"><input type=\"hidden\" name=\"entry_id\" value=\"{}\"><input type=\"hidden\" name=\"return_to\" value=\"/\"><input class=\"renderer-location-proxy\" type=\"hidden\" name=\"renderer_location\" value=\"{}\"><button type=\"submit\" class=\"secondary\">Remove</button></form>",
                    entry.id,
                    html_escape(renderer_location)
                ));
                actions.join(" <span class=\"muted-sep\">|</span> ")
            };
            format!(
                "<tr><td data-label=\"Position\">{}</td><td data-label=\"Marker\">{}</td><td data-label=\"Title\">{}</td><td data-label=\"Album\">{}</td><td data-label=\"Duration\">{}</td><td data-label=\"Actions\" class=\"actions-cell\">{}</td></tr>",
                entry.position,
                html_escape(marker),
                html_escape(&title),
                html_escape(&album),
                html_escape(&duration),
                actions
            )
        })
        .collect::<Vec<_>>()
        .join("");

    format!(
        r#"<h2 class="section-heading">Queue</h2>
{session_meta}
<p class="section-note">Renderer: {}. Status: {}. Queue version: {}.</p>
<section class="table-wrap">
  <div class="control-row" style="padding: 0 0 1rem;">
    <form class="inline-form" action="/transport/previous" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit" class="secondary">Previous</button>
    </form>
    <form class="inline-form" action="/transport/play" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit">Play</button>
    </form>
    <form class="inline-form" action="/transport/pause" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit" class="secondary">Pause</button>
    </form>
    <form class="inline-form" action="/transport/stop" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit" class="secondary">Stop</button>
    </form>
    <form class="inline-form" action="/transport/next" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit" class="secondary">Next</button>
    </form>
    <form class="inline-form" action="/queue/clear" method="get">
      <input class="renderer-location-proxy" type="hidden" name="renderer_location" value="{}">
      <input type="hidden" name="return_to" value="/">
      <button type="submit" class="secondary">Clear Queue</button>
    </form>
  </div>
  <table>
    <thead>
      <tr>
        <th>Position</th>
        <th>Marker</th>
        <th>Title</th>
        <th>Album</th>
        <th>Duration</th>
        <th>Actions</th>
      </tr>
    </thead>
    <tbody>{}</tbody>
  </table>
</section>"#,
        html_escape(renderer_location),
        html_escape(&queue.status),
        queue.version,
        html_escape(renderer_location),
        html_escape(renderer_location),
        html_escape(renderer_location),
        html_escape(renderer_location),
        html_escape(renderer_location),
        html_escape(renderer_location),
        rows,
    )
}

pub(crate) fn render_queue_panel_html(state: &ServiceState, request: &HttpRequest) -> String {
    let renderer_location = state
        .preferred_renderer_location(request.query.get("renderer_location").map(String::as_str));
    let library = state.library_snapshot();
    render_queue_panel(state, &renderer_location, &library)
}

