# Monitoring Quickstart

This quickstart sets up a simple monitoring stack for `musicd` on Unraid using:

- `musicd`
- Prometheus
- Grafana

It assumes:

- Unraid Community Applications is already installed
- you are using the template files from this repository
- all three containers will use `host` networking

## Files in this repository

Unraid templates:

- [deploy/unraid/musicd.xml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/unraid/musicd.xml)
- [deploy/unraid/prometheus.xml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/unraid/prometheus.xml)
- [deploy/unraid/grafana.xml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/unraid/grafana.xml)

Monitoring config:

- [deploy/monitoring/prometheus/prometheus.yml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/monitoring/prometheus/prometheus.yml)
- [deploy/monitoring/grafana/provisioning/datasources/prometheus.yml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/monitoring/grafana/provisioning/datasources/prometheus.yml)
- [deploy/monitoring/grafana/provisioning/dashboards/default.yml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/monitoring/grafana/provisioning/dashboards/default.yml)
- [deploy/monitoring/grafana/dashboards/musicd-overview.json](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/deploy/monitoring/grafana/dashboards/musicd-overview.json)

## Target appdata layout

Create these directories on Unraid:

```text
/mnt/user/appdata/musicd
/mnt/user/appdata/prometheus
/mnt/user/appdata/prometheus/data
/mnt/user/appdata/grafana
/mnt/user/appdata/grafana/data
/mnt/user/appdata/grafana/provisioning/datasources
/mnt/user/appdata/grafana/provisioning/dashboards
/mnt/user/appdata/grafana/dashboards
```

## Copy the template XML files

Copy the container templates into Unraid’s user template directory:

```bash
cp deploy/unraid/musicd.xml /boot/config/plugins/dockerMan/templates-user/
cp deploy/unraid/prometheus.xml /boot/config/plugins/dockerMan/templates-user/
cp deploy/unraid/grafana.xml /boot/config/plugins/dockerMan/templates-user/
```

## Copy the monitoring config files

Copy the Prometheus and Grafana config files into appdata:

```bash
cp deploy/monitoring/prometheus/prometheus.yml /mnt/user/appdata/prometheus/prometheus.yml
cp deploy/monitoring/grafana/provisioning/datasources/prometheus.yml /mnt/user/appdata/grafana/provisioning/datasources/prometheus.yml
cp deploy/monitoring/grafana/provisioning/dashboards/default.yml /mnt/user/appdata/grafana/provisioning/dashboards/default.yml
cp deploy/monitoring/grafana/dashboards/musicd-overview.json /mnt/user/appdata/grafana/dashboards/musicd-overview.json
```

## Add the containers in Unraid

In Unraid:

1. Go to `Docker`.
2. Choose `Add Container`.
3. Select the `musicd` template.
4. Select the `prometheus-for-musicd` template.
5. Select the `grafana-for-musicd` template.

Recommended startup order:

1. `musicd`
2. `prometheus-for-musicd`
3. `grafana-for-musicd`

## Container-specific notes

### `musicd`

Important defaults:

- `MUSICD_MODE=serve`
- `MUSICD_PUBLIC_BASE_URL=auto`
- host networking

This means stream URLs and artwork URLs should adapt cleanly to the current Unraid LAN IP at container startup.

### `prometheus-for-musicd`

The template runs Prometheus with:

```text
--config.file=/etc/prometheus/prometheus.yml --storage.tsdb.path=/prometheus --web.enable-lifecycle
```

The provided config scrapes:

- Prometheus itself at `127.0.0.1:9090`
- `musicd` at `127.0.0.1:8787/metrics`

Because this uses loopback on `host` networking, it does not depend on the Unraid server’s changing LAN IP.

### `grafana-for-musicd`

Before starting Grafana, set:

- `GF_SECURITY_ADMIN_PASSWORD`

The provided provisioning files create:

- a default Prometheus datasource at `http://127.0.0.1:9090`
- a `musicd` dashboard folder
- a starter dashboard named `musicd Overview`

## Access URLs

After startup:

- `musicd`: `http://<unraid-ip>:8787/`
- Prometheus: `http://<unraid-ip>:9090/`
- Grafana: `http://<unraid-ip>:3000/`

If the Unraid LAN IP changes later, the browser URLs above will change too, but:

- Prometheus scraping continues because it uses `127.0.0.1`
- Grafana’s Prometheus datasource continues because it uses `127.0.0.1`
- `musicd` will resolve a fresh public base URL on container restart because `MUSICD_PUBLIC_BASE_URL=auto`

## What you should see

### Prometheus

Open:

```text
http://<unraid-ip>:9090/targets
```

You should see at least:

- `prometheus`
- `musicd`

Both should be `UP`.

### Grafana

Log in with:

- username: `admin`
- password: the value you set in `GF_SECURITY_ADMIN_PASSWORD`

You should see:

- a default Prometheus datasource
- a dashboard called `musicd Overview`

## Troubleshooting

### `musicd` target is down in Prometheus

Check:

- `musicd` is running
- `musicd` is using `host` networking
- `http://127.0.0.1:8787/metrics` works from the Unraid host/container context

### Grafana starts but no dashboard appears

Check that these files exist in appdata:

- `/mnt/user/appdata/grafana/provisioning/dashboards/default.yml`
- `/mnt/user/appdata/grafana/dashboards/musicd-overview.json`

### Grafana cannot reach Prometheus

Check:

- Prometheus container is running on `host`
- Grafana container is running on `host`
- `http://127.0.0.1:9090/` is reachable from the Grafana container context

## Next steps

Useful additions later:

- add a node exporter for Unraid host metrics
- add uptime monitoring against `http://<unraid-ip>:8787/health`
- add a reverse proxy or local DNS name if you want stable browser URLs even when DHCP changes
