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
- `docs/mvp-plan.md`: phased implementation plan
- `docs/unraid.md`: Docker packaging and Unraid deployment notes
- `apps/musicd`: starter service binary
- `crates/musicd-core`: domain models and shared config
- `crates/musicd-upnp`: UPnP transport helpers for renderer integration

## What is already scaffolded

The Rust workspace currently provides:

- domain types for tracks, albums, renderers, and app config
- SSDP discovery for UPnP media renderers
- device-description parsing and AVTransport endpoint inspection
- UPnP SOAP calls for `SetAVTransportURI` and `Play`
- a one-file HTTP stream server with basic byte-range support
- a CLI for discovery, inspection, URL playback, and file playback
- a Docker image definition and env-driven entrypoint suitable for Unraid packaging

## Suggested next steps

1. test `discover`, `inspect`, and `play-file` against a real CXN V2 on your LAN
2. add a filesystem scanner that walks a mounted NAS path
3. store library metadata in SQLite
4. replace the one-file server with a library-backed stream endpoint
5. add a controller UI for search, browse, queue, and device selection

## Phase 1 commands

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
