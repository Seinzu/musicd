#!/bin/sh
set -eu

mode="${MUSICD_MODE:-status}"

case "$mode" in
  status)
    exec /usr/local/bin/musicd status
    ;;
  discover)
    if [ -n "${MUSICD_TIMEOUT_MS:-}" ]; then
      exec /usr/local/bin/musicd discover "$MUSICD_TIMEOUT_MS"
    fi
    exec /usr/local/bin/musicd discover
    ;;
  inspect)
    : "${MUSICD_RENDERER_LOCATION:?MUSICD_RENDERER_LOCATION is required for inspect mode}"
    exec /usr/local/bin/musicd inspect "$MUSICD_RENDERER_LOCATION"
    ;;
  play-url)
    : "${MUSICD_RENDERER_LOCATION:?MUSICD_RENDERER_LOCATION is required for play-url mode}"
    : "${MUSICD_STREAM_URL:?MUSICD_STREAM_URL is required for play-url mode}"
    if [ -n "${MUSICD_TITLE:-}" ]; then
      exec /usr/local/bin/musicd play-url "$MUSICD_RENDERER_LOCATION" "$MUSICD_STREAM_URL" "$MUSICD_TITLE"
    fi
    exec /usr/local/bin/musicd play-url "$MUSICD_RENDERER_LOCATION" "$MUSICD_STREAM_URL"
    ;;
  serve-file)
    : "${MUSICD_AUDIO_FILE:?MUSICD_AUDIO_FILE is required for serve-file mode}"
    exec /usr/local/bin/musicd serve-file "$MUSICD_AUDIO_FILE" "${MUSICD_BIND_ADDR:-0.0.0.0:7878}"
    ;;
  play-file)
    : "${MUSICD_RENDERER_LOCATION:?MUSICD_RENDERER_LOCATION is required for play-file mode}"
    : "${MUSICD_AUDIO_FILE:?MUSICD_AUDIO_FILE is required for play-file mode}"
    : "${MUSICD_PUBLIC_BASE_URL:?MUSICD_PUBLIC_BASE_URL is required for play-file mode}"
    if [ -n "${MUSICD_TITLE:-}" ]; then
      exec /usr/local/bin/musicd play-file \
        "$MUSICD_RENDERER_LOCATION" \
        "$MUSICD_AUDIO_FILE" \
        "${MUSICD_BIND_ADDR:-0.0.0.0:7878}" \
        "$MUSICD_PUBLIC_BASE_URL" \
        "$MUSICD_TITLE"
    fi
    exec /usr/local/bin/musicd play-file \
      "$MUSICD_RENDERER_LOCATION" \
      "$MUSICD_AUDIO_FILE" \
      "${MUSICD_BIND_ADDR:-0.0.0.0:7878}" \
      "$MUSICD_PUBLIC_BASE_URL"
    ;;
  *)
    echo "Unknown MUSICD_MODE: $mode" >&2
    exit 1
    ;;
esac
