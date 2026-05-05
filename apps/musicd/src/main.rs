mod artwork;
mod cli;
mod db;
mod handlers;
mod http;
mod ids;
mod library;
mod metrics;
mod renderer;
mod service;
mod types;
mod util;
mod views;

fn main() {
    if let Err(error) = cli::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use crate::artwork::{artwork_name_priority, infer_image_mime_from_bytes};
    use crate::db::Database;
    use crate::http::{parse_query_string, parse_range_header, parse_request_form};
    use crate::ids::{stable_album_id, stable_artist_id, stable_track_id};
    use crate::library::{
        Library, build_artist_summaries, compare_track_album_order, decode_id3v1_text,
        parse_vorbis_comment_block,
    };
    use crate::renderer::{
        RendererBackends, RendererKind, renderer_group_queue_key, renderer_is_viable,
        renderer_kind_for_location, renderer_needs_refresh,
    };
    use crate::service::{
        ServiceState, next_queue_entry_after, previous_queue_entry_before,
        queue_status_for_transport, should_adopt_preloaded_next_entry, should_auto_advance,
    };
    use crate::types::{
        LibraryTrack, PlaybackQueue, PlaybackSession, QueueEntry, QueueMutationEntry,
        RendererRecord, TrackArtwork,
    };
    use crate::util::{
        cleanup_track_label, infer_artist_and_album, infer_disc_and_track_numbers,
        should_skip_entry,
    };
    use musicd_core::AppConfig;
    use musicd_upnp::{PositionInfo, RendererCapabilities, TransportInfo, TransportSnapshot};
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_standard_http_ranges() {
        assert_eq!(parse_range_header("bytes=100-199", 1000), Some((100, 199)));
        assert_eq!(parse_range_header("bytes=100-", 1000), Some((100, 999)));
        assert_eq!(parse_range_header("bytes=-100", 1000), Some((900, 999)));
    }

    #[test]
    fn rejects_invalid_ranges() {
        assert_eq!(parse_range_header("items=100-200", 1000), None);
        assert_eq!(parse_range_header("bytes=500-200", 1000), None);
        assert_eq!(parse_range_header("bytes=100-2000", 1000), None);
    }

    #[test]
    fn query_parser_decodes_renderer_locations() {
        let parsed = parse_query_string(
            "renderer_location=http%3A%2F%2F192.168.1.55%3A49152%2Fdescription.xml&message=Now+playing",
        );
        assert_eq!(
            parsed.get("renderer_location").map(String::as_str),
            Some("http://192.168.1.55:49152/description.xml")
        );
        assert_eq!(
            parsed.get("message").map(String::as_str),
            Some("Now playing")
        );
    }

    #[test]
    fn form_parser_decodes_urlencoded_bodies() {
        let parsed = parse_request_form(
            Some("application/x-www-form-urlencoded; charset=utf-8"),
            b"renderer_location=http%3A%2F%2F192.168.1.55%3A49152%2Fdescription.xml&track_id=abc123",
        );
        assert_eq!(
            parsed.get("renderer_location").map(String::as_str),
            Some("http://192.168.1.55:49152/description.xml")
        );
        assert_eq!(parsed.get("track_id").map(String::as_str), Some("abc123"));
    }

    #[test]
    fn infers_renderer_kind_from_location() {
        assert_eq!(
            renderer_kind_for_location("http://192.168.1.55:49152/description.xml"),
            RendererKind::Upnp
        );
        assert_eq!(
            renderer_kind_for_location("sonos:RINCON_1234567890"),
            RendererKind::Sonos
        );
        assert_eq!(
            renderer_kind_for_location("group:abc123"),
            RendererKind::Group
        );
    }

    #[test]
    fn stable_track_ids_are_repeatable() {
        let left = stable_track_id("Artist/Album/01 - Track.flac");
        let right = stable_track_id("Artist/Album/01 - Track.flac");
        assert_eq!(left, right);
    }

    #[test]
    fn cleanup_track_label_strips_common_number_prefixes() {
        assert_eq!(cleanup_track_label("01 - Example_Track"), "Example Track");
        assert_eq!(cleanup_track_label("1. Intro"), "Intro");
    }

    #[test]
    fn infers_artist_and_album_from_relative_components() {
        let (artist, album) = infer_artist_and_album(&[
            "Boards of Canada".to_string(),
            "Music Has the Right to Children".to_string(),
            "01 - Wildlife Analysis.flac".to_string(),
        ]);
        assert_eq!(artist, "Boards of Canada");
        assert_eq!(album, "Music Has the Right to Children");

        let (artist, album) = infer_artist_and_album(&[
            "Biosphere".to_string(),
            "Substrata".to_string(),
            "Disc 1".to_string(),
            "01 - As the Sun Kissed the Horizon.flac".to_string(),
        ]);
        assert_eq!(artist, "Biosphere");
        assert_eq!(album, "Substrata");
    }

    #[test]
    fn infers_disc_and_track_numbers_from_paths() {
        let (disc, track) = infer_disc_and_track_numbers(&[
            "Biosphere".to_string(),
            "Substrata".to_string(),
            "Disc 2".to_string(),
            "03 - Chukhung.flac".to_string(),
        ]);
        assert_eq!(disc, Some(2));
        assert_eq!(track, Some(3));

        let (disc, track) = infer_disc_and_track_numbers(&[
            "Album".to_string(),
            "Track Without Prefix.flac".to_string(),
        ]);
        assert_eq!(disc, None);
        assert_eq!(track, None);
    }

    #[test]
    fn skips_hidden_metadata_entries() {
        assert!(should_skip_entry(".AppleDouble"));
        assert!(should_skip_entry("._Track.flac"));
        assert!(should_skip_entry("@eaDir"));
        assert!(!should_skip_entry("Track.flac"));
    }

    #[test]
    fn parses_vorbis_comment_block_fields() {
        let mut block = Vec::new();
        block.extend_from_slice(&5_u32.to_le_bytes());
        block.extend_from_slice(b"music");
        block.extend_from_slice(&2_u32.to_le_bytes());

        let title = b"TITLE=Roygbiv";
        block.extend_from_slice(&(title.len() as u32).to_le_bytes());
        block.extend_from_slice(title);

        let artist = b"ARTIST=Boards of Canada";
        block.extend_from_slice(&(artist.len() as u32).to_le_bytes());
        block.extend_from_slice(artist);

        let (fields, notes) = parse_vorbis_comment_block(&block);
        assert!(notes.is_empty());
        assert!(fields.contains(&(String::from("VENDOR"), String::from("music"))));
        assert!(fields.contains(&(String::from("TITLE"), String::from("Roygbiv"))));
        assert!(fields.contains(&(String::from("ARTIST"), String::from("Boards of Canada"))));
    }

    #[test]
    fn decodes_id3v1_text() {
        let bytes = b"Example Track\x00\x00\x00";
        assert_eq!(decode_id3v1_text(bytes), "Example Track");
    }

    #[test]
    fn stable_album_ids_are_repeatable() {
        let left = stable_album_id("Boards of Canada", "Music Has the Right to Children");
        let right = stable_album_id("boards of canada", "music has the right to children");
        assert_eq!(left, right);
    }

    #[test]
    fn track_album_order_prefers_numeric_positions() {
        let mut tracks = vec![
            sample_track("c", Some(1), Some(3), "Track 3"),
            sample_track("a", Some(1), Some(1), "Track 1"),
            sample_track("b", Some(1), Some(2), "Track 2"),
        ];
        tracks.sort_by(compare_track_album_order);
        let ordered_ids = tracks.into_iter().map(|track| track.id).collect::<Vec<_>>();
        assert_eq!(ordered_ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn queue_replace_and_append_round_trip() {
        let config_path = temp_config_path("queue-round-trip");
        let database = Database::open(&config_path).expect("database should open");

        let replaced = database
            .replace_queue(
                "http://renderer.local/description.xml",
                "Album Queue",
                &[QueueMutationEntry {
                    track_id: "track-1".to_string(),
                    album_id: Some("album-1".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album-1".to_string()),
                }],
            )
            .expect("queue replace should succeed");
        assert_eq!(replaced.entries.len(), 1);
        assert_eq!(replaced.current_entry_id, Some(replaced.entries[0].id));

        let appended = database
            .append_queue_entries(
                "http://renderer.local/description.xml",
                "Album Queue",
                &[QueueMutationEntry {
                    track_id: "track-2".to_string(),
                    album_id: Some("album-1".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album-1".to_string()),
                }],
            )
            .expect("queue append should succeed");
        assert_eq!(appended.entries.len(), 2);
        assert_eq!(appended.entries[0].track_id, "track-1");
        assert_eq!(appended.entries[1].track_id, "track-2");

        let _ = std::fs::remove_dir_all(config_path);
    }

    #[test]
    fn renderer_group_copies_source_queue_on_create() {
        let state = sample_state(Vec::new());
        state
            .database
            .replace_queue(
                "http://kitchen.local/description.xml",
                "Kitchen Queue",
                &[
                    QueueMutationEntry {
                        track_id: "track-1".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                    QueueMutationEntry {
                        track_id: "track-2".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                ],
            )
            .expect("source queue should be created");

        let group = state
            .create_renderer_group(
                "Downstairs",
                &[
                    "http://kitchen.local/description.xml".to_string(),
                    "http://living-room.local/description.xml".to_string(),
                ],
                Some("http://kitchen.local/description.xml"),
            )
            .expect("group should be created");
        assert_eq!(group.name, "Downstairs");
        assert_eq!(group.members.len(), 2);

        let group_queue_key = renderer_group_queue_key(&group.id);
        let queue = state
            .database
            .load_queue(&group_queue_key)
            .expect("group queue should load")
            .expect("group queue should exist");
        assert_eq!(queue.name, "Kitchen Queue");
        assert_eq!(
            queue
                .entries
                .iter()
                .map(|entry| entry.track_id.as_str())
                .collect::<Vec<_>>(),
            vec!["track-1", "track-2"]
        );

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn queue_insert_move_and_remove_round_trip() {
        let config_path = temp_config_path("queue-mutations");
        let database = Database::open(&config_path).expect("database should open");

        let initial = database
            .replace_queue(
                "http://renderer.local/description.xml",
                "Album Queue",
                &[
                    QueueMutationEntry {
                        track_id: "track-1".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                    QueueMutationEntry {
                        track_id: "track-2".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                    QueueMutationEntry {
                        track_id: "track-3".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                ],
            )
            .expect("queue replace should succeed");

        let inserted = database
            .insert_queue_entries_after_current(
                "http://renderer.local/description.xml",
                "Album Queue",
                &[QueueMutationEntry {
                    track_id: "track-x".to_string(),
                    album_id: Some("album-2".to_string()),
                    source_kind: "track".to_string(),
                    source_ref: Some("track-x".to_string()),
                }],
            )
            .expect("queue insert should succeed");
        assert_eq!(
            inserted
                .entries
                .iter()
                .map(|entry| entry.track_id.as_str())
                .collect::<Vec<_>>(),
            vec!["track-1", "track-x", "track-2", "track-3"]
        );

        let moved = database
            .move_queue_entry(
                "http://renderer.local/description.xml",
                inserted.entries[3].id,
                -1,
            )
            .expect("queue move should succeed");
        assert_eq!(
            moved
                .entries
                .iter()
                .map(|entry| entry.track_id.as_str())
                .collect::<Vec<_>>(),
            vec!["track-1", "track-x", "track-3", "track-2"]
        );

        let removed = database
            .remove_queue_entry("http://renderer.local/description.xml", moved.entries[1].id)
            .expect("queue remove should succeed");
        assert_eq!(removed.current_entry_id, initial.current_entry_id);
        assert_eq!(
            removed
                .entries
                .iter()
                .map(|entry| entry.track_id.as_str())
                .collect::<Vec<_>>(),
            vec!["track-1", "track-3", "track-2"]
        );

        let _ = std::fs::remove_dir_all(config_path);
    }

    #[test]
    fn auto_advance_requires_stop_near_end() {
        let state = sample_state(vec![sample_track("track-1", Some(1), Some(1), "Track 1")]);
        let queue = PlaybackQueue {
            renderer_location: "http://renderer.local/description.xml".to_string(),
            name: "Queue".to_string(),
            current_entry_id: Some(1),
            status: "playing".to_string(),
            version: 1,
            updated_unix: 0,
            entries: vec![QueueEntry {
                id: 1,
                position: 1,
                track_id: "track-1".to_string(),
                album_id: Some("album".to_string()),
                source_kind: "track".to_string(),
                source_ref: Some("track-1".to_string()),
                entry_status: "playing".to_string(),
                started_unix: Some(1),
                completed_unix: None,
            }],
        };
        let session = PlaybackSession {
            renderer_location: queue.renderer_location.clone(),
            queue_entry_id: Some(1),
            next_queue_entry_id: None,
            transport_state: "PLAYING".to_string(),
            current_track_uri: Some("http://musicd.local/stream/track/track-1".to_string()),
            position_seconds: Some(179),
            duration_seconds: Some(180),
            last_observed_unix: 1,
            last_error: None,
        };
        let snapshot = TransportSnapshot {
            transport_info: TransportInfo {
                transport_state: "STOPPED".to_string(),
                transport_status: Some("OK".to_string()),
                current_speed: Some("1".to_string()),
            },
            position_info: PositionInfo {
                track_uri: Some("http://musicd.local/stream/track/track-1".to_string()),
                rel_time_seconds: Some(179),
                track_duration_seconds: Some(180),
            },
        };
        assert!(should_auto_advance(
            &queue,
            Some(&session),
            &snapshot,
            &state
        ));

        let early_session = PlaybackSession {
            position_seconds: Some(40),
            ..session
        };
        let early_snapshot = TransportSnapshot {
            transport_info: snapshot.transport_info.clone(),
            position_info: PositionInfo {
                track_uri: snapshot.position_info.track_uri.clone(),
                rel_time_seconds: Some(40),
                track_duration_seconds: snapshot.position_info.track_duration_seconds,
            },
        };
        assert!(!should_auto_advance(
            &queue,
            Some(&early_session),
            &early_snapshot,
            &state
        ));

        let paused_session = PlaybackSession {
            transport_state: "PAUSED_PLAYBACK".to_string(),
            position_seconds: Some(179),
            ..early_session
        };
        let stopped_snapshot = TransportSnapshot {
            transport_info: TransportInfo {
                transport_state: "STOPPED".to_string(),
                transport_status: Some("OK".to_string()),
                current_speed: Some("1".to_string()),
            },
            position_info: PositionInfo {
                track_uri: Some("http://musicd.local/stream/track/track-1".to_string()),
                rel_time_seconds: Some(179),
                track_duration_seconds: Some(180),
            },
        };
        assert!(!should_auto_advance(
            &queue,
            Some(&paused_session),
            &stopped_snapshot,
            &state
        ));
    }

    #[test]
    fn next_queue_entry_uses_queue_order() {
        let queue = PlaybackQueue {
            renderer_location: "http://renderer.local/description.xml".to_string(),
            name: "Queue".to_string(),
            current_entry_id: Some(2),
            status: "playing".to_string(),
            version: 1,
            updated_unix: 0,
            entries: vec![
                QueueEntry {
                    id: 10,
                    position: 1,
                    track_id: "track-1".to_string(),
                    album_id: Some("album".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album".to_string()),
                    entry_status: "completed".to_string(),
                    started_unix: Some(1),
                    completed_unix: Some(2),
                },
                QueueEntry {
                    id: 20,
                    position: 2,
                    track_id: "track-2".to_string(),
                    album_id: Some("album".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album".to_string()),
                    entry_status: "playing".to_string(),
                    started_unix: Some(3),
                    completed_unix: None,
                },
                QueueEntry {
                    id: 30,
                    position: 3,
                    track_id: "track-3".to_string(),
                    album_id: Some("album".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album".to_string()),
                    entry_status: "pending".to_string(),
                    started_unix: None,
                    completed_unix: None,
                },
            ],
        };

        let next = next_queue_entry_after(&queue, 20).expect("next queue entry should exist");
        assert_eq!(next.id, 30);
        assert!(next_queue_entry_after(&queue, 30).is_none());
        let previous =
            previous_queue_entry_before(&queue, 20).expect("previous queue entry should exist");
        assert_eq!(previous.id, 10);
        assert!(previous_queue_entry_before(&queue, 10).is_none());
    }

    #[test]
    fn adopts_preloaded_next_entry_when_renderer_reports_next_uri() {
        let queue = PlaybackQueue {
            renderer_location: "http://renderer.local/description.xml".to_string(),
            name: "Queue".to_string(),
            current_entry_id: Some(20),
            status: "playing".to_string(),
            version: 1,
            updated_unix: 0,
            entries: vec![
                QueueEntry {
                    id: 20,
                    position: 1,
                    track_id: "track-1".to_string(),
                    album_id: Some("album".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album".to_string()),
                    entry_status: "playing".to_string(),
                    started_unix: Some(1),
                    completed_unix: None,
                },
                QueueEntry {
                    id: 30,
                    position: 2,
                    track_id: "track-2".to_string(),
                    album_id: Some("album".to_string()),
                    source_kind: "album".to_string(),
                    source_ref: Some("album".to_string()),
                    entry_status: "pending".to_string(),
                    started_unix: None,
                    completed_unix: None,
                },
            ],
        };
        let snapshot = TransportSnapshot {
            transport_info: TransportInfo {
                transport_state: "PLAYING".to_string(),
                transport_status: Some("OK".to_string()),
                current_speed: Some("1".to_string()),
            },
            position_info: PositionInfo {
                track_uri: Some("http://musicd.local/stream/track/track-2".to_string()),
                rel_time_seconds: Some(1),
                track_duration_seconds: Some(180),
            },
        };

        assert!(should_adopt_preloaded_next_entry(
            &queue,
            &snapshot,
            Some("http://musicd.local/stream/track/track-2")
        ));
        assert!(!should_adopt_preloaded_next_entry(
            &queue,
            &snapshot,
            Some("http://musicd.local/stream/track/track-3")
        ));
    }

    #[test]
    fn queue_status_follows_transport_state() {
        assert_eq!(queue_status_for_transport("PLAYING"), "playing");
        assert_eq!(queue_status_for_transport("TRANSITIONING"), "playing");
        assert_eq!(queue_status_for_transport("PAUSED_PLAYBACK"), "paused");
        assert_eq!(queue_status_for_transport("STOPPED"), "stopped");
    }

    #[test]
    fn prioritizes_cover_art_names() {
        assert!(
            artwork_name_priority("cover.jpg") < artwork_name_priority("folder.jpg"),
            "cover.jpg should outrank folder.jpg"
        );
        assert!(
            artwork_name_priority("folder.jpg") < artwork_name_priority("front.png"),
            "folder.jpg should outrank front.png"
        );
        assert_eq!(artwork_name_priority("booklet.jpg"), None);
    }

    #[test]
    fn detects_common_artwork_signatures() {
        assert_eq!(
            infer_image_mime_from_bytes(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0, 0, 0]),
            Some("image/jpeg")
        );
        assert_eq!(
            infer_image_mime_from_bytes(b"\x89PNG\r\n\x1a\nrest"),
            Some("image/png")
        );
        assert_eq!(
            infer_image_mime_from_bytes(b"RIFFxxxxWEBPrest"),
            Some("image/webp")
        );
        assert_eq!(infer_image_mime_from_bytes(b"not an image"), None);
    }

    #[test]
    fn merges_artists_with_same_normalized_name() {
        let mut first = sample_track("track-1", Some(1), Some(1), "Song A");
        first.artist = "Radiohead".to_string();
        first.album = "In Rainbows".to_string();
        first.album_id = stable_album_id(&first.artist, &first.album);

        let mut second = sample_track("track-2", Some(1), Some(1), "Song B");
        second.artist = " radiohead ".to_string();
        second.album = "Kid A".to_string();
        second.album_id = stable_album_id(&second.artist, &second.album);

        let artists = build_artist_summaries(&[first, second]);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].track_count, 2);
        assert_eq!(artists[0].album_count, 2);
        assert_eq!(artists[0].id, stable_artist_id("Radiohead"));
    }

    #[test]
    fn records_track_play_history_per_started_entry() {
        let config_path = temp_config_path("track-play-history");
        let database = Database::open(&config_path).expect("database should open");
        let queue = database
            .replace_queue(
                "renderer-1",
                "Test Queue",
                &[
                    QueueMutationEntry {
                        track_id: "track-1".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                    QueueMutationEntry {
                        track_id: "track-2".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "album".to_string(),
                        source_ref: Some("album-1".to_string()),
                    },
                ],
            )
            .expect("queue should be created");

        let first_entry = queue.entries.first().expect("first entry").id;
        let second_entry = queue.entries.get(1).expect("second entry").id;

        database
            .mark_queue_play_started(
                "renderer-1",
                first_entry,
                "track-1",
                "http://musicd.local/stream/track/track-1",
                Some(180),
            )
            .expect("first play should be recorded");
        database
            .adopt_next_queue_entry_as_current(
                "renderer-1",
                second_entry,
                "track-2",
                "http://musicd.local/stream/track/track-2",
                Some(200),
            )
            .expect("second play should be recorded");

        assert_eq!(database.count_track_plays("track-1").unwrap_or(0), 1);
        assert_eq!(database.count_track_plays("track-2").unwrap_or(0), 1);

        let history = database
            .load_track_play_history("track-2")
            .expect("history should load");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].track_id, "track-2");
        assert_eq!(history[0].renderer_location, "renderer-1");
        assert_eq!(history[0].queue_entry_id, Some(second_entry));
    }

    #[test]
    fn persists_normalized_albums_and_artists() {
        let config_path = temp_config_path("normalized-library");
        let database = Database::open(&config_path).expect("database should open");

        let mut first = sample_track("track-1", Some(1), Some(1), "15 Step");
        first.artist = "Radiohead".to_string();
        first.album = "In Rainbows".to_string();
        first.album_id = stable_album_id(&first.artist, &first.album);
        first.artwork = Some(TrackArtwork {
            cache_key: "cover.jpg".to_string(),
            source: "Embedded artwork".to_string(),
            mime_type: "image/jpeg".to_string(),
        });

        let mut second = sample_track("track-2", Some(1), Some(2), "Bodysnatchers");
        second.artist = "Radiohead".to_string();
        second.album = "In Rainbows".to_string();
        second.album_id = stable_album_id(&second.artist, &second.album);

        let mut third = sample_track("track-3", Some(1), Some(1), "Everything In Its Right Place");
        third.artist = "Radiohead".to_string();
        third.album = "Kid A".to_string();
        third.album_id = stable_album_id(&third.artist, &third.album);

        let library = Library::build(
            PathBuf::from("/music"),
            vec![first.clone(), second.clone(), third.clone()],
            &[],
        );

        database
            .save_library(&library)
            .expect("library should be persisted");

        let albums = database.load_albums().expect("albums should load");
        assert_eq!(albums.len(), 2);
        let in_rainbows = albums
            .iter()
            .find(|album| album.id == stable_album_id("Radiohead", "In Rainbows"))
            .expect("in rainbows album should exist");
        let expected_artwork_url = format!(
            "/artwork/album/{}",
            stable_album_id("Radiohead", "In Rainbows")
        );
        assert_eq!(in_rainbows.artist_id, stable_artist_id("Radiohead"));
        assert_eq!(in_rainbows.track_count, 2);
        assert_eq!(in_rainbows.first_track_id, "track-1");
        assert_eq!(
            in_rainbows.artwork_url.as_deref(),
            Some(expected_artwork_url.as_str())
        );
        assert_eq!(
            in_rainbows
                .artwork
                .as_ref()
                .map(|artwork| artwork.cache_key.as_str()),
            Some("cover.jpg")
        );

        let artists = database.load_artists().expect("artists should load");
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].id, stable_artist_id("Radiohead"));
        assert_eq!(artists[0].album_count, 2);
        assert_eq!(artists[0].track_count, 3);
        assert_eq!(
            artists[0].first_album_id,
            stable_album_id("Radiohead", "In Rainbows")
        );
    }

    #[test]
    fn persists_renderer_capabilities_and_health() {
        let config_path = temp_config_path("renderer-capabilities");
        let database = Database::open(&config_path).expect("database should open");

        database
            .upsert_renderer(&RendererRecord {
                location: "http://192.168.1.55:49152/description.xml".to_string(),
                name: "CXN V2".to_string(),
                manufacturer: Some("Cambridge Audio".to_string()),
                model_name: Some("CXN V2".to_string()),
                av_transport_control_url: Some(
                    "http://192.168.1.55:49152/upnp/control/avtransport1".to_string(),
                ),
                capabilities: RendererCapabilities {
                    av_transport_actions: Some(vec![
                        "Next".to_string(),
                        "Pause".to_string(),
                        "SetNextAVTransportURI".to_string(),
                    ]),
                    has_playlist_extension_service: Some(true),
                },
                last_checked_unix: 100,
                last_reachable_unix: Some(95),
                last_error: Some("timed out".to_string()),
                last_seen_unix: 95,
            })
            .expect("renderer should persist");

        let renderer = database
            .load_renderer("http://192.168.1.55:49152/description.xml")
            .expect("renderer should load")
            .expect("renderer record should exist");
        assert_eq!(renderer.name, "CXN V2");
        assert_eq!(
            renderer.capabilities.supports_set_next_av_transport_uri(),
            Some(true)
        );
        assert_eq!(renderer.capabilities.supports_pause(), Some(true));
        assert_eq!(renderer.capabilities.supports_previous(), Some(false));
        assert_eq!(
            renderer.capabilities.has_playlist_extension_service,
            Some(true)
        );
        assert_eq!(renderer.last_checked_unix, 100);
        assert_eq!(renderer.last_reachable_unix, Some(95));
        assert_eq!(renderer.last_error.as_deref(), Some("timed out"));
        assert_eq!(renderer.last_seen_unix, 95);
    }

    #[test]
    fn renderer_refresh_targets_incomplete_upnp_records() {
        let complete = RendererRecord {
            location: "http://192.168.1.55:49152/description.xml".to_string(),
            name: "CXN V2".to_string(),
            manufacturer: Some("Cambridge Audio".to_string()),
            model_name: Some("CXN V2".to_string()),
            av_transport_control_url: Some("http://renderer/avtransport".to_string()),
            capabilities: RendererCapabilities {
                av_transport_actions: Some(vec!["Pause".to_string()]),
                has_playlist_extension_service: Some(true),
            },
            last_checked_unix: 100,
            last_reachable_unix: Some(100),
            last_error: None,
            last_seen_unix: 100,
        };
        assert!(!renderer_needs_refresh(&complete));

        let mut missing_actions = complete.clone();
        missing_actions.capabilities.av_transport_actions = None;
        assert!(renderer_needs_refresh(&missing_actions));

        let mut fallback_name = complete.clone();
        fallback_name.name = fallback_name.location.clone();
        assert!(renderer_needs_refresh(&fallback_name));
    }

    #[test]
    fn rejects_non_playable_upnp_renderer_records() {
        let invalid = RendererRecord {
            location: "http://192.168.1.173:80/description.xml".to_string(),
            name: "Hue Bridge".to_string(),
            manufacturer: Some("Signify".to_string()),
            model_name: Some("Philips hue bridge 2015".to_string()),
            av_transport_control_url: None,
            capabilities: RendererCapabilities::default(),
            last_checked_unix: 10,
            last_reachable_unix: Some(10),
            last_error: None,
            last_seen_unix: 10,
        };
        assert!(!renderer_is_viable(&invalid));

        let mut valid = invalid.clone();
        valid.av_transport_control_url = Some("http://renderer/avtransport".to_string());
        assert!(renderer_is_viable(&valid));
    }

    fn sample_track(
        id: &str,
        disc_number: Option<u32>,
        track_number: Option<u32>,
        title: &str,
    ) -> LibraryTrack {
        LibraryTrack {
            id: id.to_string(),
            album_id: "album".to_string(),
            title: title.to_string(),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            disc_number,
            track_number,
            duration_seconds: Some(180),
            relative_path: format!("{title}.flac"),
            path: PathBuf::from(format!("/music/{title}.flac")),
            mime_type: "audio/flac".to_string(),
            file_size: 123,
            artwork: None,
        }
    }

    fn sample_state(tracks: Vec<LibraryTrack>) -> ServiceState {
        let config_path = temp_config_path("service-state");
        let database = Database::open(&config_path).expect("database should open");
        ServiceState {
            config: AppConfig {
                instance_name: "musicd".to_string(),
                library_path: PathBuf::from("/music"),
                config_path,
                bind_address: "0.0.0.0:7878".to_string(),
                base_url: "http://192.168.1.10:7878".to_string(),
                discovery_timeout_ms: 1500,
                default_renderer_location: None,
                debug_mode: false,
            },
            database,
            library: arc_swap::ArcSwap::from_pointee(Library::build(
                PathBuf::from("/music"),
                tracks,
                &[],
            )),
            renderer_backends: RendererBackends::default(),
            metrics: OnceLock::new(),
        }
    }

    fn temp_config_path(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        std::env::temp_dir().join(format!("musicd-{label}-{unique}"))
    }
}
