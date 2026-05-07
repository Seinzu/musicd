# Queue Plan

## Current state

`musicd` can currently:

- scan and persist tracks, albums, artwork references, and remembered renderers
- stream individual tracks over stable HTTP URLs
- send `SetAVTransportURI` followed by `Play` to a UPnP renderer
- start an album by choosing its first ordered track

What it cannot do yet:

- persist a playback queue
- observe transport state after playback begins
- advance automatically from one track to the next
- support pause, stop, seek, next, or previous in a durable way

## Core queue idea

Model the queue as a renderer-specific ordered playlist owned by `musicd`, not by the renderer.

That means:

- `musicd` keeps the source of truth for what should play next
- the renderer stays a relatively dumb transport target
- the queue survives service restarts because it lives in SQLite
- album playback becomes a special case of "replace the queue with these ordered tracks"

This is the simplest path from the current architecture to something Roon-like.

## Why this is the right first queue model

Today the UPnP adapter only exposes:

- `SetAVTransportURI`
- `Play`

There is no transport polling, event subscription, or renderer-native queue integration yet. Because of that, the safest queue implementation is:

1. store queue state locally
2. start one queue entry on the renderer
3. poll transport state
4. advance when the current item ends

This will likely produce small gaps between tracks at first, but it is realistic and incremental.

## Required additions

### 1. Persisted queue tables

Add SQLite tables along these lines:

- `playback_queues`
  - `renderer_location`
  - `name`
  - `current_entry_id`
  - `status`
  - `version`
  - `updated_unix`
- `queue_entries`
  - `id`
  - `renderer_location`
  - `position`
  - `track_id`
  - `album_id`
  - `source_kind`
  - `source_ref`
  - `entry_status`
  - `started_unix`
  - `completed_unix`
- `playback_sessions`
  - `renderer_location`
  - `queue_entry_id`
  - `transport_state`
  - `current_track_uri`
  - `position_seconds`
  - `duration_seconds`
  - `last_observed_unix`
  - `last_error`

The queue tables should store intent. The session table should store what the renderer appears to be doing right now.

### 2. Richer track metadata

Queue advancement will be more reliable if tracks persist:

- `duration_seconds`
- maybe `sample_rate`
- maybe `bit_depth`

The scanner already reads duration through `lofty` during inspection, so this is mainly a persistence task.

### 3. Transport observation

Extend the UPnP adapter with:

- `GetTransportInfo`
- `GetPositionInfo`
- `Stop`
- `Pause`

Optional later:

- `Seek`
- `Next`
- `Previous`
- `SetNextAVTransportURI` if a renderer exposes it

The first queue implementation only really needs:

- play current item
- detect whether playback is still active
- detect that playback has ended or stalled

### 4. Queue worker

Add a background worker loop in the service that:

- wakes every 1-2 seconds
- polls active renderer sessions
- compares observed state with desired queue state
- advances to the next queue entry when needed
- records errors and retry counts

This worker should be the only place that mutates queue progression. The UI and API should enqueue or request actions, but not drive transport transitions directly.

## Queue behavior model

### Replace queue

Use this for:

- play track now
- play album now

Behavior:

- clear existing queue for the renderer
- insert the new entries
- set current entry to the first item
- start playback immediately

### Append to queue

Use this for:

- add track next
- add album next
- add to end

Behavior:

- keep the current queue
- insert entries after the current item or at the end
- do not interrupt current playback unless explicitly requested

### Resume after restart

On service startup:

- load queues from SQLite
- inspect remembered renderer sessions
- poll transport state
- if the renderer is idle but the queue has an unfinished current entry, mark the session stale and let the user resume or restart

Do not assume `musicd` can always reconstruct exact renderer state after a restart. Persisted session state should be treated as a hint, not absolute truth.

## Album playback design

Album playback should become:

1. fetch ordered tracks for the album
2. replace the renderer queue with those tracks
3. mark entry 1 as current
4. start playback

That means the current `Play Album` button can later be reimplemented on top of the queue without changing the UI concept.

## Transport advancement heuristics

Because we do not yet have renderer event subscriptions, queue advancement will initially rely on polling plus heuristics:

- if `GetTransportInfo` reports active playback, keep polling
- if `GetPositionInfo` position is moving, playback is healthy
- if transport becomes `STOPPED` after meaningful progress, mark the current entry complete and advance
- if the renderer reports no media or the URI no longer matches the current queue item, treat the session as interrupted

First version rule:

- prefer predictable gaps over fragile over-automation

If the renderer supports `SetNextAVTransportURI`, we can later reduce gaps by preloading the next track.

## Suggested API shape

Add HTTP endpoints roughly like:

- `GET /api/queue?renderer_location=...`
- `POST /queue/replace-track?renderer_location=...&track_id=...`
- `POST /queue/replace-album?renderer_location=...&album_id=...`
- `POST /queue/append-track?renderer_location=...&track_id=...`
- `POST /queue/append-album?renderer_location=...&album_id=...`
- `POST /transport/pause?renderer_location=...`
- `POST /transport/stop?renderer_location=...`
- `POST /transport/next?renderer_location=...`

The exact route names can change, but the model should separate queue mutation from transport mutation.

## UI plan

### Phase 1 UI

- show "Now Playing" panel
- show current renderer
- show current queue
- allow:
  - play now
  - play album
  - add next
  - add to queue
  - skip
  - clear queue

### Phase 2 UI

- drag to reorder queue
- remove individual queue items
- seek and pause controls
- queue history or recently played

## Implementation phases

### Phase A: Queue foundation

- add queue/session SQLite schema
- persist track durations
- add queue domain types and service methods
- add queue API endpoints without background advancement

Expected result:

- we can replace or append a queue
- starting playback still only launches the first item

### Phase B: Transport polling

- implement `GetTransportInfo` and `GetPositionInfo`
- add background queue worker
- advance to the next item when the current one finishes

Expected result:

- real continuous playback with small gaps

### Phase C: Controls and resilience

- add pause/stop/next
- persist retry/error state
- improve recovery after service restart

Expected result:

- queue behaves like a real controller rather than a fire-and-forget launcher

### Phase D: Gap reduction and polish

- inspect renderer service descriptions for optional next-track support
- try preloading next track when available
- add queue reordering and nicer now-playing feedback

Expected result:

- smoother playback and better UX

## Biggest risks

### 1. Renderer state ambiguity

Some renderers report transport state inconsistently. We should expect heuristics and edge cases.

### 2. Restart recovery

If the queue survives but the renderer state changes while `musicd` is offline, queue/session reconciliation can only be best-effort.

### 3. Gapless expectations

The first queue implementation will not be true gapless playback. It will be queue-based continuous playback with likely short transitions.

## Recommended next build step

Implement Phase A and Phase B in order:

1. queue/session SQLite schema plus queue APIs
2. transport polling and automatic advancement

That gives us a real end-to-end queue architecture quickly, and all later controls build naturally on top of it.
