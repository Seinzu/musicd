use std::fs;
use std::path::{Path, PathBuf};

use crate::ids::stable_track_id;
use crate::util::looks_like_disc_folder;

use super::mime::{image_extension_for_mime, infer_image_mime_from_path};
use super::{ArtworkCandidate, ArtworkData};

pub(super) fn find_sidecar_artwork(
    root: &Path,
    track_path: &Path,
    relative_components: &[String],
) -> Option<ArtworkCandidate> {
    let search_dirs = artwork_search_dirs(track_path, relative_components);
    for directory in search_dirs {
        let mut entries = fs::read_dir(&directory)
            .ok()?
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        entries.sort_by_key(|entry| entry.path());
        let mut best_match: Option<(usize, PathBuf, String)> = None;

        for entry in entries {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                continue;
            }

            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy().to_string();
            let Some(priority) = artwork_name_priority(&file_name) else {
                continue;
            };
            let should_replace = best_match
                .as_ref()
                .map(|(best_priority, best_path, _)| {
                    priority < *best_priority || (priority == *best_priority && path < *best_path)
                })
                .unwrap_or(true);
            if should_replace {
                best_match = Some((priority, path, file_name));
            }
        }

        if let Some((priority, path, _)) = best_match {
            let mime_type = infer_image_mime_from_path(&path)?;
            let extension = image_extension_for_mime(mime_type)?;
            let relative_source = path
                .strip_prefix(root)
                .ok()
                .map(|value| value.display().to_string())
                .unwrap_or_else(|| path.display().to_string());
            return Some(ArtworkCandidate {
                cache_key: stable_track_id(&format!("sidecar:{relative_source}:{priority}")),
                source: format!("Sidecar file: {relative_source}"),
                mime_type: mime_type.to_string(),
                extension,
                data: ArtworkData::File(path),
            });
        }
    }

    None
}

fn artwork_search_dirs(track_path: &Path, relative_components: &[String]) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Some(directory) = track_path.parent() {
        directories.push(directory.to_path_buf());
        if relative_components.len() > 2 {
            let parent_name = relative_components
                .get(relative_components.len().saturating_sub(2))
                .map(String::as_str)
                .unwrap_or_default();
            if looks_like_disc_folder(parent_name) {
                if let Some(parent) = directory.parent() {
                    if parent != directory {
                        directories.push(parent.to_path_buf());
                    }
                }
            }
        }
    }
    directories
}

pub(crate) fn artwork_name_priority(file_name: &str) -> Option<usize> {
    let normalized = file_name.trim().to_ascii_lowercase();
    let stem = Path::new(&normalized)
        .file_stem()
        .and_then(|value| value.to_str())?;

    let stem_priority = match stem {
        "cover" => 0,
        "folder" => 1,
        "front" => 2,
        "album" => 3,
        "artwork" => 4,
        _ => return None,
    };

    let extension_priority = match Path::new(&normalized)
        .extension()
        .and_then(|value| value.to_str())?
    {
        "jpg" => 0,
        "jpeg" => 1,
        "png" => 2,
        "webp" => 3,
        _ => return None,
    };

    Some((stem_priority * 10) + extension_priority)
}
