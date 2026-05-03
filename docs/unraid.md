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
- `MUSICD_PUBLIC_BASE_URL`
- `MUSICD_RENDERER_LOCATION`
- `MUSICD_STREAM_URL`
- `MUSICD_AUDIO_FILE`
- `MUSICD_TITLE`

### Public base URL and IP changes

For the long-running `serve` mode, `MUSICD_PUBLIC_BASE_URL` is now optional.

If you leave it unset, or set it to:

```text
MUSICD_PUBLIC_BASE_URL=auto
```

`musicd` will derive a LAN-reachable base URL from the current bind port and host networking at startup. That means a normal Unraid restart with a changed LAN IP no longer requires you to manually rewrite the container env var just to make renderer stream URLs valid again.

Practical guidance:

- for most Unraid `host`-network installs, `auto` is now the recommended default
- if you want a fixed URL anyway, still prefer a DHCP reservation on your router
- `play-file` mode still needs an explicit `MUSICD_PUBLIC_BASE_URL`, because that CLI path takes the public base URL as an argument

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
MUSICD_PUBLIC_BASE_URL=auto
MUSICD_INSTANCE_NAME=Living Room musicd
MUSICD_DISCOVERY_TIMEOUT_MS=2000
MUSICD_DEFAULT_RENDERER_LOCATION=http://192.168.1.55:49152/description.xml
MUSICD_DEBUG=true
```

After the container starts, open the resolved URL from the logs or:

```text
http://192.168.1.10:8787/
```

That page lets you browse the scanned library, discover renderers, rescan the share, inspect metadata, and play or queue music for a selected renderer.
The discovered renderers and your last selected renderer are persisted in SQLite under `/config`, extracted artwork is cached under `/config/artwork`, and queue/session state is stored in the same SQLite database.
Albums are now grouped in the UI as well, and `Play Album` fills the queue and starts the first ordered track. A background worker now polls UPnP transport state and advances to the next queued track when track-end detection is confident.

When `MUSICD_DEBUG=true`, `musicd` emits extra renderer and queue transition logs to the container output. This is useful for tracking pause/sleep edge cases, unexpected `STOPPED` or `NO_MEDIA_PRESENT` transitions, and auto-advance decisions.

## Monitoring and metrics

The container now exposes:

- `GET /health`
- `GET /metrics`

`/health` is intended for simple uptime checks and Docker healthchecks.

`/metrics` is Prometheus-style text and currently includes:

- indexed track, album, and artist counts
- remembered renderer counts
- reachable renderer counts
- playing queue counts
- SQLite database size
- artwork cache file count and byte size

Good practical integrations:

- use the built-in Docker `HEALTHCHECK` from the image for container liveness
- point something like Uptime Kuma at `http://<unraid-host>:8787/health`
- scrape `http://<unraid-host>:8787/metrics` from Prometheus, Grafana Agent, or another simple metrics collector

This is intentionally lightweight for now: it is not deep per-request profiling, but it gives you a real first monitoring surface instead of only container logs.

### Monitoring templates

The repository now also includes matching Unraid templates for the monitoring stack:

- [deploy/unraid/prometheus.xml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/unraid/prometheus.xml)
- [deploy/unraid/grafana.xml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/unraid/grafana.xml)

These are designed to pair with:

- [deploy/monitoring/prometheus/prometheus.yml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/monitoring/prometheus/prometheus.yml)
- [deploy/monitoring/grafana/provisioning/datasources/prometheus.yml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/monitoring/grafana/provisioning/datasources/prometheus.yml)
- [deploy/monitoring/grafana/provisioning/dashboards/default.yml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/monitoring/grafana/provisioning/dashboards/default.yml)
- [deploy/monitoring/grafana/dashboards/musicd-overview.json](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/monitoring/grafana/dashboards/musicd-overview.json)

Suggested Unraid flow:

1. Copy the template XML files into `/boot/config/plugins/dockerMan/templates-user/`.
2. Copy the Prometheus and Grafana config/provisioning files into the matching `appdata` paths referenced by those templates.
3. Add the `prometheus-for-musicd` container in Unraid.
4. Add the `grafana-for-musicd` container in Unraid.
5. Start `musicd`, then Prometheus, then Grafana.

With the default files here:

- Prometheus scrapes `musicd` at `127.0.0.1:8787/metrics`
- Grafana talks to Prometheus at `127.0.0.1:9090`
- the starter `musicd Overview` dashboard appears automatically

If you want the shortest end-to-end setup path, follow [docs/monitoring-quickstart.md](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/docs/monitoring-quickstart.md).

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

This repository now includes a ready-to-edit template at [deploy/unraid/musicd.xml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/unraid/musicd.xml).

It is set up for the current recommended model:

- `ghcr.io/seinzu/musicd:edge`
- `host` networking
- `/config` on appdata
- `/music` read-only
- `MUSICD_PUBLIC_BASE_URL=auto`

To use it locally on an Unraid box:

1. Copy the XML into `/boot/config/plugins/dockerMan/templates-user/`.
2. In Unraid, go to `Docker`.
3. Choose `Add Container`.
4. Load the `musicd` template from the template dropdown.
5. Adjust the host music path, instance name, and any optional advanced fields.

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
