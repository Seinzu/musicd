# Running On Unraid

## Recommended packaging model

Run `musicd` as a Docker container on Unraid.

For the current service and renderer work, use `host` networking.

Why:

- SSDP discovery uses multicast UDP and behaves most predictably from the host network namespace
- the Cambridge Audio renderer must be able to fetch the HTTP stream URL directly from the NAS
- you avoid bridge-network port and discovery edge cases while the app is still early

This matches Unraid's Docker guidance around choosing the right network mode and keeping persistent app data in `appdata`. See:

- [Unraid Docker overview](https://docs.unraid.net/unraid-os/using-unraid-to/run-docker-containers/overview/)
- [Managing and customizing containers](https://docs.unraid.net/unraid-os/using-unraid-to/run-docker-containers/managing-and-customizing-containers/)
- [Community Applications](https://docs.unraid.net/unraid-os/using-unraid-to/run-docker-containers/community-applications/)

## What to mount

Recommended mappings:

- `/mnt/user/appdata/musicd` -> `/config`
- `/mnt/user/Music` -> `/music` as read-only

Notes:

- `musicd` does not persist much yet, but `/config` gives us a clean place for future database and cache files
- `musicd` now stores its SQLite database and remembered renderer state under `/config/musicd.db`
- extracted artwork is cached under `/config/artwork`
- the music share should usually be read-only for the container

## Environment variables

The image is driven by environment variables through `docker-entrypoint.sh`.

Common values:

- `MUSICD_MODE`
- `MUSICD_BIND_ADDR`
- `MUSICD_LIBRARY_PATH`
- `MUSICD_CONFIG_PATH`
- `MUSICD_DISCOVERY_TIMEOUT_MS`
- `MUSICD_DEFAULT_RENDERER_LOCATION`
- `MUSICD_INSTANCE_NAME`
- `MUSICD_DEBUG`
- `MUSICD_RENDERER_LOCATION`
- `MUSICD_STREAM_URL`
- `MUSICD_AUDIO_FILE`
- `MUSICD_PUBLIC_BASE_URL`
- `MUSICD_TITLE`

### Useful modes

`serve`

- scans the mounted music share and starts the browser UI and stream service
- this is now the recommended default mode for Unraid

`discover`

- sends SSDP `M-SEARCH` and prints discovered renderer `LOCATION` URLs

`inspect`

- fetches a renderer description document and prints its AVTransport endpoint

`serve-file`

- serves one local file at `/stream/current`

`play-file`

- serves one local file and immediately sends `SetAVTransportURI` and `Play`

## Build the image

From this repository:

```bash
docker build -t musicd:phase1 .
```

If you want Unraid to pull from a registry instead, push the image to something like GitHub Container Registry and use that image name in Unraid.

## Registry tag strategy

The GitHub publish workflow now produces both moving and immutable tags:

- `edge`: updated on every push to `main`
- `latest`: updated only when a Git tag like `api-v1.2.3` is pushed
- `api-v1.2.3`: the preferred API release tag
- `v1.2.3`: a legacy-compatible API release tag
- `1.2.3`, `1.2`, `1`: semver-style release aliases for tagged releases
- `sha-<commit>`: an immutable tag for each published build

For Unraid updates, point the container at a moving tag:

- use `ghcr.io/<owner>/<repo>:edge` if you want every `main` build
- use `ghcr.io/<owner>/<repo>:latest` if you only want published releases

If you point Unraid at an immutable tag like `sha-abc1234` or `1.2.3`, Unraid will keep running that exact image and should not offer an update until you change the repository tag in the template.

## Example Unraid container settings

Repository:

```text
ghcr.io/<owner>/<repo>:edge
```

Network type:

```text
Host
```

Volume mappings:

```text
/mnt/user/appdata/musicd -> /config
/mnt/user/Music -> /music (read-only)
```

### Example: long-running service

Environment:

```text
MUSICD_MODE=serve
MUSICD_LIBRARY_PATH=/music
MUSICD_CONFIG_PATH=/config
MUSICD_BIND_ADDR=0.0.0.0:8787
MUSICD_PUBLIC_BASE_URL=http://192.168.1.10:8787
MUSICD_INSTANCE_NAME=Living Room musicd
MUSICD_DISCOVERY_TIMEOUT_MS=2000
MUSICD_DEFAULT_RENDERER_LOCATION=http://192.168.1.55:49152/description.xml
MUSICD_DEBUG=true
```

After the container starts, open:

```text
http://192.168.1.10:8787/
```

That page lets you browse the scanned library, discover renderers, rescan the share, inspect metadata, and play or queue music for a selected renderer.
The discovered renderers and your last selected renderer are persisted in SQLite under `/config`, extracted artwork is cached under `/config/artwork`, and queue/session state is stored in the same SQLite database.
Albums are now grouped in the UI as well, and `Play Album` fills the queue and starts the first ordered track. A background worker now polls UPnP transport state and advances to the next queued track when track-end detection is confident.

When `MUSICD_DEBUG=true`, `musicd` emits extra renderer and queue transition logs to the container output. This is useful for tracking pause/sleep edge cases, unexpected `STOPPED` or `NO_MEDIA_PRESENT` transitions, and auto-advance decisions.

### Example: discovery utility

Environment:

```text
MUSICD_MODE=discover
MUSICD_DISCOVERY_TIMEOUT_MS=2000
```

This is mainly for testing because the container exits after printing the discovery result.

### Example: serve and play a file to a CXN V2

Environment:

```text
MUSICD_MODE=play-file
MUSICD_RENDERER_LOCATION=http://192.168.1.55:49152/description.xml
MUSICD_AUDIO_FILE=/music/Test Album/01 - Example Track.flac
MUSICD_BIND_ADDR=0.0.0.0:7878
MUSICD_PUBLIC_BASE_URL=http://192.168.1.10:7878
MUSICD_TITLE=Example Track
```

Important:

- `MUSICD_PUBLIC_BASE_URL` must be the NAS LAN IP or hostname reachable by the CXN V2
- do not use `localhost` or `127.0.0.1`
- the chosen port must not already be used by another Unraid container or service

## Creating an Unraid template

For local use, the easiest path is:

1. Build or pull the image.
2. In Unraid, go to `Docker`.
3. Choose `Add Container`.
4. Set the repository, network type, paths, and environment variables.
5. Save the template.

Unraid stores saved container templates on the flash drive, and the current docs note those templates are used for reinstall/restore workflows. Community Applications submissions also require template files plus documentation and a support thread.

If you later want to publish this as a Community Applications app, prepare:

- a stable registry image
- clear install docs
- a support thread
- a CA-compatible template generated from Unraid's GUI flow

## Suggested next packaging improvement

Right now the container is suitable for a simple long-running local music service.

The next good step is to deepen that service so it:

- reads actual metadata tags
- adds queueing and playback state
- exposes a richer control API
