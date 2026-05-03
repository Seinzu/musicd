# Android API Contract

This document describes the app-facing HTTP surface that `musicd` exposes for the planned Android controller.

It is intentionally narrower and more action-oriented than the browser UI routes. The browser can keep using redirect-based endpoints; the Android app should use the routes below.

## Base assumptions

- Base URL example: `http://192.168.1.10:8787`
- All responses are JSON
- Read endpoints use `GET`
- Mutation endpoints use `POST`
- For now, `POST` bodies should be sent as `application/x-www-form-urlencoded`

Example form body:

```text
renderer_location=http%3A%2F%2F192.168.1.55%3A49152%2Fdescription.xml&track_id=abc123
```

## Response conventions

Successful mutation responses use this shape:

```json
{
  "ok": true,
  "message": "Human readable summary",
  "renderer_location": "http://192.168.1.55:49152/description.xml",
  "queue": {},
  "session": {}
}
```

Error responses use:

```json
{
  "ok": false,
  "error": "Problem description"
}
```

## Library endpoints

### `GET /api/tracks`

Returns the full scanned track list.

Each item includes:

- `id`
- `album_id`
- `title`
- `artist`
- `album`
- `disc_number`
- `track_number`
- `duration_seconds`
- `path`
- `mime_type`
- `size`
- `artwork_url`

### `GET /api/tracks/<track_id>`

Returns a single track plus embedded metadata details.

Includes:

- all basic track fields
- `relative_path`
- `absolute_path`
- `artwork`
- `embedded_metadata.parser`
- `embedded_metadata.fields`
- `embedded_metadata.notes`

### `GET /api/albums`

Returns album summaries.

Each item includes:

- `id`
- `title`
- `artist`
- `track_count`
- `first_track_id`
- `artwork_url`

### `GET /api/albums/<album_id>`

Returns album detail plus ordered track list.

Includes:

- album summary fields
- `tracks[]`

Each track in `tracks[]` includes:

- `id`
- `title`
- `artist`
- `album`
- `disc_number`
- `track_number`
- `duration_seconds`
- `artwork_url`

### `GET /api/albums/<album_id>/artwork/candidates`

Searches MusicBrainz/Cover Art Archive for likely artwork matches for an album.

Returns:

- `album`
- `candidates[]`

Each candidate includes:

- `release_id`
- `release_group_id`
- `title`
- `artist`
- `date`
- `country`
- `score`
- `thumbnail_url`
- `image_url`
- `source`

### `GET /api/artists`

Returns artist summaries.

Each item includes:

- `id`
- `name`
- `album_count`
- `track_count`
- `artwork_url`
- `first_album_id`

### `GET /api/artists/<artist_id>`

Returns artist detail plus album summaries for that artist.

Includes:

- `id`
- `name`
- `album_count`
- `track_count`
- `artwork_url`
- `first_album_id`
- `albums[]`

## Renderer endpoints

### `GET /api/server`

Returns basic server identity metadata for clients.

Includes:

- `name`
- `base_url`
- `bind_address`

### `GET /api/renderers`

Returns persisted renderer history known to `musicd`.

Each item includes:

- `location`
- `name`
- `manufacturer`
- `model_name`
- `av_transport_control_url`
- `capabilities.av_transport_actions`
- `capabilities.supports_set_next_av_transport_uri`
- `capabilities.supports_pause`
- `capabilities.supports_stop`
- `capabilities.supports_next`
- `capabilities.supports_previous`
- `capabilities.supports_seek`
- `capabilities.has_playlist_extension_service`
- `health.last_checked_unix`
- `health.last_reachable_unix`
- `health.last_error`
- `health.reachable`
- `last_seen_unix`
- `selected`
- `kind`

### `GET /api/renderers/discover`

Runs discovery and returns discovered renderers.

### `POST /api/renderers/discover`

Same behavior as the `GET` route, but easier for app-side “refresh” actions.

### `POST /api/renderers/register-android-local`

Registers the Android device as a logical local renderer.

Fields:

- `renderer_location`
- `name`
- `manufacturer`
- `model_name`

Behavior:

- upserts an `android_local` renderer record
- marks it reachable
- makes it available in the standard renderer list

### `POST /api/renderers/android-local/session`

Reports Android-local playback session state back to `musicd`.

Fields:

- `renderer_location`
- `transport_state`
- `current_track_uri`
- `position_seconds`
- `duration_seconds`

Behavior:

- updates the persisted playback session for the local renderer
- updates queue status based on the reported transport state

### `POST /api/renderers/android-local/completed`

Reports that the local renderer finished the current queue entry.

Fields:

- `renderer_location`

Behavior:

- advances the server-owned queue
- starts the next queue entry if one exists

### `GET /api/events?renderer_location=<location>`

Returns a server-sent events stream for queue and now-playing updates for a renderer.

The stream emits `playback` events with JSON payloads shaped like:

- `renderer_location`
- `now_playing`
- `queue`

This is intended for long-lived controller subscriptions so clients can react to transport and queue changes without polling.

## Queue and session endpoints

### `GET /api/queue?renderer_location=<location>`

Returns the queue for the selected renderer.

Includes:

- `renderer_location`
- `name`
- `status`
- `version`
- `updated_unix`
- `current_entry_id`
- `entries[]`
- `session`

### `GET /api/session?renderer_location=<location>`

Returns a thin session wrapper:

- `renderer_location`
- `session`

Session includes:

- `transport_state`
- `queue_entry_id`
- `next_queue_entry_id`
- `current_track_uri`
- `position_seconds`
- `duration_seconds`
- `last_observed_unix`
- `last_error`
- `title`
- `artist`
- `album`

### `GET /api/now-playing?renderer_location=<location>`

Returns a lightweight home-screen payload for the selected renderer.

Includes:

- `renderer_location`
- `renderer`
- `current_track`
- `session`
- `queue_summary`

`queue_summary` includes:

- `status`
- `name`
- `entry_count`
- `current_entry_id`
- `updated_unix`
- `version`

## Playback endpoints

### `POST /api/play`

Fields:

- `renderer_location`
- `track_id`

Behavior:

- replaces the queue with the selected track
- starts playback

### `POST /api/albums/artwork/select`

Fields:

- `album_id`
- `release_id`

Behavior:

- fetches the selected release’s front artwork from the Cover Art Archive
- caches it under `/config/artwork`
- persists an album-level artwork override for future scans

### `POST /api/play-album`

Fields:

- `renderer_location`
- `album_id`

Behavior:

- replaces the queue with the album in playback order
- starts playback from the first track

## Queue mutation endpoints

### `POST /api/queue/append-track`

Fields:

- `renderer_location`
- `track_id`

### `POST /api/queue/play-next-track`

Fields:

- `renderer_location`
- `track_id`

### `POST /api/queue/append-album`

Fields:

- `renderer_location`
- `album_id`

### `POST /api/queue/play-next-album`

Fields:

- `renderer_location`
- `album_id`

### `POST /api/queue/move`

Fields:

- `renderer_location`
- `entry_id`
- `direction`

`direction` must be:

- `up`
- `down`

### `POST /api/queue/remove`

Fields:

- `renderer_location`
- `entry_id`

### `POST /api/queue/clear`

Fields:

- `renderer_location`

## Transport endpoints

### `POST /api/transport/play`

Fields:

- `renderer_location`

### `POST /api/transport/pause`

Fields:

- `renderer_location`

### `POST /api/transport/stop`

Fields:

- `renderer_location`

### `POST /api/transport/next`

Fields:

- `renderer_location`

### `POST /api/transport/previous`

Fields:

- `renderer_location`

## Artwork and stream endpoints

### `GET /artwork/track/<track_id>`

Returns track artwork bytes when available.

### `GET /artwork/album/<album_id>`

Returns album-level override artwork bytes when a manual MusicBrainz-backed selection has been saved.

### `GET /stream/track/<track_id>`

Returns the streamable audio resource for a track.

## Current gaps

This contract is good enough to start Android implementation, but a few follow-on improvements are still desirable:

- request/response DTO cleanup for stronger consistency
- JSON request bodies instead of form-encoded mutation bodies
- a proper `GET /api/now-playing` alias if we want a more focused home-screen endpoint
- server-sent events for live queue/session updates
- authentication if remote access becomes a goal
