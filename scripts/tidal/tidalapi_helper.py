#!/usr/bin/env python3
"""Small JSON helper for musicd's TIDAL integration.

This intentionally wraps tidalapi instead of copying behavior from another
project. The Rust service treats this file as a replaceable helper process.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

STREAM_SCHEMA_VERSION = 2


def import_tidalapi():
    try:
        import tidalapi
        from tidalapi import media
    except Exception as exc:  # pragma: no cover - depends on host env
        raise RuntimeError("tidalapi is not installed; run `pip install tidalapi`") from exc
    return tidalapi, media


def session_path(args: argparse.Namespace) -> Path:
    path = Path(args.session_file).expanduser()
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def pending_pkce_path(path: Path) -> Path:
    return path.with_suffix(path.suffix + ".pkce")


def config_for_quality(quality: str):
    tidalapi, _ = import_tidalapi()
    return tidalapi.Config(quality=quality)


def session_for_quality(quality: str):
    tidalapi, _ = import_tidalapi()
    return tidalapi.Session(config_for_quality(quality))


def load_session(args: argparse.Namespace):
    path = session_path(args)
    session = session_for_quality(args.quality)
    if not path.exists():
        raise RuntimeError(f"TIDAL session file does not exist: {path}")
    session.load_session_from_file(path)
    if not session.check_login():
        raise RuntimeError("TIDAL session is not valid; run the helper login command again")
    return session


def cmd_login(args: argparse.Namespace) -> None:
    session = session_for_quality(args.quality)
    ok = session.login_session_file(session_path(args), do_pkce=True)
    if not ok:
        raise RuntimeError("TIDAL login failed")
    emit({"ok": True, "session_file": str(session_path(args))})


def cmd_auth_url(args: argparse.Namespace) -> None:
    config = config_for_quality(args.quality)
    session = session_for_quality(args.quality)
    path = session_path(args)
    pending = {
        "client_unique_key": config.client_unique_key,
        "code_verifier": config.code_verifier,
        "code_challenge": config.code_challenge,
        "quality": args.quality,
    }
    pending_pkce_path(path).write_text(json.dumps(pending), encoding="utf-8")
    session.config.client_unique_key = config.client_unique_key
    session.config.code_verifier = config.code_verifier
    session.config.code_challenge = config.code_challenge
    emit({"auth_url": session.pkce_login_url(), "session_file": str(path)})


def cmd_complete_auth(args: argparse.Namespace) -> None:
    path = session_path(args)
    pending_path = pending_pkce_path(path)
    if not pending_path.exists():
        raise RuntimeError(f"pending PKCE state does not exist: {pending_path}")
    pending = json.loads(pending_path.read_text(encoding="utf-8"))
    session = session_for_quality(pending.get("quality") or args.quality)
    session.config.client_unique_key = pending["client_unique_key"]
    session.config.code_verifier = pending["code_verifier"]
    session.config.code_challenge = pending["code_challenge"]
    token = session.pkce_get_auth_token(args.redirect_url)
    session.process_auth_token(token, is_pkce_token=True)
    session.save_session_to_file(path)
    pending_path.unlink(missing_ok=True)
    emit({"ok": True, "session_file": str(path)})


def cmd_search_tracks(args: argparse.Namespace) -> None:
    _, media = import_tidalapi()
    session = load_session(args)
    results = session.search(args.query, models=[media.Track], limit=args.limit)
    emit([track_json(track) for track in results.get("tracks", [])])


def cmd_search_albums(args: argparse.Namespace) -> None:
    _, media = import_tidalapi()
    session = load_session(args)
    results = session.search(args.query, models=[media.Album], limit=args.limit)
    emit([album_json(album) for album in results.get("albums", [])])


def cmd_album_tracks(args: argparse.Namespace) -> None:
    session = load_session(args)
    album = session.album(args.album_id)
    tracks = album_tracks(album)
    album_item = album_json(album)
    album_item["track_count"] = album_item.get("track_count") or len(tracks)
    emit({"album": album_item, "tracks": [track_json(track) for track in tracks]})


def cmd_resolve_track(args: argparse.Namespace) -> None:
    session = load_session(args)
    track = session.track(args.track_id)
    stream = track.get_stream()
    manifest = stream.get_stream_manifest()
    urls = playable_urls(manifest)
    if not urls:
        raise RuntimeError(f"TIDAL returned no stream URLs for track {args.track_id}")
    item = track_json(track)
    item.update(
        {
            "stream_url": urls[0],
            "stream_urls": urls,
            "mime_type": normalized_mime_type(manifest.mime_type),
            "audio_quality": enum_value(stream.audio_quality),
            "manifest_mime_type": enum_value(getattr(manifest, "manifest_mime_type", "")),
            "stream_format": stream_format(manifest),
            "helper_schema_version": STREAM_SCHEMA_VERSION,
        }
    )
    emit(item)


def track_json(track: Any) -> dict[str, Any]:
    album = getattr(track, "album", None)
    artist = getattr(track, "artist", None)
    artwork_url = None
    if album is not None:
        try:
            artwork_url = album.image(640, 640)
        except Exception:
            artwork_url = None
    return {
        "track_id": str(track.id),
        "title": track.title,
        "artist": getattr(artist, "name", None),
        "album": getattr(album, "name", None) or getattr(album, "title", None),
        "duration_seconds": normalized_duration_seconds(getattr(track, "duration", None)),
        "artwork_url": artwork_url,
    }


def album_json(album: Any) -> dict[str, Any]:
    artist = getattr(album, "artist", None)
    artwork_url = None
    try:
        artwork_url = album.image(640, 640)
    except Exception:
        artwork_url = None
    release_date = getattr(album, "release_date", None)
    return {
        "album_id": str(album.id),
        "title": getattr(album, "name", None) or getattr(album, "title", None) or "",
        "artist": getattr(artist, "name", None),
        "track_count": (
            getattr(album, "num_tracks", None)
            or getattr(album, "number_of_tracks", None)
            or getattr(album, "track_count", None)
        ),
        "duration_seconds": normalized_duration_seconds(getattr(album, "duration", None)),
        "artwork_url": artwork_url,
        "release_date": str(release_date) if release_date is not None else None,
    }


def album_tracks(album: Any) -> list[Any]:
    tracks = getattr(album, "tracks", None)
    if callable(tracks):
        return list(tracks())
    if tracks is not None:
        return list(tracks)
    get_tracks = getattr(album, "get_tracks", None)
    if callable(get_tracks):
        return list(get_tracks())
    items = getattr(album, "items", None)
    if callable(items):
        return list(items())
    return []


def normalized_duration_seconds(value: Any) -> int | None:
    if value is None:
        return None
    total_seconds = getattr(value, "total_seconds", None)
    if callable(total_seconds):
        try:
            value = total_seconds()
        except Exception:
            return None
    try:
        duration = float(value)
    except (TypeError, ValueError):
        return None
    if duration <= 0:
        return None

    # tidalapi/tidal clients have exposed durations in seconds, milliseconds,
    # microseconds, and occasionally nanoseconds depending on object source.
    if duration > 100_000_000_000:
        duration /= 1_000_000_000
    elif duration > 100_000_000:
        duration /= 1_000_000
    elif duration > 86_400:
        duration /= 1_000

    if duration <= 0 or duration > 86_400:
        return None
    return int(round(duration))


def enum_value(value: Any) -> str:
    raw_value = getattr(value, "value", None)
    if raw_value is not None:
        return str(raw_value)
    name = getattr(value, "name", None)
    if name is not None:
        return str(name)
    return str(value)


def normalized_mime_type(value: Any) -> str:
    raw = enum_value(value).strip()
    normalized = raw.lower().replace("_", "-")
    if "/" in normalized:
        return normalized
    if normalized.endswith(".flac") or normalized in {"flac", "mime-type.flac"}:
        return "audio/flac"
    if normalized.endswith(".m4a") or normalized in {"m4a", "mp4", "alac", "mime-type.m4a"}:
        return "audio/mp4"
    if normalized in {"aac", "mime-type.aac"}:
        return "audio/aac"
    if normalized in {"mpeg", "mp3", "mime-type.mp3", "mime-type.mpeg"}:
        return "audio/mpeg"
    if normalized in {"dash", "dash-xml", "vnd.mpeg.dash.mpd", "mpeg-dash"}:
        return "application/dash+xml"
    if normalized in {"hls", "mpegurl", "x-mpegurl", "vnd.apple.mpegurl"}:
        return "application/vnd.apple.mpegurl"
    return "application/octet-stream"


def playable_urls(manifest: Any) -> list[str]:
    urls = [str(url).strip() for url in manifest.get_urls() if str(url).strip()]
    dash_info = getattr(manifest, "dash_info", None)
    init_url = str(getattr(dash_info, "first_url", "") or "").strip()
    if init_url and (not urls or urls[0] != init_url):
        urls.insert(0, init_url)
    return unique_values(urls)


def unique_values(values: list[str]) -> list[str]:
    unique: list[str] = []
    for value in values:
        if value and value not in unique:
            unique.append(value)
    return unique


def stream_format(manifest: Any) -> str:
    if getattr(manifest, "is_mpd", False):
        return "mpd"
    if getattr(manifest, "is_bts", False):
        return "bts"
    return "unknown"


def emit(value: Any) -> None:
    print(json.dumps(value, separators=(",", ":")))


def parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="musicd TIDAL helper")
    parser.add_argument("--session-file", default="tidal-session.json")
    parser.add_argument("--quality", default="LOSSLESS")
    subparsers = parser.add_subparsers(dest="command", required=True)

    login = subparsers.add_parser("login")
    login.set_defaults(func=cmd_login)

    auth_url = subparsers.add_parser("auth-url")
    auth_url.set_defaults(func=cmd_auth_url)

    complete = subparsers.add_parser("complete-auth")
    complete.add_argument("redirect_url")
    complete.set_defaults(func=cmd_complete_auth)

    search = subparsers.add_parser("search-tracks")
    search.add_argument("query")
    search.add_argument("limit", type=int, nargs="?", default=20)
    search.set_defaults(func=cmd_search_tracks)

    search_albums = subparsers.add_parser("search-albums")
    search_albums.add_argument("query")
    search_albums.add_argument("limit", type=int, nargs="?", default=20)
    search_albums.set_defaults(func=cmd_search_albums)

    album_tracks_parser = subparsers.add_parser("album-tracks")
    album_tracks_parser.add_argument("album_id")
    album_tracks_parser.set_defaults(func=cmd_album_tracks)

    resolve = subparsers.add_parser("resolve-track")
    resolve.add_argument("track_id")
    resolve.set_defaults(func=cmd_resolve_track)

    return parser


def main() -> int:
    args = parser().parse_args()
    try:
        args.func(args)
        return 0
    except Exception as exc:
        print(str(exc), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
