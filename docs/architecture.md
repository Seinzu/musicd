# Architecture

## Product direction

The target product is a local-first music system similar in spirit to Roon, but with a tighter first milestone:

- library stored on a NAS
- metadata indexed into a local database
- playback sent to devices on the LAN
- renderer control handled by network protocols the device already supports

For a Cambridge Audio CXN V2, the best first renderer path is `UPnP AVTransport`.

## MVP scope

### Included

- scan a mounted NAS directory
- identify albums, artists, and tracks
- parse basic tags such as title, album, artist, disc, track number, codec, sample rate
- store metadata in SQLite
- serve a stable local HTTP URL for each playable track
- discover compatible renderers on the LAN
- push playback to a renderer with UPnP transport commands
- expose a simple controller UI for browse, search, queue, and transport

### Deferred

- bit-perfect DSP pipeline
- multi-room sync
- streaming service integration
- editorial metadata at Roon quality
- custom low-latency transport comparable to RAAT

## Top-level components

### 1. Library scanner

Responsibility:

- walk the NAS-mounted filesystem
- fingerprint files by path, size, and modified timestamp
- detect adds, updates, and deletions

Implementation notes:

- start with a mounted path such as `/Volumes/Music` or a Linux bind mount
- avoid SMB protocol handling inside the app in the first version
- let the host OS mount the NAS, then scan a normal directory tree

### 2. Metadata pipeline

Responsibility:

- read tags and technical properties
- normalize album artist and multi-disc structures
- prepare artwork references

Implementation notes:

- start with a lightweight tag extraction layer
- persist only the fields needed for search and playback first
- add richer relationships after basic playback works end to end

### 3. Library database

Responsibility:

- keep normalized entities for tracks, albums, artists, artwork, and renderers
- support browse and search queries

Implementation notes:

- SQLite is the right first store
- use full-text search later if basic search becomes limiting

### 4. Stream server

Responsibility:

- serve track URLs the renderer can pull directly
- optionally transcode unsupported formats later

Implementation notes:

- first version should prefer direct file streaming
- only add transcoding when a target renderer cannot play a source format
- CXN V2 compatibility already covers common formats including FLAC, ALAC, WAV, AIFF, MP3, AAC, OGG Vorbis, and DSD x64 according to Cambridge Audio documentation

### 5. Renderer discovery and control

Responsibility:

- discover network renderers
- inspect device descriptions
- control transport and basic playback state

Implementation notes:

- use SSDP for discovery
- start with UPnP `AVTransport` and `RenderingControl`
- generate DIDL-Lite metadata when setting the transport URI
- treat AirPlay 2 and Chromecast as separate adapters after UPnP works

### 6. Controller application

Responsibility:

- browse the library
- choose a playback device
- manage queue and transport controls

Implementation notes:

- web UI is enough for an MVP
- mobile apps can come later once server APIs are stable

## End-to-end playback flow

1. The app scans the mounted NAS music folder and stores metadata in SQLite.
2. The controller UI lets the user browse tracks and pick a renderer.
3. The server creates a stable local HTTP URL for the selected track.
4. The UPnP adapter sends `SetAVTransportURI` to the renderer with the stream URL and metadata.
5. The adapter sends `Play`.
6. The renderer pulls the audio from the server directly over the LAN.

## Recommended initial stack

- backend: Rust
- library database: SQLite
- controller UI: web frontend talking to the backend over HTTP
- renderer protocol support: UPnP first

Rust is a good fit here because the core service is long-running, I/O-heavy, and likely to grow into discovery, streaming, and protocol work. If you want the fastest path to a proof of concept, you could also swap the backend to TypeScript, but I would still keep the same component boundaries.
