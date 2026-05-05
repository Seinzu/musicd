use std::fs;
use std::io;
use std::path::Path;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::read_from_path;
use lofty::tag::Accessor;

use crate::artwork::resolve_track_artwork;
use crate::ids::{stable_album_id, stable_track_id};
use crate::types::{LibraryTrack, ParsedTrackTags};
use crate::util::{
    component_to_string, infer_artist_and_album, infer_disc_and_track_numbers, infer_mime_type,
    is_supported_audio_file, should_skip_entry,
};

use super::sort::compare_library_tracks;

pub(crate) fn scan_library(root: &Path, config_path: &Path) -> io::Result<Vec<LibraryTrack>> {
    if !root.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("library path does not exist: {}", root.display()),
        ));
    }

    let artwork_cache_dir = config_path.join("artwork");
    fs::create_dir_all(&artwork_cache_dir)?;
    let mut tracks = Vec::new();
    scan_dir(root, root, &artwork_cache_dir, &mut tracks)?;
    tracks.sort_by(compare_library_tracks);
    Ok(tracks)
}

fn scan_dir(
    root: &Path,
    dir: &Path,
    artwork_cache_dir: &Path,
    tracks: &mut Vec<LibraryTrack>,
) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata()?;
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if should_skip_entry(&file_name) {
            continue;
        }

        if metadata.is_dir() {
            scan_dir(root, &path, artwork_cache_dir, tracks)?;
            continue;
        }

        if !is_supported_audio_file(&path) {
            continue;
        }

        let relative_components = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .components()
            .filter_map(component_to_string)
            .collect::<Vec<_>>();
        let relative_path = relative_components.join("/");
        let parsed_tags = read_lofty_track_tags(&path);
        let title = parsed_tags
            .title
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| crate::inferred_title(&path));
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
        let mime_type = infer_mime_type(&path).to_string();
        let id = stable_track_id(&relative_path);
        let album_id = stable_album_id(&artist, &album);
        let artwork =
            resolve_track_artwork(root, &path, &relative_components, &id, artwork_cache_dir);

        tracks.push(LibraryTrack {
            id,
            album_id,
            title,
            artist,
            album,
            disc_number,
            track_number,
            duration_seconds: parsed_tags.duration_seconds,
            relative_path,
            path,
            mime_type,
            file_size: metadata.len(),
            artwork,
        });
    }

    Ok(())
}

fn read_lofty_track_tags(path: &Path) -> ParsedTrackTags {
    let tagged_file = match read_from_path(path) {
        Ok(tagged_file) => tagged_file,
        Err(_) => return ParsedTrackTags::default(),
    };

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
        disc_number: tag.disk(),
        track_number: tag.track(),
        duration_seconds: {
            let seconds = tagged_file.properties().duration().as_secs();
            if seconds == 0 { None } else { Some(seconds) }
        },
    }
}
