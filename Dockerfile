FROM rust:1.94-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY apps ./apps
COPY crates ./crates

RUN cargo build --release -p musicd

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/musicd /usr/local/bin/musicd
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

RUN chmod +x /usr/local/bin/docker-entrypoint.sh

ENV MUSICD_MODE=status
ENV MUSICD_BIND_ADDR=0.0.0.0:7878
ENV MUSICD_LIBRARY_PATH=/music

EXPOSE 7878/tcp

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
