# NAS Music Streaming Platform

This workspace is a local-network music server and controller focused on:

- indexing a music library from a NAS
- serving playable stream URLs on the local network
- controlling network renderers such as Cambridge Audio and Sonos UPnP devices
- providing a native Android controller, including optional playback on the phone itself
- growing into richer browsing, search, queue management, enrichment, and multi-zone playback

## Why this shape

For a first version, the safest renderer target is `UPnP` because Cambridge Audio's CXN V2 documentation explicitly lists UPnP compatibility for local playback. That lets us build a control server that:

1. scans the NAS music library
2. serves tracks over local HTTP
3. tells the CXN V2 which URL to play using UPnP transport commands

That is much more realistic than trying to recreate all of Roon's RAAT-like behavior from day one.

## Repository layout

- `docs/architecture.md`: system design and protocol choices
- `docs/android-app-plan.md`: Android controller direction, API needs, and build phases
- `docs/android-local-renderer-plan.md`: plan for using the Android device itself as a server-owned queue renderer
- `docs/android-api-contract.md`: Android-facing HTTP routes and payloads
- `docs/mvp-plan.md`: phased implementation plan
- `docs/queue-plan.md`: queue and transport progression plan
- `docs/unraid.md`: Docker packaging and Unraid deployment notes
- `docs/monitoring-quickstart.md`: Prometheus/Grafana setup for Unraid
- `docs/versioning.md`: split app/api versioning and conventional commit rules
- `apps/musicd`: Rust service binary and browser UI
- `apps/musicd-cli`: small CLI companion tools
- `apps/musicd-android`: native Android controller app
- `crates/musicd-core`: domain models and shared config
- `crates/musicd-upnp`: UPnP transport helpers for renderer integration
- `deploy/unraid`: Unraid container templates for `musicd`, Prometheus, and Grafana
- `deploy/monitoring`: Prometheus and Grafana starter configuration

## What is already implemented

The current codebase already provides:

- normalized SQLite persistence for tracks, albums, artists, renderers, queue state, playback sessions, and play history
- library scanning from a mounted music share with embedded-tag parsing, disc/track ordering, and artwork extraction
- album-level artwork persistence plus manual MusicBrainz/Cover Art Archive artwork selection
- SSDP discovery and UPnP inspection for plain and nested `MediaRenderer` devices, including Sonos-style descriptions
- persisted renderer capabilities and health, including optional AVTransport action support and last-known reachability
- UPnP playback control for `SetAVTransportURI`, `SetNextAVTransportURI`, `Play`, `Pause`, `Stop`, `Next`, `Previous`, and `Seek`
- HTTP audio streaming with byte-range support and renderer metadata that can include album art
- a browser UI for browsing, queueing, renderer selection, playback control, metadata inspection, and rescans
- a native Android app with `Home`, `Library`, and `Queue`, search, artist and album browsing, queue editing, renderer picking, and live now-playing updates
- Android notification and media-session integration, plus `android_local` playback so the phone can act as a renderer
- SSE-backed live queue and now-playing updates for the web and Android clients
- Docker packaging, Unraid templates, container healthchecks, and Prometheus/Grafana starter monitoring

## Suggested next steps

1. expand MusicBrainz and Cover Art Archive enrichment beyond manual artwork selection into stored release links and broader metadata matching
2. keep polishing the Android controller with local cache, adaptive layouts, and signed release packaging
3. deepen observability and deploy ergonomics with richer metrics, CA-readiness, and release artifacts for templates/config bundles

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
- monitor current renderer/session state live without manual refreshes

The service also exposes:

- `GET /health`
- `GET /metrics`
- `GET /api/server`
- `GET /api/renderers`
- `GET /api/now-playing?renderer_location=<location>`
- `GET /api/artists`
- `GET /api/albums`
- `GET /api/queue?renderer_location=<location>`
- `GET /api/events?renderer_location=<location>`
- `GET /api/tracks`
- `GET /api/tracks/<track_id>`
- `GET /artwork/track/<track_id>`
- `GET /artwork/album/<album_id>`
- `GET /api/renderers/discover`
- `GET /stream/track/<track_id>`

There are also mutation endpoints for queue editing, transport actions, Android local renderer registration/session reporting, and manual album-art selection. The current app-facing contract is documented in [docs/android-api-contract.md](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/docs/android-api-contract.md).

## Android app

The Android app now goes well beyond a scaffold. It currently includes:

- native server onboarding and renderer selection
- `Home`, `Library`, and `Queue`
- artist and album browsing with search facets
- queue editing and transport controls
- SSE-backed live updates
- rich playback notifications and media controls
- optional `android_local` playback so the phone itself can be selected as a renderer

Build a debug APK locally:

```bash
cd apps/musicd-android
./gradlew :app:assembleDebug
```

CI also builds a debug APK through [.github/workflows/android-debug-apk.yml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/.github/workflows/android-debug-apk.yml).

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
There is also a starter Unraid template in [deploy/unraid/musicd.xml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/unraid/musicd.xml).
Matching monitoring templates now live in [deploy/unraid/prometheus.xml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/unraid/prometheus.xml) and [deploy/unraid/grafana.xml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/unraid/grafana.xml).

For `serve` mode on Unraid, `MUSICD_PUBLIC_BASE_URL` can now be left unset or set to `auto`, and `musicd` will derive a LAN-reachable base URL from the current host-network address and bind port at startup. That makes ordinary Unraid restarts and DHCP IP changes much less manual.

If you publish the image through GitHub Actions, the workflow emits both moving tags for deployment (`edge` from `main`, `latest` from release tags) and immutable tags (`sha-<commit>` plus semver release tags). For Unraid, use a moving tag in the template if you want the WebUI to notice updates.

## Versioning

The repository uses split versioning:

- `api`: the Rust backend and Docker image release line
- `app`: the Android controller release line

The current source versions in-tree are:

- `api`: `2.3.0`
- `app`: `1.1.1`

Scoped conventional commits like `feat(api): ...` and `fix(app): ...` feed the version-planning workflow. See [docs/versioning.md](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/docs/versioning.md) for the tag rules, bump logic, and helper commands.
