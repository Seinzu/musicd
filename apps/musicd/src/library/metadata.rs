use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::read_from_path;
use lofty::tag::Accessor;

use crate::types::EmbeddedMetadata;
use crate::util::file_extension;

pub(crate) fn inspect_embedded_metadata(path: &Path) -> io::Result<EmbeddedMetadata> {
    if let Ok(metadata) = inspect_with_lofty(path) {
        return Ok(metadata);
    }

    match file_extension(path).as_deref() {
        Some("flac") => inspect_flac_metadata(path),
        Some("mp3") => inspect_mp3_metadata(path),
        Some("m4a" | "alac" | "aac") => Ok(EmbeddedMetadata {
            format_name: "MP4-family file".to_string(),
            fields: Vec::new(),
            notes: vec!["Embedded tag parsing for this format is not implemented yet.".to_string()],
        }),
        Some("ogg") => Ok(EmbeddedMetadata {
            format_name: "Ogg container".to_string(),
            fields: Vec::new(),
            notes: vec!["Embedded tag parsing for Ogg/Vorbis is not implemented yet.".to_string()],
        }),
        Some("wav" | "aiff" | "aif" | "dsf") => Ok(EmbeddedMetadata {
            format_name: "Audio file".to_string(),
            fields: Vec::new(),
            notes: vec![
                "No embedded metadata parser is implemented for this format yet.".to_string(),
            ],
        }),
        _ => Ok(EmbeddedMetadata {
            format_name: "Unknown".to_string(),
            fields: Vec::new(),
            notes: vec!["Unknown file type.".to_string()],
        }),
    }
}

fn inspect_with_lofty(path: &Path) -> io::Result<EmbeddedMetadata> {
    let tagged_file = read_from_path(path).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("lofty failed to read tags: {error}"),
        )
    })?;

    let mut fields = Vec::new();
    let mut notes = Vec::new();
    let tag_types = tagged_file
        .tags()
        .iter()
        .map(|tag| format!("{:?}", tag.tag_type()))
        .collect::<Vec<_>>();
    notes.push(format!("Lofty file type: {:?}", tagged_file.file_type()));
    if tag_types.is_empty() {
        notes.push("Lofty did not find any readable tags in this file.".to_string());
    } else {
        notes.push(format!("Readable tag types: {}", tag_types.join(", ")));
    }

    if let Some(tag) = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag())
    {
        fields.push(("TAG_TYPE".to_string(), format!("{:?}", tag.tag_type())));
        push_optional_field(
            &mut fields,
            "TITLE",
            tag.title().map(|value| value.into_owned()),
        );
        push_optional_field(
            &mut fields,
            "ARTIST",
            tag.artist().map(|value| value.into_owned()),
        );
        push_optional_field(
            &mut fields,
            "ALBUM",
            tag.album().map(|value| value.into_owned()),
        );
        push_optional_field(
            &mut fields,
            "GENRE",
            tag.genre().map(|value| value.into_owned()),
        );
        push_optional_field(
            &mut fields,
            "TRACKNUMBER",
            tag.track().map(|value| value.to_string()),
        );
        push_optional_field(
            &mut fields,
            "TRACKTOTAL",
            tag.track_total().map(|value| value.to_string()),
        );
        push_optional_field(
            &mut fields,
            "DISCNUMBER",
            tag.disk().map(|value| value.to_string()),
        );
        push_optional_field(
            &mut fields,
            "DISCTOTAL",
            tag.disk_total().map(|value| value.to_string()),
        );
        push_optional_field(
            &mut fields,
            "COMMENT",
            tag.comment().map(|value| value.into_owned()),
        );
    }

    let properties = tagged_file.properties();
    push_optional_field(
        &mut fields,
        "DURATION_SECONDS",
        Some(properties.duration().as_secs().to_string()),
    );
    push_optional_field(
        &mut fields,
        "CHANNELS",
        properties.channels().map(|value| value.to_string()),
    );
    push_optional_field(
        &mut fields,
        "SAMPLE_RATE",
        properties.sample_rate().map(|value| value.to_string()),
    );
    push_optional_field(
        &mut fields,
        "AUDIO_BITRATE_KBPS",
        properties.audio_bitrate().map(|value| value.to_string()),
    );
    push_optional_field(
        &mut fields,
        "OVERALL_BITRATE_KBPS",
        properties.overall_bitrate().map(|value| value.to_string()),
    );
    push_optional_field(
        &mut fields,
        "BIT_DEPTH",
        properties.bit_depth().map(|value| value.to_string()),
    );

    Ok(EmbeddedMetadata {
        format_name: "Lofty parsed metadata".to_string(),
        fields,
        notes,
    })
}

fn push_optional_field(fields: &mut Vec<(String, String)>, key: &str, value: Option<String>) {
    if let Some(value) = value {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            fields.push((key.to_string(), trimmed.to_string()));
        }
    }
}

fn inspect_flac_metadata(path: &Path) -> io::Result<EmbeddedMetadata> {
    let mut file = File::open(path)?;
    let mut signature = [0_u8; 4];
    file.read_exact(&mut signature)?;
    if &signature != b"fLaC" {
        return Ok(EmbeddedMetadata {
            format_name: "FLAC".to_string(),
            fields: Vec::new(),
            notes: vec!["File does not begin with the FLAC signature.".to_string()],
        });
    }

    let mut notes = Vec::new();
    let mut fields = Vec::new();
    loop {
        let mut header = [0_u8; 4];
        if file.read_exact(&mut header).is_err() {
            break;
        }
        let is_last = header[0] & 0x80 != 0;
        let block_type = header[0] & 0x7f;
        let block_len =
            ((header[1] as usize) << 16) | ((header[2] as usize) << 8) | header[3] as usize;
        let mut block = vec![0_u8; block_len];
        file.read_exact(&mut block)?;

        if block_type == 4 {
            let (parsed_fields, parsed_notes) = parse_vorbis_comment_block(&block);
            fields.extend(parsed_fields);
            notes.extend(parsed_notes);
        }

        if is_last {
            break;
        }
    }

    if fields.is_empty() {
        notes.push("No Vorbis comment fields were parsed from this FLAC file.".to_string());
    }

    Ok(EmbeddedMetadata {
        format_name: "FLAC Vorbis comments".to_string(),
        fields,
        notes,
    })
}

pub(crate) fn parse_vorbis_comment_block(block: &[u8]) -> (Vec<(String, String)>, Vec<String>) {
    let mut offset = 0;
    let mut notes = Vec::new();
    let mut fields = Vec::new();

    let Some(vendor_len) = read_le_u32(block, &mut offset) else {
        return (
            fields,
            vec!["Vorbis comment block ended before vendor length.".to_string()],
        );
    };
    if offset + vendor_len as usize > block.len() {
        return (
            fields,
            vec!["Vorbis comment vendor string length was invalid.".to_string()],
        );
    }
    let vendor = String::from_utf8_lossy(&block[offset..offset + vendor_len as usize]).to_string();
    offset += vendor_len as usize;
    fields.push(("VENDOR".to_string(), vendor));

    let Some(comment_count) = read_le_u32(block, &mut offset) else {
        notes.push("Vorbis comment block ended before the comment count.".to_string());
        return (fields, notes);
    };

    for _ in 0..comment_count {
        let Some(comment_len) = read_le_u32(block, &mut offset) else {
            notes.push("Vorbis comment block ended before a comment length.".to_string());
            break;
        };
        let comment_len = comment_len as usize;
        if offset + comment_len > block.len() {
            notes.push("Vorbis comment block contained an invalid comment length.".to_string());
            break;
        }
        let comment = String::from_utf8_lossy(&block[offset..offset + comment_len]).to_string();
        offset += comment_len;
        if let Some((key, value)) = comment.split_once('=') {
            fields.push((key.to_ascii_uppercase(), value.to_string()));
        } else {
            notes.push(format!("Unstructured Vorbis comment: {comment}"));
        }
    }

    (fields, notes)
}

fn inspect_mp3_metadata(path: &Path) -> io::Result<EmbeddedMetadata> {
    let mut file = File::open(path)?;
    let mut notes = Vec::new();
    let mut fields = Vec::new();

    let mut header = [0_u8; 10];
    let read = file.read(&mut header)?;
    if read >= 10 && &header[..3] == b"ID3" {
        let size = decode_synchsafe_u32(&header[6..10]);
        notes.push(format!(
            "ID3v2.{}.{} tag detected at file start ({} bytes before audio frames).",
            header[3], header[4], size
        ));
    } else {
        notes.push("No ID3v2 header detected at the start of the file.".to_string());
    }

    let file_len = file.metadata()?.len();
    if file_len >= 128 {
        file.seek(SeekFrom::End(-128))?;
        let mut trailer = [0_u8; 128];
        file.read_exact(&mut trailer)?;
        if &trailer[..3] == b"TAG" {
            fields.push(("TITLE".to_string(), decode_id3v1_text(&trailer[3..33])));
            fields.push(("ARTIST".to_string(), decode_id3v1_text(&trailer[33..63])));
            fields.push(("ALBUM".to_string(), decode_id3v1_text(&trailer[63..93])));
            fields.push(("YEAR".to_string(), decode_id3v1_text(&trailer[93..97])));
            let comment = decode_id3v1_text(&trailer[97..127]);
            if !comment.is_empty() {
                fields.push(("COMMENT".to_string(), comment));
            }
            if trailer[125] == 0 && trailer[126] != 0 {
                fields.push(("TRACKNUMBER".to_string(), trailer[126].to_string()));
            }
        } else {
            notes.push("No ID3v1 trailer detected at the end of the file.".to_string());
        }
    }

    Ok(EmbeddedMetadata {
        format_name: "MP3 tags".to_string(),
        fields,
        notes,
    })
}

fn read_le_u32(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    if *offset + 4 > bytes.len() {
        return None;
    }
    let value = u32::from_le_bytes([
        bytes[*offset],
        bytes[*offset + 1],
        bytes[*offset + 2],
        bytes[*offset + 3],
    ]);
    *offset += 4;
    Some(value)
}

fn decode_synchsafe_u32(bytes: &[u8]) -> u32 {
    ((bytes[0] as u32) << 21)
        | ((bytes[1] as u32) << 14)
        | ((bytes[2] as u32) << 7)
        | (bytes[3] as u32)
}

pub(crate) fn decode_id3v1_text(bytes: &[u8]) -> String {
    let trimmed = bytes
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&trimmed).trim().to_string()
}
