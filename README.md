# NAS Music Streaming Platform

This workspace is a starting point for a Roon-like local music application focused on:

- indexing a music library from a NAS
- serving playable stream URLs on the local network
- controlling network renderers such as a Cambridge Audio CXN V2
- growing into rich browsing, search, queue management, and multi-zone playback

## Why this shape

For a first version, the safest renderer target is `UPnP` because Cambridge Audio's CXN V2 documentation explicitly lists UPnP compatibility for local playback. That lets us build a control server that:

1. scans the NAS music library
2. serves tracks over local HTTP
3. tells the CXN V2 which URL to play using UPnP transport commands

That is much more realistic than trying to recreate all of Roon's RAAT-like behavior from day one.

## Repository layout

- `docs/architecture.md`: system design and protocol choices
- `docs/android-app-plan.md`: Android controller direction, API needs, and build phases
- `docs/android-api-contract.md`: Android-facing HTTP routes and payloads
- `docs/mvp-plan.md`: phased implementation plan
- `docs/queue-plan.md`: queue and transport progression plan
- `docs/unraid.md`: Docker packaging and Unraid deployment notes
- `apps/musicd`: starter service binary
- `crates/musicd-core`: domain models and shared config
- `crates/musicd-upnp`: UPnP transport helpers for renderer integration

## What is already scaffolded

The Rust workspace currently provides:

- domain types for tracks, albums, renderers, and app config
- a long-running library service that scans a mounted music share
- SQLite-backed persistence for the scanned library and renderer history
- first-pass local artwork extraction from embedded tags and common sidecar files
- album grouping with stable IDs plus disc/track ordering
- SSDP discovery for UPnP media renderers
- device-description parsing and AVTransport endpoint inspection
- UPnP SOAP calls for `SetAVTransportURI` and `Play`
- HTTP track streaming with basic byte-range support
- a browser UI plus JSON endpoints for browse, discovery, rescan, and playback
- album pages and a first-pass `Play Album` flow that starts from the first ordered track
- queue persistence in SQLite with renderer-specific queue state and session snapshots
- UPnP transport polling with first-pass automatic queue advancement
- CLI commands for service mode, discovery, inspection, URL playback, and file playback
- a Docker image definition and env-driven entrypoint suitable for Unraid packaging

## Suggested next steps

1. normalize artwork and library data into album/artist tables instead of track-level fallbacks
2. add queue state and transport status polling under `/config`
3. persist richer renderer capabilities and connection health
4. add MusicBrainz and Cover Art Archive enrichment on top of the local index
5. expand the controller UI beyond the single-page MVP

## Service mode

Run the long-lived service:

```bash
cargo run -p musicd -- serve
```

Then open `http://<host>:<port>/` in a browser. The page lets you:

- browse the scanned library
- browse grouped albums and open album detail pages
- filter tracks client-side
- discover UPnP renderers
- paste or reuse a renderer `LOCATION` URL
- play a selected track to that renderer
- start album playback from the first ordered track
- queue tracks and albums for a selected renderer
- continue through a queued album automatically when track-end detection is confident
- preview a track directly from the service
- inspect inferred metadata, embedded tags, and the artwork source for a track

The service also exposes:

- `GET /health`
- `GET /api/albums`
- `GET /api/queue?renderer_location=<location>`
- `GET /api/tracks`
- `GET /api/tracks/<track_id>`
- `GET /artwork/track/<track_id>`
- `GET /api/renderers/discover`
- `GET /stream/track/<track_id>`

## Utility commands

Discover renderers:

```bash
cargo run -p musicd -- discover
```

Inspect a renderer from its SSDP `LOCATION` URL:

```bash
cargo run -p musicd -- inspect http://192.168.1.55:49152/description.xml
```

Serve a local file:

```bash
cargo run -p musicd -- serve-file /path/to/test.flac 0.0.0.0:7878
```

Serve a file and tell the renderer to play it:

```bash
cargo run -p musicd -- play-file \
  http://192.168.1.55:49152/description.xml \
  /path/to/test.flac \
  0.0.0.0:7878 \
  http://192.168.1.10:7878 \
  "Test Track"
```

## Run on Unraid

See [docs/unraid.md](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/docs/unraid.md) for the recommended Docker packaging model, path mappings, environment variables, and example Unraid settings.

If you publish the image through GitHub Actions, the workflow emits both moving tags for deployment (`edge` from `main`, `latest` from release tags) and immutable tags (`sha-<commit>` plus semver release tags). For Unraid, use a moving tag in the template if you want the WebUI to notice updates.
