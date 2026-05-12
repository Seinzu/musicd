use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use lofty::file::{AudioFile, TaggedFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::read_from_path;
use lofty::tag::{Accessor, ItemKey, Tag};
use rayon::prelude::*;

use crate::artwork::{EmbeddedPicture, resolve_track_artwork};
use crate::ids::{stable_album_id_from_folder, stable_album_id_from_release, stable_track_id};
use crate::types::{LibraryTrack, ParsedTrackTags, TrackArtwork, TrackMetadata};
use crate::util::{
    component_to_string, infer_artist_and_album, infer_disc_and_track_numbers, infer_mime_type,
    inferred_title, is_supported_audio_file, looks_like_disc_folder, should_skip_entry,
};

use super::sort::compare_library_tracks;

type AlbumArtworkCache = Arc<Mutex<HashMap<String, Arc<OnceLock<Option<TrackArtwork>>>>>>;

struct AudioFileEntry {
    path: PathBuf,
    file_size: u64,
}

#[derive(Default)]
struct ScanProgress {
    visited_dirs: usize,
    audio_files: usize,
    skipped_entries: usize,
}

/// Progress event emitted during library scan (for SSE streaming)
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanProgressEvent {
    pub stage: String,
    pub current: usize,
    pub total: Option<usize>,
    pub percent: Option<u8>,
    pub message: Option<String>,
}

pub(crate) fn scan_library(root: &Path, config_path: &Path) -> io::Result<Vec<LibraryTrack>> {
    if !root.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("library path does not exist: {}", root.display()),
        ));
    }

    let artwork_cache_dir = config_path.join("artwork");
    fs::create_dir_all(&artwork_cache_dir)?;

    let mut audio_files = Vec::new();
    let mut progress = ScanProgress::default();
    collect_audio_files(root, &mut audio_files, &mut progress)?;
    eprintln!(
        "library scan: discovered {} audio files under {} (visited {} directories, skipped {} entries)",
        audio_files.len(),
        root.display(),
        progress.visited_dirs,
        progress.skipped_entries,
    );

    let artwork_cache: AlbumArtworkCache = Arc::new(Mutex::new(HashMap::new()));
    eprintln!(
        "library scan: extracting metadata for {} audio files",
        audio_files.len()
    );
    let processed_files = AtomicUsize::new(0);

    let mut tracks: Vec<LibraryTrack> = audio_files
        .par_iter()
        .filter_map(|file| {
            let track = build_library_track(root, file, &artwork_cache_dir, &artwork_cache);
            let processed = processed_files.fetch_add(1, Ordering::Relaxed) + 1;
            if processed == 1 || processed % 250 == 0 || processed == audio_files.len() {
                eprintln!(
                    "library scan: metadata extracted for {}/{} files",
                    processed,
                    audio_files.len()
                );
            }
            track
        })
        .collect();
    eprintln!(
        "library scan: extracted metadata for {} playable tracks",
        tracks.len()
    );

    tracks.sort_by(compare_library_tracks);
    Ok(tracks)
}

fn collect_audio_files(
    root: &Path,
    output: &mut Vec<AudioFileEntry>,
    progress: &mut ScanProgress,
) -> io::Result<()> {
    walk_dir(root, output, progress)
}

fn collect_audio_files_with_progress<F>(
    root: &Path,
    output: &mut Vec<AudioFileEntry>,
    progress: &mut ScanProgress,
    report_progress: &F,
) -> io::Result<()>
where
    F: Fn(ScanProgressEvent) -> io::Result<()> + Sync,
{
    walk_dir_with_progress(root, output, progress, report_progress)
}

fn walk_dir(
    dir: &Path,
    output: &mut Vec<AudioFileEntry>,
    progress: &mut ScanProgress,
) -> io::Result<()> {
    progress.visited_dirs += 1;
    if progress.visited_dirs == 1 || progress.visited_dirs % 250 == 0 {
        eprintln!(
            "library scan: walking directories visited={} audio_files={}",
            progress.visited_dirs, progress.audio_files
        );
    }

    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata()?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if should_skip_entry(&file_name) {
            progress.skipped_entries += 1;
            continue;
        }

        if metadata.is_dir() {
            walk_dir(&path, output, progress)?;
            continue;
        }

        if !is_supported_audio_file(&path) {
            continue;
        }

        progress.audio_files += 1;
        if progress.audio_files % 1000 == 0 {
            eprintln!(
                "library scan: found {} audio files so far",
                progress.audio_files
            );
        }
        output.push(AudioFileEntry {
            path,
            file_size: metadata.len(),
        });
    }
    Ok(())
}

fn walk_dir_with_progress<F>(
    dir: &Path,
    output: &mut Vec<AudioFileEntry>,
    progress: &mut ScanProgress,
    report_progress: &F,
) -> io::Result<()>
where
    F: Fn(ScanProgressEvent) -> io::Result<()> + Sync,
{
    progress.visited_dirs += 1;
    if progress.visited_dirs == 1 || progress.visited_dirs % 50 == 0 {
        eprintln!(
            "library scan: walking directories visited={} audio_files={}",
            progress.visited_dirs, progress.audio_files
        );
        report_discovery_progress(progress, report_progress)?;
    }

    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata()?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if should_skip_entry(&file_name) {
            progress.skipped_entries += 1;
            continue;
        }

        if metadata.is_dir() {
            walk_dir_with_progress(&path, output, progress, report_progress)?;
            continue;
        }

        if !is_supported_audio_file(&path) {
            continue;
        }

        progress.audio_files += 1;
        if progress.audio_files == 1 || progress.audio_files % 250 == 0 {
            eprintln!(
                "library scan: found {} audio files so far",
                progress.audio_files
            );
            report_discovery_progress(progress, report_progress)?;
        }
        output.push(AudioFileEntry {
            path,
            file_size: metadata.len(),
        });
    }
    Ok(())
}

fn report_discovery_progress<F>(progress: &ScanProgress, report_progress: &F) -> io::Result<()>
where
    F: Fn(ScanProgressEvent) -> io::Result<()> + Sync,
{
    report_progress(ScanProgressEvent {
        stage: "discovering".to_string(),
        current: progress.audio_files,
        total: None,
        percent: Some(discovery_percent(progress)),
        message: Some(format!(
            "Visited {} directories and found {} audio files",
            progress.visited_dirs, progress.audio_files
        )),
    })
}

fn discovery_percent(progress: &ScanProgress) -> u8 {
    let directory_score = (progress.visited_dirs as f64 / (progress.visited_dirs as f64 + 100.0))
        * 12.0;
    let file_score = (progress.audio_files as f64 / (progress.audio_files as f64 + 1_000.0)) * 8.0;
    (directory_score + file_score).clamp(1.0, 19.0).round() as u8
}

fn build_library_track(
    root: &Path,
    file: &AudioFileEntry,
    artwork_cache_dir: &Path,
    artwork_cache: &AlbumArtworkCache,
) -> Option<LibraryTrack> {
    let path = &file.path;
    let relative_components = path
        .strip_prefix(root)
        .unwrap_or(path)
        .components()
        .filter_map(component_to_string)
        .collect::<Vec<_>>();
    let relative_path = relative_components.join("/");

    let (parsed_tags, embedded_picture) = read_track_metadata(path);

    let title = parsed_tags
        .title
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| inferred_title(path));
    let (fallback_artist, fallback_album) = infer_artist_and_album(&relative_components);
    let artist = parsed_tags
        .artist
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_artist);
    let album = parsed_tags
        .album
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_album);
    let (fallback_disc_number, fallback_track_number) =
        infer_disc_and_track_numbers(&relative_components);
    let disc_number = parsed_tags.disc_number.or(fallback_disc_number);
    let track_number = parsed_tags.track_number.or(fallback_track_number);
    let mime_type = infer_mime_type(path).to_string();
    let id = stable_track_id(&relative_path);

    let album_artist = parsed_tags
        .album_artist
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            if parsed_tags.compilation.unwrap_or(false) {
                Some("Various Artists".to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| artist.clone());

    let album_id = parsed_tags
        .metadata
        .musicbrainz_release_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(stable_album_id_from_release)
        .unwrap_or_else(|| {
            let parent_is_disc = {
                let len = relative_components.len();
                len >= 2
                    && relative_components
                        .get(len - 2)
                        .map(|value| looks_like_disc_folder(value))
                        .unwrap_or(false)
            };
            let album_folder = if parent_is_disc && relative_components.len() >= 3 {
                relative_components
                    .get(relative_components.len() - 3)
                    .cloned()
            } else {
                relative_components
                    .len()
                    .checked_sub(2)
                    .and_then(|idx| relative_components.get(idx).cloned())
            };
            let folder = album_folder.unwrap_or_else(|| album.clone());

            stable_album_id_from_folder(&folder)
        });

    let artwork = resolve_album_artwork(artwork_cache, &album_id, || {
        resolve_track_artwork(
            root,
            path,
            &relative_components,
            &album_id,
            embedded_picture,
            artwork_cache_dir,
        )
    });

    Some(LibraryTrack {
        id,
        album_id,
        title,
        artist,
        album,
        album_artist,
        disc_number,
        track_number,
        duration_seconds: parsed_tags.duration_seconds,
        relative_path,
        path: path.clone(),
        mime_type,
        file_size: file.file_size,
        artwork,
        metadata: parsed_tags.metadata,
    })
}

/// Resolve artwork for an album, deduping concurrent requests so each album's
/// extraction (and disk write) runs at most once per scan. Workers that arrive
/// after the first one block on `OnceLock::get_or_init`.
fn resolve_album_artwork(
    cache: &AlbumArtworkCache,
    album_id: &str,
    resolver: impl FnOnce() -> Option<TrackArtwork>,
) -> Option<TrackArtwork> {
    let cell = {
        let mut guard = cache.lock().expect("artwork cache poisoned");
        Arc::clone(guard.entry(album_id.to_string()).or_default())
    };
    cell.get_or_init(resolver).clone()
}

fn read_track_metadata(path: &Path) -> (ParsedTrackTags, Option<EmbeddedPicture>) {
    let tagged_file = match read_from_path(path) {
        Ok(file) => file,
        Err(_) => return (ParsedTrackTags::default(), None),
    };
    let tags = extract_tags(&tagged_file);
    let picture = extract_embedded_picture(&tagged_file);
    (tags, picture)
}

fn extract_tags(tagged_file: &TaggedFile) -> ParsedTrackTags {
    let Some(tag) = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
    else {
        return ParsedTrackTags::default();
    };

    ParsedTrackTags {
        title: tag.title().map(|value| value.into_owned()),
        artist: tag.artist().map(|value| value.into_owned()),
        album: tag.album().map(|value| value.into_owned()),
        album_artist: tag_text(tag, ItemKey::AlbumArtist),
        compilation: tag_text(tag, ItemKey::FlagCompilation)
            .map(|value| value == "1"),
        disc_number: tag.disk(),
        track_number: tag.track(),
        duration_seconds: {
            let seconds = tagged_file.properties().duration().as_secs();
            if seconds == 0 { None } else { Some(seconds) }
        },
        metadata: extract_track_metadata(tag),
    }
}

fn extract_track_metadata(tag: &Tag) -> TrackMetadata {
    TrackMetadata {
        musicbrainz_release_id: tag_text(tag, ItemKey::MusicBrainzReleaseId),
        musicbrainz_release_group_id: tag_text(tag, ItemKey::MusicBrainzReleaseGroupId),
        musicbrainz_recording_id: tag_text(tag, ItemKey::MusicBrainzRecordingId),
        musicbrainz_release_track_id: tag_text(tag, ItemKey::MusicBrainzTrackId),
        release_date: tag_text(tag, ItemKey::ReleaseDate)
            .or_else(|| tag_text(tag, ItemKey::RecordingDate)),
        original_release_date: tag_text(tag, ItemKey::OriginalReleaseDate),
        release_country: tag_text(tag, ItemKey::ReleaseCountry),
        release_type: tag_text(tag, ItemKey::MusicBrainzReleaseType),
        genres: tag_values(tag, ItemKey::Genre),
    }
}

fn tag_text(tag: &Tag, key: ItemKey) -> Option<String> {
    tag.get_string(key)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn tag_values(tag: &Tag, key: ItemKey) -> Vec<String> {
    let mut values = Vec::new();
    for value in tag.get_strings(key) {
        for part in value.split([';', ',']) {
            let part = part.trim();
            if !part.is_empty() && !values.iter().any(|existing| existing == part) {
                values.push(part.to_string());
            }
        }
    }
    values
}

fn extract_embedded_picture(tagged_file: &TaggedFile) -> Option<EmbeddedPicture> {
    let (picture, tag_label) = tagged_file
        .tags()
        .iter()
        .find_map(|tag| {
            tag.get_picture_type(PictureType::CoverFront)
                .map(|picture| (picture, format!("{:?}", tag.tag_type())))
        })
        .or_else(|| {
            tagged_file.tags().iter().find_map(|tag| {
                tag.pictures()
                    .first()
                    .map(|picture| (picture, format!("{:?}", tag.tag_type())))
            })
        })
        .or_else(|| {
            tagged_file
                .primary_tag()
                .or_else(|| tagged_file.first_tag())
                .and_then(|tag| {
                    tag.get_picture_type(PictureType::CoverFront)
                        .or_else(|| tag.pictures().first())
                        .map(|picture| (picture, format!("{:?}", tag.tag_type())))
                })
        })?;
    let mime_type = picture
        .mime_type()
        .map(|value| value.as_str().to_string())
        .or_else(|| {
            crate::artwork::infer_image_mime_from_bytes(picture.data()).map(ToString::to_string)
        })?;

    Some(EmbeddedPicture {
        bytes: picture.data().to_vec(),
        mime_type,
        pic_type: format!("{:?}", picture.pic_type()),
        tag_label,
    })
}

/// Perform a library scan and report coarse progress as the same scan work runs.
pub(crate) fn scan_library_with_progress<F>(
    root: &Path,
    config_path: &Path,
    report_progress: F,
) -> io::Result<Vec<LibraryTrack>>
where
    F: Fn(ScanProgressEvent) -> io::Result<()> + Sync,
{
    if !root.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("library path does not exist: {}", root.display()),
        ));
    }

    let artwork_cache_dir = config_path.join("artwork");
    fs::create_dir_all(&artwork_cache_dir)?;

    let mut audio_files = Vec::new();
    let mut progress = ScanProgress::default();
    collect_audio_files_with_progress(root, &mut audio_files, &mut progress, &report_progress)?;

    report_progress(ScanProgressEvent {
        stage: "discovering".to_string(),
        current: audio_files.len(),
        total: None,
        percent: Some(20),
        message: Some(format!(
            "library scan: discovered {} audio files",
            audio_files.len()
        )),
    })?;

    eprintln!(
        "library scan: discovered {} audio files under {} (visited {} directories, skipped {} entries)",
        audio_files.len(),
        root.display(),
        progress.visited_dirs,
        progress.skipped_entries,
    );

    let artwork_cache: AlbumArtworkCache = Arc::new(Mutex::new(HashMap::new()));

    report_progress(ScanProgressEvent {
        stage: "extracting_metadata".to_string(),
        current: 0,
        total: Some(audio_files.len()),
        percent: Some(20),
        message: None,
    })?;

    eprintln!(
        "library scan: extracting metadata for {} audio files",
        audio_files.len()
    );

    let processed_files = AtomicUsize::new(0);
    let progress_interval = progress_report_interval(audio_files.len());
    let progress_failed = AtomicBool::new(false);
    let progress_error = Mutex::new(None);

    let mut tracks: Vec<LibraryTrack> = audio_files
        .par_iter()
        .filter_map(|file| {
            if progress_failed.load(Ordering::Relaxed) {
                return None;
            }
            let track = build_library_track(root, file, &artwork_cache_dir, &artwork_cache);
            let processed = processed_files.fetch_add(1, Ordering::Relaxed) + 1;

            if processed == 1 || processed % progress_interval == 0 || processed == audio_files.len()
            {
                let event = ScanProgressEvent {
                    stage: "extracting_metadata".to_string(),
                    current: processed,
                    total: Some(audio_files.len()),
                    percent: Some(metadata_percent(processed, audio_files.len())),
                    message: None,
                };
                if let Err(error) = report_progress(event) {
                    progress_failed.store(true, Ordering::Relaxed);
                    let mut guard = progress_error.lock().expect("progress error lock poisoned");
                    if guard.is_none() {
                        *guard = Some(error);
                    }
                }
                eprintln!(
                    "library scan: metadata extracted for {}/{} files",
                    processed,
                    audio_files.len()
                );
            }
            track
        })
        .collect();

    if let Some(error) = progress_error
        .into_inner()
        .expect("progress error lock poisoned")
    {
        return Err(error);
    }

    report_progress(ScanProgressEvent {
        stage: "building_index".to_string(),
        current: tracks.len(),
        total: None,
        percent: Some(90),
        message: Some(format!(
            "library scan: extracted metadata for {} playable tracks",
            tracks.len()
        )),
    })?;

    eprintln!(
        "library scan: extracted metadata for {} playable tracks",
        tracks.len()
    );

    tracks.sort_by(compare_library_tracks);
    report_progress(ScanProgressEvent {
        stage: "building_index".to_string(),
        current: tracks.len(),
        total: None,
        percent: Some(93),
        message: Some("library scan: sorted tracks".to_string()),
    })?;
    Ok(tracks)
}

fn progress_report_interval(total: usize) -> usize {
    (total / 100).max(1)
}

fn metadata_percent(processed: usize, total: usize) -> u8 {
    if total == 0 {
        return 88;
    }
    let progress = processed as f64 / total as f64;
    (20.0 + progress * 68.0).clamp(20.0, 88.0).round() as u8
}
