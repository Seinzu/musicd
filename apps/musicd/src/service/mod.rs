use std::io;
use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwap;
use musicd_core::AppConfig;

use crate::db::Database;
use crate::library::{Library, ScanProgressEvent, scan_library, scan_library_with_progress};
use crate::metrics;
use crate::renderer::{RendererBackend, RendererBackends};
use crate::types::{
    AlbumSummary, ArtistSummary, DirectStreamMetadata, LibraryTrack, LikeResult, PlaybackQueue,
    PlaybackSession, RendererRecord,
};
use crate::util::now_unix_timestamp;

mod artwork;
pub(crate) mod events;
mod groups;
mod poll;
mod queue;
mod radio;
mod recommendations;
mod renderers;
mod transport;

pub(crate) use events::PlaybackEvents;
pub(crate) use poll::{
    next_queue_entry_after, previous_queue_entry_before, queue_status_for_transport,
    spawn_queue_worker,
};
#[cfg(test)]
pub(crate) use poll::{should_adopt_preloaded_next_entry, should_auto_advance};

#[derive(Debug)]
pub(crate) struct ServiceState {
    pub(crate) config: AppConfig,
    pub(crate) database: Database,
    pub(crate) library: ArcSwap<Library>,
    pub(crate) renderer_backends: RendererBackends,
    pub(crate) metrics: OnceLock<Arc<metrics::Metrics>>,
    pub(crate) events: PlaybackEvents,
    /// State for tracking concurrent rescans
    pub(crate) rescan_state: RescanState,
}

/// State for tracking an active rescan operation
#[derive(Debug)]
pub(crate) struct RescanState {
    is_scanning: Arc<std::sync::atomic::AtomicBool>,
}

impl RescanState {
    pub(crate) fn new() -> Self {
        Self {
            is_scanning: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn try_start(&self) -> bool {
        !self
            .is_scanning
            .swap(true, std::sync::atomic::Ordering::AcqRel)
    }

    fn finish(&self) {
        self.is_scanning
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

struct RescanGuard<'a>(&'a RescanState);

impl Drop for RescanGuard<'_> {
    fn drop(&mut self) {
        self.0.finish();
    }
}

impl ServiceState {
    pub(crate) fn debug_enabled(&self) -> bool {
        self.config.debug_mode
    }

    pub(crate) fn metrics(&self) -> Option<&metrics::Metrics> {
        self.metrics.get().map(|arc| arc.as_ref())
    }

    pub(crate) fn install_metrics(&self, metrics: Arc<metrics::Metrics>) {
        let _ = self.metrics.set(metrics);
    }

    pub(crate) fn debug_log(&self, event: &str, details: impl AsRef<str>) {
        if self.debug_enabled() {
            eprintln!(
                "[musicd-debug][{}][{}] {}",
                now_unix_timestamp(),
                event,
                details.as_ref()
            );
        }
    }

    pub(crate) fn load(config: AppConfig) -> io::Result<Self> {
        let database = Database::open(&config.config_path)?;
        let persisted_library = database.load_library(config.library_path.clone())?;
        let state = Self {
            config,
            database,
            library: ArcSwap::from_pointee(persisted_library),
            renderer_backends: RendererBackends::default(),
            metrics: OnceLock::new(),
            events: PlaybackEvents::new(),
            rescan_state: RescanState::new(),
        };

        let persisted_track_count = state.track_count();
        if state.config.skip_startup_scan && persisted_track_count > 0 {
            eprintln!(
                "library scan: skipped startup scan, using persisted index with {} tracks",
                persisted_track_count
            );
        } else {
            if state.config.skip_startup_scan {
                eprintln!(
                    "library scan: startup scan requested to skip but persisted index is empty"
                );
            }
            eprintln!(
                "library scan: starting initial scan of {}",
                state.config.library_path.display()
            );
            match scan_library(&state.config.library_path, &state.config.config_path) {
                Ok(tracks) => state.replace_library(tracks)?,
                Err(error) if state.track_count() > 0 => {
                    eprintln!("library scan failed, continuing with persisted index: {error}");
                }
                Err(error) => return Err(error),
            }
        }

        state.debug_log(
            "service-load",
            format!(
                "tracks={} default_renderer={}",
                state.track_count(),
                state
                    .config
                    .default_renderer_location
                    .as_deref()
                    .unwrap_or("<none>")
            ),
        );

        Ok(state)
    }

    pub(crate) fn library_snapshot(&self) -> Arc<Library> {
        self.library.load_full()
    }

    pub(crate) fn track_count(&self) -> usize {
        self.library_snapshot().tracks.len()
    }

    pub(crate) fn tracks_snapshot(&self) -> Arc<[LibraryTrack]> {
        Arc::clone(&self.library_snapshot().tracks)
    }

    pub(crate) fn albums_snapshot(&self) -> Arc<[AlbumSummary]> {
        Arc::clone(&self.library_snapshot().albums)
    }

    pub(crate) fn artists_snapshot(&self) -> Arc<[ArtistSummary]> {
        Arc::clone(&self.library_snapshot().artists)
    }

    pub(crate) fn find_track(&self, track_id: &str) -> Option<LibraryTrack> {
        let library = self.library_snapshot();
        library
            .track_index
            .get(track_id)
            .map(|&idx| library.tracks[idx].clone())
    }

    pub(crate) fn find_album(&self, album_id: &str) -> Option<AlbumSummary> {
        let library = self.library_snapshot();
        library
            .album_index
            .get(album_id)
            .map(|&idx| library.albums[idx].clone())
    }

    pub(crate) fn find_artist(&self, artist_id: &str) -> Option<ArtistSummary> {
        let library = self.library_snapshot();
        library
            .artist_index
            .get(artist_id)
            .map(|&idx| library.artists[idx].clone())
    }

    pub(crate) fn tracks_for_album(&self, album_id: &str) -> Vec<LibraryTrack> {
        let library = self.library_snapshot();
        library
            .tracks_by_album
            .get(album_id)
            .map(|indexes| {
                indexes
                    .iter()
                    .map(|&idx| library.tracks[idx].clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn first_track_for_album(&self, album_id: &str) -> Option<LibraryTrack> {
        let library = self.library_snapshot();
        library
            .tracks_by_album
            .get(album_id)
            .and_then(|indexes| indexes.first())
            .map(|&idx| library.tracks[idx].clone())
    }

    pub(crate) fn albums_for_artist(&self, artist_id: &str) -> Vec<AlbumSummary> {
        let library = self.library_snapshot();
        library
            .albums_by_artist
            .get(artist_id)
            .map(|indexes| {
                indexes
                    .iter()
                    .map(|&idx| library.albums[idx].clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn like_item(
        &self,
        item_kind: &str,
        item_id: &str,
        client_id: &str,
    ) -> io::Result<LikeResult> {
        match item_kind {
            "album" if self.find_album(item_id).is_none() => {
                return Err(io::Error::new(io::ErrorKind::NotFound, "album not found"));
            }
            "track" if self.find_track(item_id).is_none() => {
                return Err(io::Error::new(io::ErrorKind::NotFound, "track not found"));
            }
            "album" | "track" => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "item_kind must be album or track",
                ));
            }
        }

        let created =
            self.database
                .add_item_like(item_kind, item_id, client_id, now_unix_timestamp())?;
        let like_count = self.database.count_item_likes(item_kind, item_id)?;
        Ok(LikeResult {
            item_kind: item_kind.to_string(),
            item_id: item_id.to_string(),
            like_count,
            liked_by_client: true,
            created,
        })
    }

    pub(crate) fn album_like_counts(&self) -> std::collections::HashMap<String, u64> {
        self.database.item_like_counts("album").unwrap_or_default()
    }

    pub(crate) fn track_like_counts(&self) -> std::collections::HashMap<String, u64> {
        self.database.item_like_counts("track").unwrap_or_default()
    }

    pub(crate) fn client_liked_album_ids(
        &self,
        client_id: Option<&str>,
    ) -> std::collections::HashSet<String> {
        self.database
            .client_liked_item_ids("album", client_id)
            .unwrap_or_default()
    }

    pub(crate) fn client_liked_track_ids(
        &self,
        client_id: Option<&str>,
    ) -> std::collections::HashSet<String> {
        self.database
            .client_liked_item_ids("track", client_id)
            .unwrap_or_default()
    }

    pub(crate) fn queue_snapshot(&self, renderer_location: &str) -> Option<PlaybackQueue> {
        self.database.load_queue(renderer_location).ok().flatten()
    }

    pub(crate) fn playback_session(&self, renderer_location: &str) -> Option<PlaybackSession> {
        self.database
            .load_playback_session(renderer_location)
            .ok()
            .flatten()
    }

    pub(crate) fn direct_stream_metadata(
        &self,
        renderer_location: &str,
    ) -> Option<DirectStreamMetadata> {
        let session = self.playback_session(renderer_location)?;
        let metadata = self
            .database
            .load_direct_stream_metadata(renderer_location)
            .ok()
            .flatten()?;
        (session.current_track_uri.as_deref() == Some(metadata.current_track_uri.as_str()))
            .then_some(metadata)
    }

    fn begin_rescan(&self) -> io::Result<RescanGuard<'_>> {
        if !self.rescan_state.try_start() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "A library rescan is already in progress",
            ));
        }
        Ok(RescanGuard(&self.rescan_state))
    }

    pub(crate) fn start_rescan(&self) -> io::Result<usize> {
        let _guard = self.begin_rescan()?;
        eprintln!(
            "library scan: starting manual rescan of {}",
            self.config.library_path.display()
        );
        let tracks = scan_library(&self.config.library_path, &self.config.config_path)?;
        let track_count = tracks.len();
        self.replace_library(tracks)?;
        Ok(track_count)
    }

    pub(crate) fn start_rescan_with_progress<F>(&self, report_progress: F) -> io::Result<usize>
    where
        F: Fn(ScanProgressEvent) -> io::Result<()> + Sync,
    {
        let _guard = self.begin_rescan()?;
        eprintln!(
            "library scan: starting manual rescan of {}",
            self.config.library_path.display()
        );

        let tracks = scan_library_with_progress(
            &self.config.library_path,
            &self.config.config_path,
            &report_progress,
        )?;

        let track_count = tracks.len();
        report_progress(ScanProgressEvent {
            stage: "saving_library".to_string(),
            current: track_count,
            total: None,
            percent: Some(96),
            message: Some(format!("Saving index for {track_count} tracks")),
        })?;
        self.replace_library(tracks)?;
        Ok(track_count)
    }

    pub(crate) fn replace_library(&self, tracks: Vec<LibraryTrack>) -> io::Result<()> {
        eprintln!(
            "library scan: building library index for {} tracks",
            tracks.len()
        );
        let overrides = self
            .database
            .list_album_artwork_overrides()
            .unwrap_or_default();
        let library = Library::build(self.config.library_path.clone(), tracks, &overrides);
        eprintln!(
            "library scan: saving {} tracks, {} albums, {} artists to database",
            library.tracks.len(),
            library.albums.len(),
            library.artists.len()
        );
        self.database.save_library(&library)?;
        eprintln!("library scan: database save complete");
        self.library.store(Arc::new(library));
        eprintln!("library scan: in-memory library index updated");
        Ok(())
    }

    pub(crate) fn refresh_album_artwork_overrides(&self) -> io::Result<()> {
        let current = self.library_snapshot();
        let tracks: Vec<LibraryTrack> = current.tracks.iter().cloned().collect();
        let overrides = self.database.list_album_artwork_overrides()?;
        let library = Library::build(current.scan_root.clone(), tracks, &overrides);
        self.library.store(Arc::new(library));
        Ok(())
    }

    pub(crate) fn renderer_backend(
        &self,
        renderer_location: &str,
    ) -> io::Result<&dyn RendererBackend> {
        self.renderer_backends
            .backend_for_location(renderer_location)
    }

    pub(crate) fn resolve_renderer(&self, renderer_location: &str) -> io::Result<RendererRecord> {
        let cached = self.database.load_renderer(renderer_location)?;
        let renderer = match self
            .renderer_backend(renderer_location)?
            .resolve_renderer(cached.as_ref(), renderer_location)
        {
            Ok(renderer) => renderer,
            Err(error) => {
                let _ = self.mark_renderer_unreachable(renderer_location, &error);
                return Err(error);
            }
        };
        if cached.as_ref() != Some(&renderer) {
            let _ = self.remember_renderer_record(&renderer);
        }
        Ok(renderer)
    }
}
