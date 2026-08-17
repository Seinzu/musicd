use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use musicd_core::LibraryWatchMode;
use notify::event::ModifyKind;
use notify::{Event, EventKind, RecursiveMode, Watcher};

use crate::library::{LibraryFileState, discover_audio_files_until, scan_library_file};
use crate::service::ServiceState;
use crate::types::LibraryTrack;
use crate::util::{component_to_string, is_supported_audio_file, should_skip_entry};

const ACTIVE_PLAYBACK_RETRY: Duration = Duration::from_secs(1);
const RECONCILE_ERROR_RETRY: Duration = Duration::from_secs(30);
const MAX_EVENT_LOOP_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
struct PendingFile {
    state: LibraryFileState,
    first_seen: Instant,
}

#[derive(Debug, Default)]
struct PollSummary {
    discovered_files: usize,
    visited_dirs: usize,
    skipped_entries: usize,
    upserted: usize,
    removed: usize,
    scan_failures: usize,
}

#[derive(Debug, Default)]
struct EventBatchSummary {
    paths: usize,
    upserted: usize,
    removed: usize,
    scan_failures: usize,
    retry_paths: Vec<PathBuf>,
}

pub(crate) fn spawn_library_watcher(state: Arc<ServiceState>) {
    if !state.config.library_watch_enabled {
        eprintln!("library watcher: disabled");
        return;
    }

    let poll_interval = Duration::from_millis(state.config.library_watch_interval_ms.max(1_000));
    let settle = Duration::from_millis(state.config.library_watch_settle_ms);
    let reconcile_interval =
        Duration::from_millis(state.config.library_watch_reconcile_interval_ms.max(60_000));
    let mode = state.config.library_watch_mode;
    let builder = thread::Builder::new().name("musicd-library-watcher".to_string());
    if let Err(error) = builder.spawn(move || match mode {
        LibraryWatchMode::Hybrid => {
            run_hybrid_watcher(state, settle, reconcile_interval);
        }
        LibraryWatchMode::Poll => run_poll_watcher(state, poll_interval, settle),
    }) {
        eprintln!("library watcher: failed to start: {error}");
    }
}

fn run_poll_watcher(state: Arc<ServiceState>, interval: Duration, settle: Duration) {
    let mut pending = HashMap::new();
    eprintln!(
        "library watcher: polling {} every {}ms",
        state.config.library_path.display(),
        interval.as_millis()
    );

    loop {
        thread::sleep(interval);
        run_reconciliation(&state, settle, &mut pending);
    }
}

fn run_hybrid_watcher(state: Arc<ServiceState>, settle: Duration, reconcile_interval: Duration) {
    let (event_tx, event_rx) = mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |event| {
        let _ = event_tx.send(event);
    }) {
        Ok(watcher) => watcher,
        Err(error) => {
            native_watcher_failed(&state, &error);
            run_reconciliation_only(state, reconcile_interval);
            return;
        }
    };

    if let Err(error) = watcher.watch(&state.config.library_path, RecursiveMode::Recursive) {
        native_watcher_failed(&state, &error);
        run_reconciliation_only(state, reconcile_interval);
        return;
    }

    if let Some(metrics) = state.metrics() {
        metrics.set_library_watcher_native_active(true);
    }
    eprintln!(
        "library watcher: hybrid native events for {} (settle={}ms, reconcile={}ms)",
        state.config.library_path.display(),
        settle.as_millis(),
        reconcile_interval.as_millis()
    );

    let mut pending_paths = HashMap::<PathBuf, Instant>::new();
    let mut poll_pending = HashMap::new();
    let mut next_reconcile = Instant::now() + reconcile_interval;

    loop {
        let now = Instant::now();
        let next_path = pending_paths.values().copied().min();
        let deadline = next_path
            .map(|path_deadline| path_deadline.min(next_reconcile))
            .unwrap_or(next_reconcile);
        let timeout = deadline
            .saturating_duration_since(now)
            .min(MAX_EVENT_LOOP_WAIT);

        match event_rx.recv_timeout(timeout) {
            Ok(Ok(event)) => {
                receive_native_event(
                    &state,
                    event,
                    settle,
                    &mut pending_paths,
                    &mut next_reconcile,
                );
            }
            Ok(Err(error)) => {
                if let Some(metrics) = state.metrics() {
                    metrics.record_library_watcher_event("error");
                }
                eprintln!("library watcher: native event error: {error}");
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                eprintln!(
                    "library watcher: native event channel disconnected; continuing with reconciliation"
                );
                if let Some(metrics) = state.metrics() {
                    metrics.set_library_watcher_native_active(false);
                }
                run_reconciliation_only(state, reconcile_interval);
                return;
            }
        }

        let now = Instant::now();
        let ready_pending_paths = pending_paths
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        if !ready_pending_paths.is_empty() {
            if state.active_library_stream_count() > 0 {
                record_deferred_event_batch(&state);
                postpone_paths(
                    &mut pending_paths,
                    &ready_pending_paths,
                    ACTIVE_PLAYBACK_RETRY,
                );
            } else {
                let ready_paths = coalesce_paths(ready_pending_paths.clone());
                match apply_event_paths(&state, &ready_paths) {
                    Ok(summary) => {
                        for path in ready_pending_paths {
                            pending_paths.remove(&path);
                        }
                        postpone_paths(
                            &mut pending_paths,
                            &summary.retry_paths,
                            RECONCILE_ERROR_RETRY,
                        );
                        if let Some(metrics) = state.metrics() {
                            let outcome = if summary.scan_failures > 0 {
                                "partial"
                            } else {
                                "completed"
                            };
                            metrics.record_library_watcher_event_batch(
                                outcome,
                                summary.upserted,
                                summary.removed,
                            );
                        }
                        if summary.upserted > 0 || summary.removed > 0 {
                            eprintln!(
                                "library watcher: native batch processed {} paths, applied {} upserts and {} removals",
                                summary.paths, summary.upserted, summary.removed
                            );
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                        record_deferred_event_batch(&state);
                        postpone_paths(
                            &mut pending_paths,
                            &ready_pending_paths,
                            ACTIVE_PLAYBACK_RETRY,
                        );
                    }
                    Err(error) => {
                        if let Some(metrics) = state.metrics() {
                            metrics.record_library_watcher_event_batch("error", 0, 0);
                        }
                        eprintln!("library watcher: native batch failed: {error}");
                        postpone_paths(
                            &mut pending_paths,
                            &ready_pending_paths,
                            RECONCILE_ERROR_RETRY,
                        );
                    }
                }
            }
            if let Some(metrics) = state.metrics() {
                metrics.set_library_watcher_pending_paths(pending_paths.len());
            }
        }

        if Instant::now() >= next_reconcile {
            if run_reconciliation(&state, Duration::ZERO, &mut poll_pending) {
                next_reconcile = Instant::now() + reconcile_interval;
            } else {
                next_reconcile = Instant::now() + RECONCILE_ERROR_RETRY;
            }
        }
    }
}

fn run_reconciliation_only(state: Arc<ServiceState>, interval: Duration) {
    let mut pending = HashMap::new();
    eprintln!(
        "library watcher: reconciliation-only polling {} every {}ms",
        state.config.library_path.display(),
        interval.as_millis()
    );
    let mut delay = interval;
    loop {
        thread::sleep(delay);
        delay = if run_reconciliation(&state, Duration::ZERO, &mut pending) {
            interval
        } else {
            RECONCILE_ERROR_RETRY
        };
    }
}

fn native_watcher_failed(state: &ServiceState, error: &notify::Error) {
    if let Some(metrics) = state.metrics() {
        metrics.set_library_watcher_native_active(false);
        metrics.record_library_watcher_event("error");
    }
    eprintln!("library watcher: native watch unavailable: {error}");
}

fn receive_native_event(
    state: &ServiceState,
    event: Event,
    settle: Duration,
    pending_paths: &mut HashMap<PathBuf, Instant>,
    next_reconcile: &mut Instant,
) {
    if event.need_rescan() {
        *next_reconcile = Instant::now();
        if let Some(metrics) = state.metrics() {
            metrics.record_library_watcher_event("rescan");
        }
        return;
    }

    if !event_affects_library(&event.kind) {
        if let Some(metrics) = state.metrics() {
            metrics.record_library_watcher_event("ignored");
        }
        return;
    }

    let deadline = Instant::now() + settle;
    let mut queued = 0;
    for path in event.paths {
        if path == state.config.library_path {
            *next_reconcile = Instant::now();
            continue;
        }
        if path_is_watchable(&state.config.library_path, &path) {
            pending_paths.insert(path, deadline);
            queued += 1;
        }
    }
    if let Some(metrics) = state.metrics() {
        metrics.record_library_watcher_event(if queued > 0 { "received" } else { "ignored" });
        metrics.set_library_watcher_pending_paths(pending_paths.len());
    }
}

fn event_affects_library(kind: &EventKind) -> bool {
    !matches!(
        kind,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_))
    )
}

fn path_is_watchable(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    !relative.as_os_str().is_empty()
        && !relative.components().any(|component| {
            component_to_string(component)
                .map(|value| should_skip_entry(&value))
                .unwrap_or(true)
        })
}

fn coalesce_paths(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort_by_key(|path| path.components().count());
    let mut output = Vec::<PathBuf>::new();
    for path in paths {
        if !output.iter().any(|ancestor| path.starts_with(ancestor)) {
            output.push(path);
        }
    }
    output
}

fn postpone_paths(
    pending_paths: &mut HashMap<PathBuf, Instant>,
    paths: &[PathBuf],
    delay: Duration,
) {
    let deadline = Instant::now() + delay;
    for path in paths {
        pending_paths.insert(path.clone(), deadline);
    }
}

fn apply_event_paths(state: &ServiceState, paths: &[PathBuf]) -> io::Result<EventBatchSummary> {
    let mut summary = EventBatchSummary {
        paths: paths.len(),
        ..EventBatchSummary::default()
    };
    let library = state.library_snapshot();
    let library_by_path = library
        .tracks
        .iter()
        .map(|track| (track.relative_path.clone(), track.clone()))
        .collect::<HashMap<_, _>>();
    let mut upsert_tracks = HashMap::<String, LibraryTrack>::new();
    let mut deleted_relative_paths = HashSet::<String>::new();

    for path in paths {
        if state.active_library_stream_count() > 0 {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "library watcher deferred for active playback",
            ));
        }

        match fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => {
                collect_directory_changes(
                    state,
                    path,
                    &library_by_path,
                    &mut upsert_tracks,
                    &mut deleted_relative_paths,
                    &mut summary,
                )?;
            }
            Ok(metadata) if metadata.is_file() && is_supported_audio_file(path) => {
                collect_file_change(
                    state,
                    path,
                    &mut upsert_tracks,
                    &mut deleted_relative_paths,
                    &mut summary,
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Some(relative_path) = relative_path(&state.config.library_path, path) {
                    for indexed_path in library_by_path.keys() {
                        if path_is_at_or_below(indexed_path, &relative_path) {
                            deleted_relative_paths.insert(indexed_path.clone());
                        }
                    }
                }
            }
            Err(error) => return Err(error),
        }
    }

    for relative_path in upsert_tracks.keys() {
        deleted_relative_paths.remove(relative_path);
    }
    if state.active_library_stream_count() > 0 {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "library watcher deferred for active playback",
        ));
    }
    let change_summary = state.apply_library_file_changes(
        upsert_tracks.into_values().collect(),
        deleted_relative_paths.into_iter().collect(),
    )?;
    summary.upserted = change_summary.upserted;
    summary.removed = change_summary.removed;
    Ok(summary)
}

fn collect_directory_changes(
    state: &ServiceState,
    path: &Path,
    library_by_path: &HashMap<String, LibraryTrack>,
    upsert_tracks: &mut HashMap<String, LibraryTrack>,
    deleted_relative_paths: &mut HashSet<String>,
    summary: &mut EventBatchSummary,
) -> io::Result<()> {
    let Some(prefix) = relative_path(&state.config.library_path, path) else {
        return Ok(());
    };
    let discovery = discover_audio_files_until(path, || state.active_library_stream_count() > 0)?;
    let mut current_paths = HashSet::new();

    for file in discovery.files {
        if state.active_library_stream_count() > 0 {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "library watcher deferred for active playback",
            ));
        }
        let Some(relative_path) = relative_path(&state.config.library_path, &file.path) else {
            continue;
        };
        let file = LibraryFileState {
            relative_path: relative_path.clone(),
            ..file
        };
        current_paths.insert(relative_path.clone());
        let changed = library_by_path
            .get(&relative_path)
            .map(|track| file_changed(track, &file))
            .unwrap_or(true);
        if changed {
            collect_file_change(
                state,
                &file.path,
                upsert_tracks,
                deleted_relative_paths,
                summary,
            );
        }
    }

    for indexed_path in library_by_path.keys() {
        if path_is_at_or_below(indexed_path, &prefix) && !current_paths.contains(indexed_path) {
            deleted_relative_paths.insert(indexed_path.clone());
        }
    }
    Ok(())
}

fn collect_file_change(
    state: &ServiceState,
    path: &Path,
    upsert_tracks: &mut HashMap<String, LibraryTrack>,
    deleted_relative_paths: &mut HashSet<String>,
    summary: &mut EventBatchSummary,
) {
    match scan_library_file(&state.config.library_path, path, &state.config.config_path) {
        Ok(Some(track)) => {
            deleted_relative_paths.remove(&track.relative_path);
            upsert_tracks.insert(track.relative_path.clone(), track);
        }
        Ok(None) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(relative_path) = relative_path(&state.config.library_path, path) {
                upsert_tracks.remove(&relative_path);
                deleted_relative_paths.insert(relative_path);
            }
        }
        Err(error) => {
            summary.scan_failures += 1;
            summary.retry_paths.push(path.to_path_buf());
            eprintln!(
                "library watcher: failed to scan native event path {}: {error}",
                path.display()
            );
        }
    }
}

fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    let components = relative
        .components()
        .map(component_to_string)
        .collect::<Option<Vec<_>>>()?;
    Some(components.join("/"))
}

fn path_is_at_or_below(candidate: &str, prefix: &str) -> bool {
    candidate == prefix
        || candidate
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn run_reconciliation(
    state: &ServiceState,
    settle: Duration,
    pending: &mut HashMap<String, PendingFile>,
) -> bool {
    if state.active_library_stream_count() > 0 {
        record_deferred_poll(state);
        return false;
    }

    let started = Instant::now();
    match poll_library(state, settle, pending) {
        Ok(summary) => {
            let outcome = if summary.scan_failures > 0 {
                "partial"
            } else {
                "completed"
            };
            if let Some(metrics) = state.metrics() {
                metrics.record_library_watcher_poll(
                    outcome,
                    started.elapsed(),
                    summary.discovered_files,
                    summary.visited_dirs,
                    summary.skipped_entries,
                    summary.upserted,
                    summary.removed,
                );
            }
            true
        }
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
            record_deferred_poll(state);
            false
        }
        Err(error) => {
            if let Some(metrics) = state.metrics() {
                metrics.record_library_watcher_error(started.elapsed());
            }
            eprintln!("library watcher: reconciliation failed: {error}");
            false
        }
    }
}

fn record_deferred_poll(state: &ServiceState) {
    if let Some(metrics) = state.metrics() {
        metrics.record_library_watcher_deferred();
    }
    state.debug_log(
        "library-watcher-deferred",
        format!(
            "kind=reconciliation active_library_streams={}",
            state.active_library_stream_count()
        ),
    );
}

fn record_deferred_event_batch(state: &ServiceState) {
    if let Some(metrics) = state.metrics() {
        metrics.record_library_watcher_event_batch("deferred", 0, 0);
    }
    state.debug_log(
        "library-watcher-deferred",
        format!(
            "kind=native-event active_library_streams={}",
            state.active_library_stream_count()
        ),
    );
}

fn poll_library(
    state: &ServiceState,
    settle: Duration,
    pending: &mut HashMap<String, PendingFile>,
) -> io::Result<PollSummary> {
    let discovery = discover_audio_files_until(&state.config.library_path, || {
        state.active_library_stream_count() > 0
    })?;
    let mut summary = PollSummary {
        discovered_files: discovery.files.len(),
        visited_dirs: discovery.visited_dirs,
        skipped_entries: discovery.skipped_entries,
        ..PollSummary::default()
    };
    let files = discovery.files;
    let current_files = files
        .iter()
        .map(|file| (file.relative_path.clone(), file.clone()))
        .collect::<HashMap<_, _>>();
    let current_paths = current_files.keys().cloned().collect::<HashSet<_>>();

    let library = state.library_snapshot();
    let library_by_path = library
        .tracks
        .iter()
        .map(|track| (track.relative_path.clone(), track.clone()))
        .collect::<HashMap<_, _>>();

    let deleted_relative_paths = library_by_path
        .keys()
        .filter(|relative_path| !current_paths.contains(*relative_path))
        .cloned()
        .collect::<Vec<_>>();

    let mut ready_files = Vec::new();
    for file in files {
        let changed = library_by_path
            .get(&file.relative_path)
            .map(|track| file_changed(track, &file))
            .unwrap_or(true);

        if changed && file_is_settled(&file, settle, pending) {
            ready_files.push(file);
        }
    }

    pending.retain(|relative_path, _| current_files.contains_key(relative_path));

    if ready_files.is_empty() && deleted_relative_paths.is_empty() {
        return Ok(summary);
    }

    let mut upsert_tracks = Vec::new();
    let mut completed_relative_paths = Vec::new();
    for file in &ready_files {
        if state.active_library_stream_count() > 0 {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "library watcher deferred for active playback",
            ));
        }
        match scan_library_file(
            &state.config.library_path,
            &file.path,
            &state.config.config_path,
        ) {
            Ok(Some(track)) => {
                upsert_tracks.push(track);
                completed_relative_paths.push(file.relative_path.clone());
            }
            Ok(None) => completed_relative_paths.push(file.relative_path.clone()),
            Err(error) => {
                summary.scan_failures += 1;
                eprintln!(
                    "library watcher: failed to scan {}: {error}",
                    file.path.display()
                );
            }
        }
    }

    let change_summary =
        state.apply_library_file_changes(upsert_tracks, deleted_relative_paths.clone())?;
    for relative_path in completed_relative_paths {
        pending.remove(&relative_path);
    }
    for relative_path in deleted_relative_paths {
        pending.remove(&relative_path);
    }

    if change_summary.upserted > 0 || change_summary.removed > 0 {
        eprintln!(
            "library watcher: applied {} upserts and {} removals",
            change_summary.upserted, change_summary.removed
        );
    }

    summary.upserted = change_summary.upserted;
    summary.removed = change_summary.removed;

    Ok(summary)
}

fn file_changed(track: &LibraryTrack, file: &LibraryFileState) -> bool {
    track.file_size != file.file_size || track.modified_unix_millis != file.modified_unix_millis
}

fn file_is_settled(
    file: &LibraryFileState,
    settle: Duration,
    pending: &mut HashMap<String, PendingFile>,
) -> bool {
    if settle.is_zero() {
        return true;
    }

    match pending.get_mut(&file.relative_path) {
        Some(pending_file) if same_fingerprint(&pending_file.state, file) => {
            pending_file.first_seen.elapsed() >= settle
        }
        Some(pending_file) => {
            pending_file.state = file.clone();
            pending_file.first_seen = Instant::now();
            false
        }
        None => {
            pending.insert(
                file.relative_path.clone(),
                PendingFile {
                    state: file.clone(),
                    first_seen: Instant::now(),
                },
            );
            false
        }
    }
}

fn same_fingerprint(left: &LibraryFileState, right: &LibraryFileState) -> bool {
    left.file_size == right.file_size && left.modified_unix_millis == right.modified_unix_millis
}

#[cfg(test)]
mod tests {
    use super::{coalesce_paths, event_affects_library, path_is_at_or_below, path_is_watchable};
    use notify::EventKind;
    use notify::event::{AccessKind, CreateKind, MetadataKind, ModifyKind, RemoveKind};
    use std::path::{Path, PathBuf};

    #[test]
    fn ignores_access_and_metadata_only_events() {
        assert!(!event_affects_library(&EventKind::Access(AccessKind::Any)));
        assert!(!event_affects_library(&EventKind::Modify(
            ModifyKind::Metadata(MetadataKind::Any)
        )));
        assert!(event_affects_library(&EventKind::Create(CreateKind::Any)));
        assert!(event_affects_library(&EventKind::Remove(RemoveKind::Any)));
    }

    #[test]
    fn filters_root_outside_and_hidden_paths() {
        let root = Path::new("/music");
        assert!(!path_is_watchable(root, Path::new("/music")));
        assert!(!path_is_watchable(root, Path::new("/other/song.flac")));
        assert!(!path_is_watchable(
            root,
            Path::new("/music/.cache/song.flac")
        ));
        assert!(!path_is_watchable(
            root,
            Path::new("/music/Artist/@eaDir/song.flac")
        ));
        assert!(path_is_watchable(
            root,
            Path::new("/music/Artist/Album/song.flac")
        ));
    }

    #[test]
    fn coalesces_descendant_paths() {
        let paths = vec![
            PathBuf::from("/music/Artist/Album/song.flac"),
            PathBuf::from("/music/Artist"),
            PathBuf::from("/music/Other/song.flac"),
        ];
        assert_eq!(
            coalesce_paths(paths),
            vec![
                PathBuf::from("/music/Artist"),
                PathBuf::from("/music/Other/song.flac")
            ]
        );
    }

    #[test]
    fn matches_exact_paths_and_directory_descendants() {
        assert!(path_is_at_or_below("Artist/song.flac", "Artist/song.flac"));
        assert!(path_is_at_or_below("Artist/Album/song.flac", "Artist"));
        assert!(!path_is_at_or_below("Artist Two/song.flac", "Artist"));
    }
}
