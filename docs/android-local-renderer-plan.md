# Android Local Renderer Plan

## Goal

Let the Android app act as an additional renderer while keeping `musicd` as the source of truth for:

- library data
- queue state
- current queue position
- transport/session state
- play history

In this model, the phone is a playback target, not the queue owner.

## Why this shape

This keeps local playback aligned with the existing product model:

- the same queue can move between external renderers and the phone
- queue editing stays server-owned
- Android remains a first-class controller even when it is also the player
- features like play history, artwork fallback, and SSE updates still flow through one backend

It also fits the current backend direction better than a phone-owned queue, because `musicd` already persists queue/session state and already exposes live playback updates.

## Target model

Add a new renderer kind:

- `android_local`

The selected renderer still lives in the same app picker, but instead of pointing to a UPnP `LOCATION` URL it points to a logical Android device identifier.

Examples:

- `android-local://this-device`
- `android-local://pixel-9-pro`

The exact identifier can stay simple at first. We only need it to be stable enough for queue/session ownership on one device.

## Ownership split

### `musicd` backend owns

- queue contents and ordering
- current queue entry id
- queue mutations
- play / pause / next / previous / stop intent
- persisted playback session snapshot
- play history

### Android app owns

- actual media playback with `Media3` / `ExoPlayer`
- local buffering state
- local progress ticking between server sync points
- audio focus, notifications, and media session integration

## Core flow

### Start playback

1. User selects `This phone` as renderer.
2. User presses play on a track or album.
3. Android sends the same existing queue/play API calls to `musicd`.
4. `musicd` updates queue/session state and emits the change over SSE.
5. The Android app sees that the active renderer is `android_local`, resolves the current queue entry to the stream URL, and starts ExoPlayer locally.
6. The Android app reports playback state back to `musicd`.

### Track advance

1. ExoPlayer reaches the end of the current item.
2. Android tells `musicd` that playback completed or requests `next`.
3. `musicd` advances the queue as the source of truth.
4. SSE broadcasts the new current entry.
5. Android starts the next track locally.

This keeps queue advancement server-owned even though playback execution is local.

## Backend changes

### 1. Add renderer kind support

Extend renderer modeling so a renderer can be:

- `upnp`
- `android_local`

The current backend abstraction already points the right way, but it currently only resolves `upnp` and rejects `sonos`. `android_local` should be treated as a logical renderer class rather than a discoverable network device.

### 2. Persist Android local renderer records

Persist at least:

- `location`
- `name`
- `kind`
- `last_seen_unix`
- `last_checked_unix`
- `reachable`
- capability flags

Suggested first-pass capabilities:

- `supports_pause = true`
- `supports_stop = true`
- `supports_next = true`
- `supports_previous = true`
- `supports_seek = true`
- `supports_set_next_av_transport_uri = false`

### 3. Add a renderer registration endpoint

The Android app needs a way to announce itself as a playable local renderer.

Suggested endpoint:

- `POST /api/renderers/register-android-local`

Payload:

- `location`
- `name`
- optional device metadata like model/manufacturer/app version

This should upsert a persisted renderer record.

### 4. Add a playback-state reporting endpoint

Because the phone is doing the actual playback, `musicd` needs explicit updates instead of polling a renderer protocol.

Suggested endpoint:

- `POST /api/renderers/android-local/session`

Payload:

- `renderer_location`
- `transport_state`
- `current_track_uri`
- `queue_entry_id`
- `position_seconds`
- `duration_seconds`
- optional `last_error`

This should update the same `playback_sessions` table used by external renderers.

### 5. Add explicit local-completion reporting

Suggested endpoint:

- `POST /api/renderers/android-local/completed`

Payload:

- `renderer_location`
- `queue_entry_id`

This lets `musicd` advance the queue intentionally instead of inferring end-of-track from UPnP transport transitions.

## Android changes

### 1. Add a real local player service

Use:

- `Media3`
- `ExoPlayer`
- existing foreground notification / media session work

This service should:

- subscribe to SSE for the selected renderer
- start/stop/pause local playback when the selected renderer is `android_local`
- report session state back to `musicd`

### 2. Register the phone as a renderer

On connect, or when opening the renderer picker, the app should register a local renderer record such as:

- name: `This phone`
- kind: `android_local`

This renderer should appear in the same picker as UPnP renderers.

### 3. Switch behavior by renderer kind

When the selected renderer is:

- `upnp`: current behavior stays as-is
- `android_local`: Android performs playback locally and reports state to `musicd`

The UI should not need to fork much beyond that.

### 4. Reuse the current notification/media session

The recent notification work should become the basis of the real local-renderer playback service instead of acting only as a remote-control mirror.

## State synchronization rules

To avoid loops:

- SSE from `musicd` is the authority for queue entry changes
- ExoPlayer is the authority for fine-grained local playback state
- Android only reports local state upward for the currently selected `android_local` renderer
- `musicd` should ignore stale local updates for non-selected renderers or mismatched queue entries

## First implementation scope

### Phase 1

- add `android_local` renderer kind
- Android registers `This phone`
- selecting that renderer enables local playback
- `Play`, `Pause`, `Stop`, `Next`, `Previous` work
- playback session updates are pushed back to `musicd`
- queue remains server-owned

### Phase 2

- seek support
- smoother prebuffering of next queued item
- audio focus and headset button polish
- better reconnection/recovery after app restart

### Phase 3

- multiple Android devices as separate logical renderers
- handoff between renderers
- optional cached artwork / metadata for local resilience

## Risks

### Queue drift

If the phone advances locally without `musicd` confirming the queue change, state will diverge. The implementation should prefer explicit completion reporting and server ack over optimistic local advancement.

### App lifecycle

If the app is killed, the queue still exists on the server but playback stops locally. We should treat this as an interrupted renderer session, not as queue completion.

### Competing transport owners

The same app instance will be:

- a controller for external renderers
- a renderer for local playback

That is fine, but the code should keep those roles distinct.

## Recommendation

Build this as a first-class renderer kind, not as a special playback shortcut hidden in the Android app.

That keeps the model coherent:

- one renderer picker
- one queue owner
- one now-playing model
- one SSE channel

The phone just becomes another renderer target that happens to execute playback locally instead of over UPnP.
