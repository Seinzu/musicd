use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::library::{LibraryFileState, discover_audio_files, scan_library_file};
use crate::service::ServiceState;
use crate::types::LibraryTrack;

#[derive(Debug, Clone)]
struct PendingFile {
    state: LibraryFileState,
    first_seen: Instant,
}

pub(crate) fn spawn_library_watcher(state: Arc<ServiceState>) {
    if !state.config.library_watch_enabled {
        eprintln!("library watcher: disabled");
        return;
    }

    let interval = Duration::from_millis(state.config.library_watch_interval_ms.max(1_000));
    let settle = Duration::from_millis(state.config.library_watch_settle_ms);
    let builder = thread::Builder::new().name("musicd-library-watcher".to_string());
    if let Err(error) = builder.spawn(move || run_library_watcher(state, interval, settle)) {
        eprintln!("library watcher: failed to start: {error}");
    }
}

fn run_library_watcher(state: Arc<ServiceState>, interval: Duration, settle: Duration) {
    let mut pending = HashMap::new();
    eprintln!(
        "library watcher: polling {} every {}ms",
        state.config.library_path.display(),
        interval.as_millis()
    );

    loop {
        thread::sleep(interval);
        if let Err(error) = poll_library(&state, settle, &mut pending) {
            eprintln!("library watcher: poll failed: {error}");
        }
    }
}

fn poll_library(
    state: &ServiceState,
    settle: Duration,
    pending: &mut HashMap<String, PendingFile>,
) -> io::Result<()> {
    let files = discover_audio_files(&state.config.library_path)?;
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
        return Ok(());
    }

    let mut upsert_tracks = Vec::new();
    let mut completed_relative_paths = Vec::new();
    for file in &ready_files {
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
                eprintln!(
                    "library watcher: failed to scan {}: {error}",
                    file.path.display()
                );
            }
        }
    }

    let summary =
        state.apply_library_file_changes(upsert_tracks, deleted_relative_paths.clone())?;
    for relative_path in completed_relative_paths {
        pending.remove(&relative_path);
    }
    for relative_path in deleted_relative_paths {
        pending.remove(&relative_path);
    }

    if summary.upserted > 0 || summary.removed > 0 {
        eprintln!(
            "library watcher: applied {} upserts and {} removals",
            summary.upserted, summary.removed
        );
    }

    Ok(())
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
