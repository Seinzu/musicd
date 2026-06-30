use std::env;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use musicd_upnp::StreamResource;
use serde::{Deserialize, Serialize};

use crate::types::{PlaybackQueue, QueueEntry, QueueMutationEntry, QueuePlayableResource};
use crate::util::url_encode;

use super::ServiceState;

const TIDAL_HELPER_TIMEOUT: Duration = Duration::from_secs(20);
const TIDAL_STREAM_SCHEMA_VERSION: u32 = 2;
const TIDAL_STREAM_CACHE_LIMIT: usize = 64;
const TIDAL_COMPATIBILITY_QUALITIES: &[&str] = &["HIGH", "LOW"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TidalQueuedTrack {
    pub(crate) track_id: String,
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) album: Option<String>,
    pub(crate) duration_seconds: Option<u64>,
    pub(crate) artwork_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TidalQueuedAlbum {
    pub(crate) album_id: String,
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) track_count: Option<u64>,
    pub(crate) duration_seconds: Option<u64>,
    pub(crate) artwork_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TidalResolvedTrack {
    track_id: String,
    title: String,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    album: Option<String>,
    #[serde(default)]
    duration_seconds: Option<u64>,
    stream_url: String,
    #[serde(default)]
    stream_urls: Vec<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    artwork_url: Option<String>,
    #[serde(default)]
    manifest_mime_type: Option<String>,
    #[serde(default)]
    stream_format: Option<String>,
    #[serde(default)]
    helper_schema_version: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TidalStreamSource {
    Direct {
        url: String,
        mime_type: String,
        stream_format: String,
    },
    Segments {
        urls: Vec<String>,
        mime_type: String,
        stream_format: String,
    },
}

impl TidalStreamSource {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Direct { .. } => "direct",
            Self::Segments { .. } => "segments",
        }
    }

    fn needs_compatibility_fallback(&self) -> bool {
        matches!(self, Self::Segments { .. })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TidalSearchTrack {
    pub(crate) track_id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) artist: Option<String>,
    #[serde(default)]
    pub(crate) album: Option<String>,
    #[serde(default)]
    pub(crate) duration_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) artwork_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct TidalSearchAlbum {
    pub(crate) album_id: String,
    pub(crate) title: String,
    #[serde(default)]
    pub(crate) artist: Option<String>,
    #[serde(default)]
    pub(crate) track_count: Option<u64>,
    #[serde(default)]
    pub(crate) duration_seconds: Option<u64>,
    #[serde(default)]
    pub(crate) artwork_url: Option<String>,
    #[serde(default)]
    pub(crate) release_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TidalAlbumTracks {
    #[serde(default)]
    album: Option<TidalSearchAlbum>,
    #[serde(default)]
    tracks: Vec<TidalSearchTrack>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TidalAuthUrl {
    pub(crate) auth_url: String,
    #[serde(default)]
    pub(crate) session_file: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TidalAuthComplete {
    pub(crate) ok: bool,
    #[serde(default)]
    pub(crate) session_file: Option<String>,
}

impl ServiceState {
    pub(crate) fn tidal_auth_url(&self) -> io::Result<TidalAuthUrl> {
        let output = self.run_tidal_helper(&["auth-url"])?;
        serde_json::from_str::<TidalAuthUrl>(&output).map_err(tidal_json_error)
    }

    pub(crate) fn complete_tidal_auth(&self, redirect_url: &str) -> io::Result<TidalAuthComplete> {
        let output = self.run_tidal_helper(&["complete-auth", redirect_url])?;
        serde_json::from_str::<TidalAuthComplete>(&output).map_err(tidal_json_error)
    }

    pub(crate) fn replace_queue_with_tidal_track(
        &self,
        renderer_location: &str,
        queued_track: TidalQueuedTrack,
    ) -> io::Result<PlaybackQueue> {
        let title = queued_track
            .title
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&queued_track.track_id)
            .to_string();
        let entry = tidal_queue_entry(queued_track)?;
        let group = self.queue_target_group(renderer_location)?;
        let queue =
            self.database
                .replace_queue(renderer_location, &format!("TIDAL: {title}"), &[entry])?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(crate) fn append_tidal_track_to_queue(
        &self,
        renderer_location: &str,
        queued_track: TidalQueuedTrack,
    ) -> io::Result<PlaybackQueue> {
        let title = queued_track
            .title
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&queued_track.track_id)
            .to_string();
        let entry = tidal_queue_entry(queued_track)?;
        let group = self.queue_target_group(renderer_location)?;
        let queue = self.database.append_queue_entries(
            renderer_location,
            &format!("TIDAL: {title}"),
            &[entry],
        )?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(crate) fn play_next_tidal_track(
        &self,
        renderer_location: &str,
        queued_track: TidalQueuedTrack,
    ) -> io::Result<PlaybackQueue> {
        let title = queued_track
            .title
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&queued_track.track_id)
            .to_string();
        let entry = tidal_queue_entry(queued_track)?;
        let group = self.queue_target_group(renderer_location)?;
        let queue = self.database.insert_queue_entries_after_current(
            renderer_location,
            &format!("TIDAL: {title}"),
            &[entry],
        )?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(crate) fn replace_queue_with_tidal_album(
        &self,
        renderer_location: &str,
        queued_album: TidalQueuedAlbum,
    ) -> io::Result<PlaybackQueue> {
        let (title, entries) = self.tidal_album_queue_entries(&queued_album)?;
        let group = self.queue_target_group(renderer_location)?;
        let queue =
            self.database
                .replace_queue(renderer_location, &format!("TIDAL: {title}"), &entries)?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(crate) fn append_tidal_album_to_queue(
        &self,
        renderer_location: &str,
        queued_album: TidalQueuedAlbum,
    ) -> io::Result<PlaybackQueue> {
        let (title, entries) = self.tidal_album_queue_entries(&queued_album)?;
        let group = self.queue_target_group(renderer_location)?;
        let queue = self.database.append_queue_entries(
            renderer_location,
            &format!("TIDAL: {title}"),
            &entries,
        )?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(crate) fn play_next_tidal_album(
        &self,
        renderer_location: &str,
        queued_album: TidalQueuedAlbum,
    ) -> io::Result<PlaybackQueue> {
        let (title, entries) = self.tidal_album_queue_entries(&queued_album)?;
        let group = self.queue_target_group(renderer_location)?;
        let queue = self.database.insert_queue_entries_after_current(
            renderer_location,
            &format!("TIDAL: {title}"),
            &entries,
        )?;
        self.finish_queue_mutation(renderer_location, group.as_ref(), Some(&queue));
        Ok(queue)
    }

    pub(crate) fn playable_resource_for_queue_entry(
        &self,
        entry: &QueueEntry,
    ) -> io::Result<QueuePlayableResource> {
        if entry.source_kind == "tidal" {
            return self.playable_resource_for_tidal_entry(entry);
        }

        let track = self
            .find_track(&entry.track_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "queued track not found"))?;
        let resource = self.stream_resource_for_track(&track);
        Ok(QueuePlayableResource {
            id: track.id.clone(),
            title: track.title.clone(),
            duration_seconds: track.duration_seconds,
            resource,
            local_track: Some(track),
        })
    }

    pub(crate) fn search_tidal_tracks(
        &self,
        query: &str,
        limit: usize,
    ) -> io::Result<Vec<TidalSearchTrack>> {
        let output =
            self.run_tidal_helper(&["search-tracks", query, &limit.clamp(1, 50).to_string()])?;
        serde_json::from_str::<Vec<TidalSearchTrack>>(&output).map_err(tidal_json_error)
    }

    pub(crate) fn search_tidal_albums(
        &self,
        query: &str,
        limit: usize,
    ) -> io::Result<Vec<TidalSearchAlbum>> {
        let output =
            self.run_tidal_helper(&["search-albums", query, &limit.clamp(1, 50).to_string()])?;
        serde_json::from_str::<Vec<TidalSearchAlbum>>(&output).map_err(tidal_json_error)
    }

    fn playable_resource_for_tidal_entry(
        &self,
        entry: &QueueEntry,
    ) -> io::Result<QueuePlayableResource> {
        let queued = tidal_queued_track(entry)?;
        let resolved = self.resolve_tidal_track(&queued.track_id)?;
        let stream_source = self.compatible_tidal_stream_source(&queued.track_id, &resolved)?;
        self.cache_tidal_stream_source(&resolved.track_id, stream_source);
        let artist = resolved.artist.or(queued.artist.clone());
        let album = resolved.album.or(queued.album.clone());
        let title = first_non_empty([
            Some(resolved.title.as_str()),
            queued.title.as_deref(),
            Some(queued.track_id.as_str()),
        ])
        .to_string();
        let duration_seconds =
            normalize_tidal_duration_seconds(resolved.duration_seconds.or(queued.duration_seconds));
        let artwork_url = resolved.artwork_url.or(queued.artwork_url);
        Ok(QueuePlayableResource {
            id: format!("tidal:{}", resolved.track_id),
            title: title.clone(),
            duration_seconds,
            resource: StreamResource {
                stream_url: self.tidal_proxy_stream_url(&resolved.track_id),
                mime_type: normalize_tidal_mime_type(resolved.mime_type.as_deref()),
                title,
                artist,
                album,
                album_art_url: artwork_url,
            },
            local_track: None,
        })
    }

    pub(crate) fn tidal_stream_source(&self, track_id: &str) -> io::Result<TidalStreamSource> {
        if let Some(source) = self.cached_tidal_stream_source(track_id) {
            self.debug_log(
                "tidal-stream-cache-hit",
                format!("track_id={track_id} source={}", source.kind_name()),
            );
            return Ok(source);
        }
        self.debug_log(
            "tidal-stream-cache-miss",
            format!("track_id={track_id} resolving=true"),
        );
        let resolved = self.resolve_tidal_track(track_id)?;
        let source = self.compatible_tidal_stream_source(track_id, &resolved)?;
        self.cache_tidal_stream_source(track_id, source.clone());
        Ok(source)
    }

    fn compatible_tidal_stream_source(
        &self,
        track_id: &str,
        resolved: &TidalResolvedTrack,
    ) -> io::Result<TidalStreamSource> {
        let source = self.tidal_stream_source_from_resolved(resolved)?;
        if !source.needs_compatibility_fallback() {
            return Ok(source);
        }
        self.debug_log(
            "tidal-stream-compat-fallback",
            format!(
                "track_id={} original_source={} original_quality={}",
                track_id,
                source.kind_name(),
                self.config.tidal_audio_quality
            ),
        );
        for quality in TIDAL_COMPATIBILITY_QUALITIES {
            if quality.eq_ignore_ascii_case(&self.config.tidal_audio_quality) {
                continue;
            }
            match self.resolve_tidal_track_with_quality(track_id, quality) {
                Ok(candidate) => match self.tidal_stream_source_from_resolved(&candidate) {
                    Ok(candidate_source) if !candidate_source.needs_compatibility_fallback() => {
                        self.debug_log(
                            "tidal-stream-compat-selected",
                            format!(
                                "track_id={} quality={} source={}",
                                track_id,
                                quality,
                                candidate_source.kind_name()
                            ),
                        );
                        return Ok(candidate_source);
                    }
                    Ok(candidate_source) => self.debug_log(
                        "tidal-stream-compat-skipped",
                        format!(
                            "track_id={} quality={} source={} reason=still-segmented",
                            track_id,
                            quality,
                            candidate_source.kind_name()
                        ),
                    ),
                    Err(error) => self.debug_log(
                        "tidal-stream-compat-skipped",
                        format!("track_id={} quality={} error={}", track_id, quality, error),
                    ),
                },
                Err(error) => self.debug_log(
                    "tidal-stream-compat-skipped",
                    format!("track_id={} quality={} error={}", track_id, quality, error),
                ),
            }
        }
        Ok(source)
    }

    fn tidal_stream_source_from_resolved(
        &self,
        resolved: &TidalResolvedTrack,
    ) -> io::Result<TidalStreamSource> {
        resolved.ensure_supported_stream_schema()?;
        let mime_type = normalize_tidal_mime_type(resolved.mime_type.as_deref());
        let urls = resolved.stream_urls();
        let stream_format = resolved.stream_format();
        if urls.len() > 1 {
            return Ok(TidalStreamSource::Segments {
                urls,
                mime_type,
                stream_format,
            });
        }
        let url = urls
            .into_iter()
            .next()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "TIDAL stream has no URL"))?;
        Ok(TidalStreamSource::Direct {
            url,
            mime_type,
            stream_format,
        })
    }

    fn cached_tidal_stream_source(&self, track_id: &str) -> Option<TidalStreamSource> {
        self.tidal_stream_cache
            .lock()
            .expect("TIDAL stream cache should not be poisoned")
            .get(track_id)
            .cloned()
    }

    fn cache_tidal_stream_source(&self, track_id: &str, source: TidalStreamSource) {
        let mut cache = self
            .tidal_stream_cache
            .lock()
            .expect("TIDAL stream cache should not be poisoned");
        if cache.len() >= TIDAL_STREAM_CACHE_LIMIT
            && !cache.contains_key(track_id)
            && let Some(key) = cache.keys().next().cloned()
        {
            cache.remove(&key);
        }
        cache.insert(track_id.to_string(), source);
    }

    fn tidal_proxy_stream_url(&self, track_id: &str) -> String {
        format!(
            "{}/stream/tidal/{}",
            self.config.resolved_base_url().trim_end_matches('/'),
            url_encode(track_id)
        )
    }

    fn resolve_tidal_track(&self, track_id: &str) -> io::Result<TidalResolvedTrack> {
        let output = self.run_tidal_helper(&["resolve-track", track_id])?;
        serde_json::from_str::<TidalResolvedTrack>(&output).map_err(tidal_json_error)
    }

    fn resolve_tidal_track_with_quality(
        &self,
        track_id: &str,
        quality: &str,
    ) -> io::Result<TidalResolvedTrack> {
        let output = self.run_tidal_helper_with_quality(quality, &["resolve-track", track_id])?;
        serde_json::from_str::<TidalResolvedTrack>(&output).map_err(tidal_json_error)
    }

    fn tidal_album_tracks(&self, album_id: &str) -> io::Result<TidalAlbumTracks> {
        let output = self.run_tidal_helper(&["album-tracks", album_id])?;
        serde_json::from_str::<TidalAlbumTracks>(&output).map_err(tidal_json_error)
    }

    fn tidal_album_queue_entries(
        &self,
        queued_album: &TidalQueuedAlbum,
    ) -> io::Result<(String, Vec<QueueMutationEntry>)> {
        let album_id = queued_album.album_id.trim();
        if album_id.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing TIDAL album id",
            ));
        }
        let album_tracks = self.tidal_album_tracks(album_id)?;
        let album_title = first_non_empty([
            queued_album.title.as_deref(),
            album_tracks
                .album
                .as_ref()
                .map(|album| album.title.as_str()),
            Some(album_id),
        ])
        .to_string();
        let album_artwork_url = album_tracks
            .album
            .as_ref()
            .and_then(|album| album.artwork_url.clone())
            .or_else(|| queued_album.artwork_url.clone());
        let entries = album_tracks
            .tracks
            .into_iter()
            .map(|track| {
                tidal_queue_entry(TidalQueuedTrack {
                    track_id: track.track_id,
                    title: Some(track.title),
                    artist: track.artist,
                    album: track.album.or_else(|| Some(album_title.clone())),
                    duration_seconds: normalize_tidal_duration_seconds(track.duration_seconds),
                    artwork_url: track.artwork_url.or_else(|| album_artwork_url.clone()),
                })
            })
            .collect::<io::Result<Vec<_>>>()?;
        if entries.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("TIDAL album {album_id} has no tracks"),
            ));
        }
        Ok((album_title, entries))
    }

    fn run_tidal_helper(&self, args: &[&str]) -> io::Result<String> {
        self.run_tidal_helper_with_quality(&self.config.tidal_audio_quality, args)
    }

    fn run_tidal_helper_with_quality(&self, quality: &str, args: &[&str]) -> io::Result<String> {
        let (program, helper_args) = self.tidal_helper_command_parts()?;
        let mut command = Command::new(&program);
        command.args(&helper_args);
        command.arg("--session-file");
        command.arg(&self.config.tidal_session_path);
        command.arg("--quality");
        command.arg(quality);
        command.args(args);
        let output = command.output().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "failed to run TIDAL helper `{}` within {:?}: {error}",
                    format_helper_command(&program, &helper_args),
                    TIDAL_HELPER_TIMEOUT,
                ),
            )
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let message = first_non_empty([Some(stderr.trim()), Some(stdout.trim())]);
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("TIDAL helper failed: {message}"),
            ));
        }
        String::from_utf8(output.stdout)
            .map(|value| value.trim().to_string())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    fn tidal_helper_command_parts(&self) -> io::Result<(OsString, Vec<OsString>)> {
        if let Some(command) = self.config.tidal_helper_command.as_deref() {
            let mut parts = command.split_whitespace();
            let program = parts.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "TIDAL helper command is empty")
            })?;
            return Ok((
                OsString::from(program),
                parts.map(OsString::from).collect::<Vec<_>>(),
            ));
        }

        let helper_path = default_tidal_helper_path().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "TIDAL helper script was not found; set MUSICD_TIDAL_HELPER_COMMAND",
            )
        })?;
        let python = find_executable_in_path(&["python3", "python"]).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Python was not found; install python3 or set MUSICD_TIDAL_HELPER_COMMAND",
            )
        })?;
        Ok((python.into_os_string(), vec![helper_path.into_os_string()]))
    }
}

impl TidalResolvedTrack {
    fn ensure_supported_stream_schema(&self) -> io::Result<()> {
        if self.helper_schema_version == Some(TIDAL_STREAM_SCHEMA_VERSION) {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "TIDAL helper returned stream schema {:?}; expected {TIDAL_STREAM_SCHEMA_VERSION}. Rebuild/redeploy the helper so resolve-track returns stream_urls and stream_format.",
                self.helper_schema_version
            ),
        ))
    }

    fn stream_urls(&self) -> Vec<String> {
        let mut urls = Vec::new();
        for url in self
            .stream_urls
            .iter()
            .chain(std::iter::once(&self.stream_url))
        {
            let url = url.trim();
            if !url.is_empty() && !urls.iter().any(|existing| existing == url) {
                urls.push(url.to_string());
            }
        }
        urls
    }

    fn stream_format(&self) -> String {
        self.stream_format
            .as_deref()
            .or(self.manifest_mime_type.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown")
            .to_string()
    }
}

fn default_tidal_helper_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current_dir) = env::current_dir() {
        candidates.push(current_dir.join("scripts/tidal/tidalapi_helper.py"));
    }
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(repo_dir) = crate_dir.parent().and_then(Path::parent) {
        candidates.push(repo_dir.join("scripts/tidal/tidalapi_helper.py"));
    }
    if let Ok(current_exe) = env::current_exe() {
        if let Some(exe_dir) = current_exe.parent() {
            candidates.push(exe_dir.join("scripts/tidal/tidalapi_helper.py"));
            candidates.push(exe_dir.join("../scripts/tidal/tidalapi_helper.py"));
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn find_executable_in_path(names: &[&str]) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

fn format_helper_command(program: &OsString, args: &[OsString]) -> String {
    std::iter::once(program)
        .chain(args.iter())
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

fn tidal_queue_entry(queued_track: TidalQueuedTrack) -> io::Result<QueueMutationEntry> {
    let track_id = queued_track.track_id.trim().to_string();
    if track_id.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing TIDAL track id",
        ));
    }
    let source_ref = serde_json::to_string(&TidalQueuedTrack {
        track_id: track_id.clone(),
        duration_seconds: normalize_tidal_duration_seconds(queued_track.duration_seconds),
        ..queued_track
    })
    .map_err(tidal_json_error)?;
    Ok(QueueMutationEntry {
        track_id: format!("tidal:{track_id}"),
        album_id: None,
        source_kind: "tidal".to_string(),
        source_ref: Some(source_ref),
    })
}

fn tidal_queued_track(entry: &QueueEntry) -> io::Result<TidalQueuedTrack> {
    if let Some(source_ref) = entry.source_ref.as_deref() {
        let mut track: TidalQueuedTrack =
            serde_json::from_str(source_ref).map_err(tidal_json_error)?;
        track.duration_seconds = normalize_tidal_duration_seconds(track.duration_seconds);
        return Ok(track);
    }
    entry
        .track_id
        .strip_prefix("tidal:")
        .map(|track_id| TidalQueuedTrack {
            track_id: track_id.to_string(),
            title: None,
            artist: None,
            album: None,
            duration_seconds: None,
            artwork_url: None,
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing TIDAL track metadata"))
}

fn normalize_tidal_duration_seconds(value: Option<u64>) -> Option<u64> {
    let mut duration = value?;
    if duration > 100_000_000_000 {
        duration = duration.saturating_add(500_000_000) / 1_000_000_000;
    } else if duration > 100_000_000 {
        duration = duration.saturating_add(500_000) / 1_000_000;
    } else if duration > 86_400 {
        duration = duration.saturating_add(500) / 1_000;
    }
    (duration > 0 && duration <= 86_400).then_some(duration)
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> &'a str {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or("")
}

fn normalize_tidal_mime_type(value: Option<&str>) -> String {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return "application/octet-stream".to_string();
    };
    let normalized = value.to_ascii_lowercase().replace('_', "-");
    if normalized.contains('/') {
        return normalized;
    }
    match normalized.as_str() {
        "flac" | "mime-type.flac" => "audio/flac",
        "m4a" | "mp4" | "alac" | "mime-type.m4a" => "audio/mp4",
        "aac" | "mime-type.aac" => "audio/aac",
        "mp3" | "mpeg" | "mime-type.mp3" | "mime-type.mpeg" => "audio/mpeg",
        "dash" | "dash-xml" | "vnd.mpeg.dash.mpd" | "mpeg-dash" => "application/dash+xml",
        "hls" | "mpegurl" | "x-mpegurl" | "vnd.apple.mpegurl" => "application/vnd.apple.mpegurl",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn tidal_json_error(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
