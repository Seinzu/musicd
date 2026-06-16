FROM rust:1.94-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY apps ./apps
COPY crates ./crates

RUN cargo build --release -p musicd

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl python3 python3-venv \
    && python3 -m venv /opt/musicd-tidal \
    && /opt/musicd-tidal/bin/pip install --no-cache-dir tidalapi \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/musicd /usr/local/bin/musicd
COPY scripts/tidal/tidalapi_helper.py /usr/local/share/musicd/tidalapi_helper.py
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

RUN chmod +x /usr/local/bin/docker-entrypoint.sh /usr/local/share/musicd/tidalapi_helper.py

ENV MUSICD_MODE=serve
ENV MUSICD_BIND_ADDR=0.0.0.0:7878
ENV MUSICD_LIBRARY_PATH=/music
ENV MUSICD_PUBLIC_BASE_URL=auto
ENV MUSICD_TIDAL_HELPER_COMMAND="/opt/musicd-tidal/bin/python /usr/local/share/musicd/tidalapi_helper.py"

EXPOSE 7878/tcp

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD sh -c 'curl -fsS "http://127.0.0.1:${MUSICD_BIND_ADDR##*:}/health" >/dev/null || exit 1'

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
