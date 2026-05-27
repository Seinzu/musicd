mod artwork;
mod assets;
mod cli;
mod db;
mod discovery;
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
    use crate::http::{HttpRequest, parse_query_string, parse_range_header, parse_request_form};
    use crate::ids::{stable_album_id, stable_artist_id, stable_track_id};
    use crate::library::{
        Library, build_artist_summaries, compare_track_album_order, decode_id3v1_text,
        parse_vorbis_comment_block,
    };
    use crate::renderer::{
        RendererBackend, RendererBackends, RendererKind, renderer_group_queue_key,
        renderer_is_viable, renderer_kind_for_location, renderer_needs_refresh,
    };
    use crate::service::{
        ServiceState, next_queue_entry_after, previous_queue_entry_before,
        queue_status_for_transport, should_adopt_preloaded_next_entry, should_auto_advance,
    };
    use crate::types::{
        LibraryTrack, PlaybackQueue, PlaybackSession, QueueEntry, QueueMutationEntry,
        RecommendationImportItem, RendererGroup, RendererRecord, TrackArtwork,
    };
    use crate::util::{
        cleanup_track_label, infer_artist_and_album, infer_disc_and_track_numbers,
        should_skip_entry,
    };
    use crate::views::{render_library_page, render_library_rows_json};
    use musicd_core::AppConfig;
    use musicd_upnp::{
        PositionInfo, RendererCapabilities, StreamResource, TransportInfo, TransportSnapshot,
    };
    use std::collections::{HashMap, VecDeque};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, OnceLock};
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
            renderer_kind_for_location("android-local://phone"),
            RendererKind::AndroidLocal
        );
        assert_eq!(
            renderer_kind_for_location("cli-local://terminal"),
            RendererKind::CliLocal
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
    fn final_queue_completion_clears_queue_and_session() {
        let config_path = temp_config_path("queue-final-completion");
        let database = Database::open(&config_path).expect("database should open");
        let renderer_location = "http://renderer.local/description.xml";
        let queue = database
            .replace_queue(
                renderer_location,
                "Album Queue",
                &[QueueMutationEntry {
                    track_id: "track-1".to_string(),
                    album_id: Some("album-1".to_string()),
                    source_kind: "track".to_string(),
                    source_ref: Some("track-1".to_string()),
                }],
            )
            .expect("queue replace should succeed");
        let current_entry_id = queue
            .current_entry_id
            .expect("queue should have current entry");
        database
            .mark_queue_play_started(
                renderer_location,
                current_entry_id,
                "track-1",
                "http://musicd.local/stream/track/track-1",
                Some(180),
            )
            .expect("queue play should start");

        let next_entry_id = database
            .advance_queue_after_completion(renderer_location)
            .expect("completion should be handled");

        assert_eq!(next_entry_id, None);
        assert!(
            database
                .load_queue(renderer_location)
                .expect("queue lookup should succeed")
                .is_none()
        );
        assert!(
            database
                .load_playback_session(renderer_location)
                .expect("session lookup should succeed")
                .is_none()
        );

        let _ = std::fs::remove_dir_all(config_path);
    }

    #[test]
    fn queue_completion_advances_when_next_entry_exists() {
        let config_path = temp_config_path("queue-mid-completion");
        let database = Database::open(&config_path).expect("database should open");
        let renderer_location = "http://renderer.local/description.xml";
        let queue = database
            .replace_queue(
                renderer_location,
                "Album Queue",
                &[
                    QueueMutationEntry {
                        track_id: "track-1".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "track".to_string(),
                        source_ref: Some("track-1".to_string()),
                    },
                    QueueMutationEntry {
                        track_id: "track-2".to_string(),
                        album_id: Some("album-1".to_string()),
                        source_kind: "track".to_string(),
                        source_ref: Some("track-2".to_string()),
                    },
                ],
            )
            .expect("queue replace should succeed");
        let current_entry_id = queue
            .current_entry_id
            .expect("queue should have current entry");
        database
            .mark_queue_play_started(
                renderer_location,
                current_entry_id,
                "track-1",
                "http://musicd.local/stream/track/track-1",
                Some(180),
            )
            .expect("queue play should start");

        let next_entry_id = database
            .advance_queue_after_completion(renderer_location)
            .expect("completion should be handled")
            .expect("next entry should exist");
        let advanced_queue = database
            .load_queue(renderer_location)
            .expect("queue lookup should succeed")
            .expect("queue should still exist");
        let session = database
            .load_playback_session(renderer_location)
            .expect("session lookup should succeed")
            .expect("session should still exist");

        assert_eq!(next_entry_id, queue.entries[1].id);
        assert_eq!(advanced_queue.current_entry_id, Some(queue.entries[1].id));
        assert_eq!(advanced_queue.status, "ready");
        assert_eq!(advanced_queue.entries[0].entry_status, "completed");
        assert_eq!(session.queue_entry_id, Some(queue.entries[1].id));
        assert_eq!(session.transport_state, "READY");

        let _ = std::fs::remove_dir_all(config_path);
    }

    #[test]
    fn skip_next_marks_previous_current_completed() {
        let renderer_location = "http://renderer.local/description.xml";
        let track_a = sample_track("track-a", Some(1), Some(1), "A");
        let track_b = sample_track("track-b", Some(1), Some(2), "B");
        let track_c = sample_track("track-c", Some(1), Some(3), "C");
        let track_d = sample_track("track-d", Some(1), Some(4), "D");
        let backend = Arc::new(FakeRendererBackend::new(renderer_location, Vec::new()));
        let state = sample_state_with_backend(
            vec![
                track_a.clone(),
                track_b.clone(),
                track_c.clone(),
                track_d.clone(),
            ],
            backend.clone(),
        );
        let first_stream_url = state.stream_resource_for_track(&track_a).stream_url;
        let second_resource = state.stream_resource_for_track(&track_b);
        let queue = state
            .database
            .replace_queue(
                renderer_location,
                "Four Tracks",
                &[
                    queue_entry_for_track(&track_a),
                    queue_entry_for_track(&track_b),
                    queue_entry_for_track(&track_c),
                    queue_entry_for_track(&track_d),
                ],
            )
            .expect("queue replace should succeed");
        let first_entry_id = queue.current_entry_id.expect("queue should have current");
        state
            .database
            .mark_queue_play_started(
                renderer_location,
                first_entry_id,
                &track_a.id,
                &first_stream_url,
                track_a.duration_seconds,
            )
            .expect("queue play should start");

        state
            .skip_to_next(renderer_location)
            .expect("skip next should advance queue");

        let played = backend.played_streams();
        assert_eq!(played.len(), 1);
        assert_eq!(played[0].stream_url, second_resource.stream_url);
        let advanced_queue = state
            .database
            .load_queue(renderer_location)
            .expect("queue lookup should succeed")
            .expect("queue should remain");
        assert_eq!(advanced_queue.current_entry_id, Some(queue.entries[1].id));
        assert_eq!(
            advanced_queue
                .entries
                .iter()
                .map(|entry| (entry.track_id.as_str(), entry.entry_status.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("track-a", "completed"),
                ("track-b", "playing"),
                ("track-c", "pending"),
                ("track-d", "pending"),
            ]
        );
        assert_eq!(
            advanced_queue
                .entries
                .iter()
                .filter(|entry| entry.entry_status == "pending")
                .map(|entry| entry.track_id.as_str())
                .collect::<Vec<_>>(),
            vec!["track-c", "track-d"]
        );

        let _ = std::fs::remove_dir_all(state.config.config_path.clone());
    }

    #[test]
    fn resume_with_empty_queue_does_not_restart_renderer_media() {
        let renderer_location = "http://renderer.local/description.xml";
        let track = sample_track("track-1", Some(1), Some(1), "Track 1");
        let backend = Arc::new(FakeRendererBackend::new(
            renderer_location,
            vec![playing_snapshot(&track, 1, 180)],
        ));
        let state = sample_state_with_backend(vec![track.clone()], backend.clone());
        state
            .database
            .record_transport_snapshot(
                renderer_location,
                "STOPPED",
                Some(&state.stream_resource_for_track(&track).stream_url),
                Some(180),
                track.duration_seconds,
            )
            .expect("stale renderer session should be recorded");

        let error = state
            .resume_renderer(renderer_location)
            .expect_err("empty queue should not be resumable");

        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(backend.play_count(), 0);
        assert!(
            state
                .database
                .load_queue(renderer_location)
                .expect("queue lookup should succeed")
                .is_none(),
            "resume should not recreate a queue"
        );

        let _ = std::fs::remove_dir_all(state.config.config_path.clone());
    }

    #[test]
    fn resume_sleeping_renderer_restarts_track_at_last_known_position() {
        let renderer_location = "http://renderer.local/description.xml";
        let track = sample_track("track-1", Some(1), Some(1), "Track 1");
        let backend = Arc::new(FakeRendererBackend::new(
            renderer_location,
            vec![playing_snapshot(&track, 0, 180)],
        ));
        let state = sample_state_with_backend(vec![track.clone()], backend.clone());
        let queue = state
            .database
            .replace_queue(
                renderer_location,
                "Manual",
                &[queue_entry_for_track(&track)],
            )
            .expect("queue should be created");
        let resource = state.stream_resource_for_track(&track);
        state
            .database
            .mark_queue_play_started(
                renderer_location,
                queue.entries[0].id,
                &track.id,
                &resource.stream_url,
                track.duration_seconds,
            )
            .expect("queue session should be marked started");
        state
            .database
            .record_transport_snapshot(
                renderer_location,
                "STOPPED",
                Some(&resource.stream_url),
                Some(42),
                track.duration_seconds,
            )
            .expect("sleeping renderer snapshot should be recorded");

        state
            .resume_renderer(renderer_location)
            .expect("sleeping renderer should resume");

        let played = backend.played_streams();
        assert_eq!(played.len(), 1);
        assert_eq!(played[0].stream_url, resource.stream_url);
        assert_eq!(backend.seek_positions(), vec![42]);
        let session = state
            .database
            .load_playback_session(renderer_location)
            .expect("session lookup should succeed")
            .expect("session should exist");
        assert_eq!(session.transport_state, "PLAYING");
        assert_eq!(session.position_seconds, Some(42));

        let _ = std::fs::remove_dir_all(state.config.config_path.clone());
    }

    #[test]
    fn queue_poll_clears_final_completed_track_without_restarting_it() {
        let renderer_location = "http://renderer.local/description.xml";
        let track = sample_track("track-1", Some(1), Some(1), "Track 1");
        let backend = Arc::new(FakeRendererBackend::new(
            renderer_location,
            vec![stopped_near_end_snapshot(&track, 179, 180)],
        ));
        let state = sample_state_with_backend(vec![track.clone()], backend.clone());
        let stream_url = state.stream_resource_for_track(&track).stream_url;
        let queue = state
            .database
            .replace_queue(
                renderer_location,
                "Single Track",
                &[queue_entry_for_track(&track)],
            )
            .expect("queue replace should succeed");
        let current_entry_id = queue
            .current_entry_id
            .expect("queue should have a current entry");
        state
            .database
            .mark_queue_play_started(
                renderer_location,
                current_entry_id,
                &track.id,
                &stream_url,
                track.duration_seconds,
            )
            .expect("queue play should start");

        state
            .poll_renderer_queue(renderer_location)
            .expect("queue poll should handle final completion");

        assert!(
            state
                .database
                .load_queue(renderer_location)
                .expect("queue lookup should succeed")
                .is_none(),
            "the final entry should clear the queue instead of remaining restartable"
        );
        assert!(
            state
                .database
                .load_playback_session(renderer_location)
                .expect("session lookup should succeed")
                .is_none(),
            "the final entry should clear stale now-playing metadata"
        );
        assert!(
            backend.played_streams().is_empty(),
            "polling a completed final track must not start the same stream again"
        );
        assert_eq!(state.database.count_track_plays(&track.id).unwrap_or(0), 1);

        let _ = std::fs::remove_dir_all(state.config.config_path.clone());
    }

    #[test]
    fn queue_poll_treats_completed_transport_as_finished() {
        let renderer_location = "http://renderer.local/description.xml";
        let track = sample_track("track-1", Some(1), Some(1), "Track 1");
        let backend = Arc::new(FakeRendererBackend::new(
            renderer_location,
            vec![completed_near_end_snapshot(&track, 180, 180)],
        ));
        let state = sample_state_with_backend(vec![track.clone()], backend.clone());
        let stream_url = state.stream_resource_for_track(&track).stream_url;
        let queue = state
            .database
            .replace_queue(
                renderer_location,
                "Single Track",
                &[queue_entry_for_track(&track)],
            )
            .expect("queue replace should succeed");
        let current_entry_id = queue
            .current_entry_id
            .expect("queue should have a current entry");
        state
            .database
            .mark_queue_play_started(
                renderer_location,
                current_entry_id,
                &track.id,
                &stream_url,
                track.duration_seconds,
            )
            .expect("queue play should start");

        state
            .poll_renderer_queue(renderer_location)
            .expect("queue poll should handle completed transport state");

        assert!(
            state
                .database
                .load_queue(renderer_location)
                .expect("queue lookup should succeed")
                .is_none()
        );
        assert!(backend.played_streams().is_empty());

        let _ = std::fs::remove_dir_all(state.config.config_path.clone());
    }

    #[test]
    fn queue_poll_starts_next_entry_with_matching_track_metadata() {
        let renderer_location = "http://renderer.local/description.xml";
        let track_1 = sample_track("track-1", Some(1), Some(1), "Track 1");
        let track_2 = sample_track("track-2", Some(1), Some(2), "Track 2");
        let backend = Arc::new(FakeRendererBackend::new(
            renderer_location,
            vec![stopped_near_end_snapshot(&track_1, 179, 180)],
        ));
        let state =
            sample_state_with_backend(vec![track_1.clone(), track_2.clone()], backend.clone());
        let first_stream_url = state.stream_resource_for_track(&track_1).stream_url;
        let second_resource = state.stream_resource_for_track(&track_2);
        let queue = state
            .database
            .replace_queue(
                renderer_location,
                "Two Tracks",
                &[
                    queue_entry_for_track(&track_1),
                    queue_entry_for_track(&track_2),
                ],
            )
            .expect("queue replace should succeed");
        let first_entry_id = queue
            .current_entry_id
            .expect("queue should have a current entry");
        let second_entry_id = queue.entries[1].id;
        state
            .database
            .mark_queue_play_started(
                renderer_location,
                first_entry_id,
                &track_1.id,
                &first_stream_url,
                track_1.duration_seconds,
            )
            .expect("queue play should start");

        state
            .poll_renderer_queue(renderer_location)
            .expect("queue poll should advance");

        let played = backend.played_streams();
        assert_eq!(played.len(), 1, "exactly the next entry should be started");
        assert_eq!(played[0].stream_url, second_resource.stream_url);
        assert_eq!(played[0].title, second_resource.title);

        let advanced_queue = state
            .database
            .load_queue(renderer_location)
            .expect("queue lookup should succeed")
            .expect("queue should remain for the next track");
        let session = state
            .database
            .load_playback_session(renderer_location)
            .expect("session lookup should succeed")
            .expect("session should track the new entry");
        assert_eq!(advanced_queue.current_entry_id, Some(second_entry_id));
        assert_eq!(advanced_queue.status, "playing");
        assert_eq!(advanced_queue.entries[0].entry_status, "completed");
        assert_eq!(advanced_queue.entries[1].entry_status, "playing");
        assert_eq!(session.queue_entry_id, Some(second_entry_id));
        assert_eq!(
            session.current_track_uri.as_deref(),
            Some(played[0].stream_url.as_str())
        );
        assert_eq!(
            state.database.count_track_plays(&track_1.id).unwrap_or(0),
            1
        );
        assert_eq!(
            state.database.count_track_plays(&track_2.id).unwrap_or(0),
            1
        );
        assert_eq!(
            backend.cleared_next_count(),
            1,
            "starting the final entry should clear any stale renderer-side next URI"
        );

        let _ = std::fs::remove_dir_all(state.config.config_path.clone());
    }

    #[test]
    fn queue_poll_clears_stale_preloaded_next_when_renderer_auto_advances_to_final_track() {
        let renderer_location = "http://renderer.local/description.xml";
        let track_1 = sample_track("track-1", Some(1), Some(1), "Track 1");
        let track_2 = sample_track("track-2", Some(1), Some(2), "Track 2");
        let backend = Arc::new(FakeRendererBackend::new(
            renderer_location,
            vec![playing_snapshot(&track_2, 1, 180)],
        ));
        let state =
            sample_state_with_backend(vec![track_1.clone(), track_2.clone()], backend.clone());
        let first_stream_url = state.stream_resource_for_track(&track_1).stream_url;
        let second_stream_url = state.stream_resource_for_track(&track_2).stream_url;
        let queue = state
            .database
            .replace_queue(
                renderer_location,
                "Two Tracks",
                &[
                    queue_entry_for_track(&track_1),
                    queue_entry_for_track(&track_2),
                ],
            )
            .expect("queue replace should succeed");
        let first_entry_id = queue
            .current_entry_id
            .expect("queue should have a current entry");
        let second_entry_id = queue.entries[1].id;
        state
            .database
            .mark_queue_play_started(
                renderer_location,
                first_entry_id,
                &track_1.id,
                &first_stream_url,
                track_1.duration_seconds,
            )
            .expect("queue play should start");
        state
            .database
            .mark_next_queue_entry_preloaded(renderer_location, Some(second_entry_id))
            .expect("next entry should be marked preloaded");

        state
            .poll_renderer_queue(renderer_location)
            .expect("queue poll should adopt renderer-advanced final track");

        let advanced_queue = state
            .database
            .load_queue(renderer_location)
            .expect("queue lookup should succeed")
            .expect("queue should remain while final track plays");
        let session = state
            .database
            .load_playback_session(renderer_location)
            .expect("session lookup should succeed")
            .expect("session should track the adopted entry");
        assert_eq!(advanced_queue.current_entry_id, Some(second_entry_id));
        assert_eq!(advanced_queue.entries[0].entry_status, "completed");
        assert_eq!(advanced_queue.entries[1].entry_status, "playing");
        assert_eq!(session.queue_entry_id, Some(second_entry_id));
        assert_eq!(session.next_queue_entry_id, None);
        assert_eq!(
            session.current_track_uri.as_deref(),
            Some(second_stream_url.as_str())
        );
        assert!(backend.played_streams().is_empty());
        assert_eq!(
            backend.cleared_next_count(),
            1,
            "the renderer's native next URI should be cleared once the preloaded final track becomes current"
        );

        let _ = std::fs::remove_dir_all(state.config.config_path.clone());
    }

    #[test]
    fn start_current_single_track_clears_stale_renderer_next_slot() {
        let renderer_location = "http://renderer.local/description.xml";
        let track = sample_track("track-1", Some(1), Some(1), "Track 1");
        let backend = Arc::new(FakeRendererBackend::new(renderer_location, Vec::new()));
        let state = sample_state_with_backend(vec![track.clone()], backend.clone());
        state
            .database
            .replace_queue(
                renderer_location,
                "Single Track",
                &[queue_entry_for_track(&track)],
            )
            .expect("queue replace should succeed");

        state
            .start_current_queue_entry(renderer_location)
            .expect("single track should start");

        assert_eq!(
            backend.cleared_next_count(),
            1,
            "starting a queue with no successor should clear any stale renderer-side next URI"
        );

        let _ = std::fs::remove_dir_all(state.config.config_path.clone());
    }

    #[test]
    fn queue_poll_does_not_repeat_final_track_next_clear_when_session_has_no_next() {
        let renderer_location = "http://renderer.local/description.xml";
        let track_1 = sample_track("track-1", Some(1), Some(1), "Track 1");
        let track_2 = sample_track("track-2", Some(1), Some(2), "Track 2");
        let backend = Arc::new(FakeRendererBackend::new(
            renderer_location,
            vec![playing_snapshot(&track_2, 21, 180)],
        ));
        let state =
            sample_state_with_backend(vec![track_1.clone(), track_2.clone()], backend.clone());
        let second_stream_url = state.stream_resource_for_track(&track_2).stream_url;
        let queue = state
            .database
            .replace_queue(
                renderer_location,
                "Two Tracks",
                &[
                    queue_entry_for_track(&track_1),
                    queue_entry_for_track(&track_2),
                ],
            )
            .expect("queue replace should succeed");
        let second_entry_id = queue.entries[1].id;
        state
            .database
            .select_queue_entry(renderer_location, second_entry_id)
            .expect("final entry should be selected");
        state
            .database
            .mark_queue_play_started(
                renderer_location,
                second_entry_id,
                &track_2.id,
                &second_stream_url,
                track_2.duration_seconds,
            )
            .expect("final entry should be marked playing");
        state
            .database
            .mark_next_queue_entry_preloaded(renderer_location, None)
            .expect("session should not remember a preloaded next entry");

        state
            .poll_renderer_queue(renderer_location)
            .expect("queue poll should refresh final track state");

        assert_eq!(
            backend.cleared_next_count(),
            0,
            "final-track polling should not repeatedly clear the renderer next slot once the DB session is already clear"
        );

        let _ = std::fs::remove_dir_all(state.config.config_path.clone());
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
                None,
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
        assert!(state.database.delete_renderer_group(&group.id).unwrap());
        assert!(
            state
                .database
                .load_queue(&renderer_group_queue_key(&group.id))
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .database
                .load_renderer_group(&group.id)
                .unwrap()
                .is_none()
        );

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn renderer_group_create_transfers_active_source_playback() {
        let track_1 = sample_track("track-1", Some(1), Some(1), "Track 1");
        let track_2 = sample_track("track-2", Some(1), Some(2), "Track 2");
        let state = sample_state(vec![track_1.clone(), track_2.clone()]);
        let source_location = "android-local://phone-a";

        let source_queue = state
            .database
            .replace_queue(
                source_location,
                "Source Queue",
                &[
                    queue_entry_for_track(&track_1),
                    queue_entry_for_track(&track_2),
                ],
            )
            .expect("source queue should be created");
        let advanced = state
            .database
            .advance_queue_after_completion(source_location)
            .expect("advance should succeed")
            .expect("next entry should exist");
        let source_current_id = advanced;
        state
            .database
            .mark_queue_play_started(
                source_location,
                source_current_id,
                &track_2.id,
                "http://musicd.local/stream/track/track-2",
                track_2.duration_seconds,
            )
            .expect("source playback should start");
        state
            .database
            .record_transport_snapshot(
                source_location,
                "PLAYING",
                Some("http://musicd.local/stream/track/track-2"),
                Some(42),
                track_2.duration_seconds,
            )
            .expect("source position snapshot should record");

        let group = state
            .create_renderer_group(
                "Phones",
                &[
                    source_location.to_string(),
                    "android-local://phone-b".to_string(),
                ],
                Some(source_location),
                None,
            )
            .expect("group should be created");
        let group_queue_key = renderer_group_queue_key(&group.id);

        let group_queue = state
            .database
            .load_queue(&group_queue_key)
            .expect("group queue should load")
            .expect("group queue should exist");
        assert_eq!(group_queue.entries.len(), source_queue.entries.len());
        let expected_position = source_queue
            .entries
            .iter()
            .find(|entry| entry.id == source_current_id)
            .map(|entry| entry.position)
            .expect("source current entry should exist");
        let group_current = group_queue
            .current_entry_id
            .and_then(|id| group_queue.entries.iter().find(|entry| entry.id == id))
            .expect("group queue should have a current entry");
        assert_eq!(group_current.position, expected_position);
        assert_eq!(group_current.track_id, track_2.id);
        assert_eq!(group_queue.status, "playing");

        let track_1_in_group = group_queue
            .entries
            .iter()
            .find(|entry| entry.track_id == track_1.id)
            .expect("track-1 should still be in the group queue");
        assert_eq!(
            track_1_in_group.entry_status, "completed",
            "previously played tracks must keep their completed status across the transfer"
        );
        assert!(track_1_in_group.completed_unix.is_some());

        let group_session = state
            .database
            .load_playback_session(&group_queue_key)
            .expect("group session should load")
            .expect("group session should exist");
        assert_eq!(group_session.transport_state, "PLAYING");
        assert_eq!(group_session.queue_entry_id, Some(group_current.id));
        assert_eq!(group_session.position_seconds, Some(42));

        assert!(
            state
                .database
                .load_queue(source_location)
                .expect("source queue lookup should succeed")
                .is_none(),
            "source queue should be cleared after transfer"
        );
        assert!(
            state
                .database
                .load_playback_session(source_location)
                .expect("source session lookup should succeed")
                .is_none(),
            "source session should be cleared after transfer"
        );

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn renderer_group_delete_transfers_queue_to_inheritor() {
        let track_1 = sample_track("track-1", Some(1), Some(1), "Track 1");
        let track_2 = sample_track("track-2", Some(1), Some(2), "Track 2");
        let state = sample_state(vec![track_1.clone(), track_2.clone()]);
        let source_location = "android-local://phone-a";
        state
            .database
            .replace_queue(
                source_location,
                "Source Queue",
                &[
                    queue_entry_for_track(&track_1),
                    queue_entry_for_track(&track_2),
                ],
            )
            .expect("source queue should be created");
        let advanced = state
            .database
            .advance_queue_after_completion(source_location)
            .expect("advance should succeed")
            .expect("next entry should exist");
        state
            .database
            .mark_queue_play_started(
                source_location,
                advanced,
                &track_2.id,
                "http://musicd.local/stream/track/track-2",
                track_2.duration_seconds,
            )
            .expect("source playback should start");
        state
            .database
            .record_transport_snapshot(
                source_location,
                "PLAYING",
                Some("http://musicd.local/stream/track/track-2"),
                Some(99),
                track_2.duration_seconds,
            )
            .expect("source position snapshot should record");

        let group = state
            .create_renderer_group(
                "Phones",
                &[
                    source_location.to_string(),
                    "android-local://phone-b".to_string(),
                ],
                Some(source_location),
                None,
            )
            .expect("group should be created");
        let group_queue_key = renderer_group_queue_key(&group.id);

        // Pre-condition: source's individual queue is now empty (transferred to group on create).
        assert!(
            state
                .database
                .load_queue(source_location)
                .expect("source queue lookup should succeed")
                .is_none()
        );

        let deleted = state
            .delete_renderer_group_by_queue_key(&group_queue_key, Some(source_location))
            .expect("group delete should succeed");
        assert_eq!(deleted.id, group.id);

        let restored_queue = state
            .database
            .load_queue(source_location)
            .expect("inheritor queue lookup should succeed")
            .expect("inheritor queue should be re-created");
        assert_eq!(restored_queue.entries.len(), 2);
        assert_eq!(restored_queue.status, "playing");
        let restored_current = restored_queue
            .current_entry_id
            .and_then(|id| restored_queue.entries.iter().find(|entry| entry.id == id))
            .expect("inheritor queue should have a current entry");
        assert_eq!(restored_current.track_id, track_2.id);
        let track_1_in_inheritor = restored_queue
            .entries
            .iter()
            .find(|entry| entry.track_id == track_1.id)
            .expect("track-1 should still be in the inheritor queue");
        assert_eq!(
            track_1_in_inheritor.entry_status, "completed",
            "previously played tracks must keep their completed status when transferred back"
        );
        assert!(track_1_in_inheritor.completed_unix.is_some());

        let restored_session = state
            .database
            .load_playback_session(source_location)
            .expect("inheritor session lookup should succeed")
            .expect("inheritor session should exist");
        assert_eq!(restored_session.transport_state, "PLAYING");
        assert_eq!(restored_session.position_seconds, Some(99));
        assert_eq!(restored_session.queue_entry_id, Some(restored_current.id));

        assert!(
            state
                .database
                .load_queue(&group_queue_key)
                .expect("group queue lookup should succeed")
                .is_none(),
            "group queue should be deleted"
        );
        assert!(
            state
                .database
                .load_renderer_group(&group.id)
                .expect("group lookup should succeed")
                .is_none(),
            "group record should be deleted"
        );

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn renderer_group_create_allows_missing_source_queue() {
        let state = sample_state(Vec::new());
        let group = state
            .create_renderer_group(
                "Downstairs",
                &[
                    "http://kitchen.local/description.xml".to_string(),
                    "http://living-room.local/description.xml".to_string(),
                ],
                Some("http://kitchen.local/description.xml"),
                None,
            )
            .expect("group should be created without a source queue");

        assert_eq!(group.name, "Downstairs");
        assert_eq!(group.members.len(), 2);
        let queue = state
            .database
            .load_queue(&renderer_group_queue_key(&group.id))
            .expect("group queue should load")
            .expect("group queue should exist");
        assert_eq!(queue.entries.len(), 0);

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn renderer_group_lifecycle_rejects_invalid_members() {
        let state = sample_state(Vec::new());
        let duplicate_error = state
            .create_renderer_group(
                "Duplicate",
                &renderer_locations(&[
                    "http://kitchen.local/description.xml",
                    "http://kitchen.local/description.xml",
                    "http://living-room.local/description.xml",
                ]),
                None,
                None,
            )
            .expect_err("duplicate members should be rejected");
        assert_eq!(duplicate_error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(
            duplicate_error.to_string(),
            "renderer groups cannot contain duplicate members"
        );

        let nested_error = state
            .create_renderer_group(
                "Nested",
                &renderer_locations(&["group:nested", "http://living-room.local/description.xml"]),
                None,
                None,
            )
            .expect_err("nested groups should be rejected");
        assert_eq!(nested_error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(
            nested_error.to_string(),
            "renderer groups cannot contain other groups"
        );

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn renderer_group_additions_require_private_renderer_owner() {
        let state = sample_state(Vec::new());
        state
            .database
            .upsert_renderer(&RendererRecord {
                location: "android-local://owner-phone".to_string(),
                name: "Owner phone".to_string(),
                manufacturer: Some("Android".to_string()),
                model_name: None,
                av_transport_control_url: None,
                capabilities: RendererCapabilities::default(),
                visibility: "private".to_string(),
                owner_client_id: Some("owner-client".to_string()),
                last_checked_unix: 10,
                last_reachable_unix: Some(10),
                last_error: None,
                last_seen_unix: 10,
            })
            .expect("private renderer should persist");

        let denied = state
            .create_renderer_group(
                "Shared",
                &renderer_locations(&[
                    "android-local://owner-phone",
                    "http://kitchen.local/description.xml",
                ]),
                None,
                Some("other-client"),
            )
            .expect_err("other clients cannot add private renderers");
        assert_eq!(denied.kind(), std::io::ErrorKind::PermissionDenied);

        let group = state
            .create_renderer_group(
                "Shared",
                &renderer_locations(&[
                    "android-local://owner-phone",
                    "http://kitchen.local/description.xml",
                ]),
                None,
                Some("owner-client"),
            )
            .expect("owner can add private renderer");
        assert_eq!(group.members.len(), 2);

        assert!(
            state
                .check_direct_renderer_access("android-local://owner-phone", Some("owner-client"))
                .is_ok()
        );
        assert_eq!(
            state
                .check_direct_renderer_access("android-local://owner-phone", Some("other-client"))
                .expect_err("other clients cannot control private renderer")
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn imports_album_recommendations_idempotently() {
        let config_path = temp_config_path("album-recommendations");
        let database = Database::open(&config_path).expect("database should open");
        let items = vec![RecommendationImportItem {
            recommendation_key: None,
            source: None,
            batch_id: None,
            seed_album_id: "seed-album".to_string(),
            seed_musicbrainz_release_id: Some("seed-release".to_string()),
            suggested_artist: "Talk Talk".to_string(),
            suggested_title: "Spirit of Eden".to_string(),
            suggested_musicbrainz_release_id: Some("suggested-release".to_string()),
            suggested_musicbrainz_release_group_id: Some("suggested-group".to_string()),
            confidence: Some(0.92),
            rationale: Some("Shared spacious late-night pacing.".to_string()),
            external_url: Some("https://musicbrainz.org/release-group/suggested-group".to_string()),
            tidal_url: Some("https://tidal.com/browse/album/12345".to_string()),
            artwork_url: None,
            status: None,
        }];

        let imported = database
            .upsert_album_recommendations("llm-test", Some("batch-1"), &items)
            .expect("recommendation import should succeed");
        assert_eq!(imported, 1);
        database
            .upsert_album_recommendations("llm-test", Some("batch-1"), &items)
            .expect("recommendation import should be idempotent");

        let recommendations = database
            .list_album_recommendations(Some("seed-album"))
            .expect("recommendations should load");
        assert_eq!(recommendations.len(), 1);
        assert_eq!(recommendations[0].source, "llm-test");
        assert_eq!(recommendations[0].batch_id.as_deref(), Some("batch-1"));
        assert_eq!(recommendations[0].suggested_artist, "Talk Talk");
        assert_eq!(recommendations[0].suggested_title, "Spirit of Eden");
        assert_eq!(
            recommendations[0].tidal_url.as_deref(),
            Some("https://tidal.com/browse/album/12345")
        );
        assert_eq!(recommendations[0].status, "suggested");

        let _ = std::fs::remove_dir_all(config_path);
    }

    #[test]
    fn deletes_album_recommendations() {
        let config_path = temp_config_path("album-recommendations-delete");
        let database = Database::open(&config_path).expect("database should open");
        let items = vec![RecommendationImportItem {
            recommendation_key: None,
            source: None,
            batch_id: None,
            seed_album_id: "seed-album".to_string(),
            seed_musicbrainz_release_id: None,
            suggested_artist: "Talk Talk".to_string(),
            suggested_title: "Spirit of Eden".to_string(),
            suggested_musicbrainz_release_id: None,
            suggested_musicbrainz_release_group_id: Some("suggested-group".to_string()),
            confidence: Some(0.92),
            rationale: None,
            external_url: None,
            tidal_url: None,
            artwork_url: None,
            status: None,
        }];

        database
            .upsert_album_recommendations("llm-test", Some("batch-1"), &items)
            .expect("recommendation import should succeed");
        let deleted = database
            .delete_album_recommendations()
            .expect("recommendations should delete");
        assert_eq!(deleted, 1);
        assert!(
            database
                .list_album_recommendations(None)
                .expect("recommendations should load")
                .is_empty()
        );

        let _ = std::fs::remove_dir_all(config_path);
    }

    #[test]
    fn renderer_group_lifecycle_updates_members_and_queue_name() {
        let state = sample_state(Vec::new());
        let group = create_sample_renderer_group(
            &state,
            "Downstairs",
            &[
                "http://kitchen.local/description.xml",
                "http://living-room.local/description.xml",
            ],
        );
        let group_location = renderer_group_queue_key(&group.id);
        assert_group_members(
            &group,
            &[
                "http://kitchen.local/description.xml",
                "http://living-room.local/description.xml",
            ],
        );

        let updated = state
            .update_renderer_group_by_queue_key(
                &group_location,
                "Evening",
                &renderer_locations(&[
                    "http://living-room.local/description.xml",
                    "android-local://phone",
                ]),
                None,
            )
            .expect("group should update");
        assert_eq!(updated.name, "Evening");
        assert_group_members(
            &updated,
            &[
                "http://living-room.local/description.xml",
                "android-local://phone",
            ],
        );
        let preserved_member = updated
            .members
            .iter()
            .find(|member| member.renderer_location == "http://living-room.local/description.xml")
            .expect("unchanged member should remain");
        let original_joined = group
            .members
            .iter()
            .find(|member| member.renderer_location == "http://living-room.local/description.xml")
            .expect("original member should exist")
            .joined_unix;
        assert_eq!(preserved_member.joined_unix, original_joined);

        let queue = state
            .database
            .load_queue(&group_location)
            .expect("group queue should load")
            .expect("group queue should exist");
        assert_eq!(queue.name, "Evening");

        let too_small_error = state
            .update_renderer_group_by_queue_key(
                &group_location,
                "Too Small",
                &renderer_locations(&["android-local://phone"]),
                None,
            )
            .expect_err("single-member groups should be rejected");
        assert_eq!(too_small_error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(
            too_small_error.to_string(),
            "renderer groups require at least two members"
        );

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn renderer_group_add_member_during_playback_succeeds() {
        let track = sample_track("track-1", Some(1), Some(1), "Track 1");
        let state = sample_state(vec![track.clone()]);
        let group = create_sample_renderer_group(
            &state,
            "Phones",
            &["android-local://phone-a", "android-local://phone-b"],
        );
        let group_location = renderer_group_queue_key(&group.id);
        state
            .database
            .replace_queue(&group_location, "Phones", &[queue_entry_for_track(&track)])
            .expect("group queue should be created");
        state
            .start_current_queue_entry(&group_location)
            .expect("group playback should start");

        let updated = state
            .update_renderer_group_by_queue_key(
                &group_location,
                "Phones",
                &renderer_locations(&[
                    "android-local://phone-a",
                    "android-local://phone-b",
                    "android-local://phone-c",
                ]),
                None,
            )
            .expect("group update should succeed during active playback");
        assert_group_members(
            &updated,
            &[
                "android-local://phone-a",
                "android-local://phone-b",
                "android-local://phone-c",
            ],
        );

        let session = state
            .database
            .load_playback_session(&group_location)
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(session.transport_state, "PLAYING");
        assert_eq!(session.queue_entry_id.is_some(), true);

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn renderer_group_queue_mutations_require_existing_group() {
        let track = sample_track("track-1", Some(1), Some(1), "Track 1");
        let state = sample_state(vec![track.clone()]);
        let error = state
            .append_track_to_queue("group:missing", &track)
            .expect_err("missing group queue mutations should fail");
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(error.to_string(), "renderer group not found");
        assert!(
            state
                .database
                .load_queue("group:missing")
                .expect("queue lookup should succeed")
                .is_none()
        );

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn renderer_group_queue_mutations_refresh_active_group_preload() {
        let track_1 = sample_track("track-1", Some(1), Some(1), "Track 1");
        let track_2 = sample_track("track-2", Some(1), Some(2), "Track 2");
        let track_3 = sample_track("track-3", Some(1), Some(3), "Track 3");
        let state = sample_state(vec![track_1.clone(), track_2.clone(), track_3.clone()]);
        let group = create_sample_renderer_group(
            &state,
            "Phones",
            &["android-local://phone-a", "android-local://phone-b"],
        );
        let group_location = renderer_group_queue_key(&group.id);
        let queue = state
            .database
            .replace_queue(
                &group_location,
                "Phones",
                &[
                    queue_entry_for_track(&track_1),
                    queue_entry_for_track(&track_2),
                ],
            )
            .expect("group queue should be created");
        state
            .start_current_queue_entry(&group_location)
            .expect("group playback should start");
        let stale_next_entry_id = queue.entries[1].id;
        state
            .database
            .mark_next_queue_entry_preloaded(&group_location, Some(stale_next_entry_id))
            .expect("test should install stale preload");

        let updated = state
            .play_next_track(&group_location, &track_3)
            .expect("play-next should update group queue");
        assert_eq!(
            updated
                .entries
                .iter()
                .map(|entry| entry.track_id.as_str())
                .collect::<Vec<_>>(),
            vec!["track-1", "track-3", "track-2"]
        );
        let session = state
            .database
            .load_playback_session(&group_location)
            .expect("group session should load")
            .expect("group session should exist");
        assert_eq!(session.next_queue_entry_id, None);

        state
            .clear_queue(&group_location)
            .expect("group queue should clear");
        assert!(
            state
                .database
                .load_queue(&group_location)
                .expect("queue lookup should succeed")
                .is_none()
        );
        assert!(
            state
                .database
                .load_playback_session(&group_location)
                .expect("session lookup should succeed")
                .is_none()
        );

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn renderer_group_partial_fanout_records_session_warning() {
        let track = sample_track("track-1", Some(1), Some(1), "Track 1");
        let state = sample_state(vec![track.clone()]);
        let group = create_sample_renderer_group(
            &state,
            "Mixed",
            &["android-local://phone", "sonos:kitchen"],
        );
        let group_location = renderer_group_queue_key(&group.id);
        state
            .database
            .replace_queue(&group_location, "Mixed", &[queue_entry_for_track(&track)])
            .expect("group queue should be created");

        let (_, _, renderer_name, renderer_location) = state
            .start_current_queue_entry(&group_location)
            .expect("partial fan-out should still start");
        assert_eq!(renderer_name, "Mixed (1 of 2 renderers)");
        assert_eq!(renderer_location, group_location);
        let session = state
            .database
            .load_playback_session(&renderer_location)
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(session.transport_state, "PLAYING");
        assert!(session.last_error.as_deref().is_some_and(|error| {
            error.contains("Group start partially failed on 1 of 2 renderers")
                && error.contains("sonos:kitchen")
        }));

        let pause_message = state
            .pause_renderer(&renderer_location)
            .expect("partial pause should still succeed");
        assert_eq!(pause_message, "Group playback paused on 1 of 2 renderers.");
        let session = state
            .database
            .load_playback_session(&renderer_location)
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(session.transport_state, "PAUSED_PLAYBACK");
        assert!(session.last_error.as_deref().is_some_and(|error| {
            error.contains("Group pause partially failed on 1 of 2 renderers")
                && error.contains("sonos:kitchen")
        }));

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn renderer_group_total_fanout_failure_marks_queue_error() {
        let track = sample_track("track-1", Some(1), Some(1), "Track 1");
        let state = sample_state(vec![track.clone()]);
        let group = create_sample_renderer_group(&state, "Broken", &["sonos:kitchen", "sonos:den"]);
        let group_location = renderer_group_queue_key(&group.id);
        let queue = state
            .database
            .replace_queue(&group_location, "Broken", &[queue_entry_for_track(&track)])
            .expect("group queue should be created");

        let error = state
            .start_current_queue_entry(&group_location)
            .expect_err("total fan-out failure should fail playback");
        assert!(
            error
                .to_string()
                .contains("group playback failed on all members")
        );
        let session = state
            .database
            .load_playback_session(&group_location)
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(session.transport_state, "ERROR");
        assert_eq!(session.queue_entry_id, Some(queue.entries[0].id));
        assert!(session.last_error.as_deref().is_some_and(|error| {
            error.contains("sonos:kitchen") && error.contains("sonos:den")
        }));

        let _ = std::fs::remove_dir_all(state.config.config_path);
    }

    #[test]
    fn renderer_group_fans_out_playback_to_members() {
        let track = sample_track("track-1", Some(1), Some(1), "Track 1");
        let state = sample_state(vec![track.clone()]);
        let group = state
            .create_renderer_group(
                "Phones",
                &[
                    "android-local://phone-a".to_string(),
                    "android-local://phone-b".to_string(),
                ],
                None,
                None,
            )
            .expect("group should be created");
        let group_location = renderer_group_queue_key(&group.id);
        let queue = state
            .database
            .replace_queue(
                &group_location,
                "Phones",
                &[QueueMutationEntry {
                    track_id: track.id.clone(),
                    album_id: Some(track.album_id.clone()),
                    source_kind: "track".to_string(),
                    source_ref: Some(track.id.clone()),
                }],
            )
            .expect("group queue should be created");

        let (started_track, entry_id, renderer_name, renderer_location) = state
            .start_current_queue_entry(&group_location)
            .expect("group playback should start");
        assert_eq!(started_track.id, track.id);
        assert_eq!(entry_id, queue.entries[0].id);
        assert_eq!(renderer_name, "Phones (2 renderers)");
        assert_eq!(renderer_location, group_location);

        let queue = state
            .database
            .load_queue(&renderer_location)
            .expect("queue should load")
            .expect("queue should exist");
        assert_eq!(queue.status, "playing");
        let session = state
            .database
            .load_playback_session(&renderer_location)
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(session.transport_state, "PLAYING");
        assert_eq!(session.queue_entry_id, Some(entry_id));
        state
            .poll_renderer_group_queue(&renderer_location)
            .expect("local-only group polling should be a no-op");
        let session = state
            .database
            .load_playback_session(&renderer_location)
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(session.transport_state, "PLAYING");

        let pause_message = state
            .pause_renderer(&renderer_location)
            .expect("group pause should fan out");
        assert_eq!(pause_message, "Group playback paused on 2 renderers.");
        let session = state
            .database
            .load_playback_session(&renderer_location)
            .expect("session should load")
            .expect("session should exist");
        assert_eq!(session.transport_state, "PAUSED_PLAYBACK");

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
        first.artist = "J Dilla".to_string();
        first.album_artist = "Radiohead".to_string();
        first.album = "In Rainbows".to_string();
        first.album_id = stable_album_id(&first.album_artist, &first.album);

        let mut second = sample_track("track-2", Some(1), Some(1), "Song B");
        second.artist = "MF DOOM".to_string();
        second.album_artist = " radiohead ".to_string();
        second.album = "Kid A".to_string();
        second.album_id = stable_album_id(&second.album_artist, &second.album);

        let artists = build_artist_summaries(&[first, second]);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].track_count, 2);
        assert_eq!(artists[0].album_count, 2);
        assert_eq!(artists[0].id, stable_artist_id("Radiohead"));
    }

    #[test]
    fn library_albums_facet_omits_track_table() {
        let state = sample_state(vec![sample_track("track-1", Some(1), Some(1), "Song A")]);
        let mut query = HashMap::new();
        query.insert("facet".to_string(), "albums".to_string());
        let request = HttpRequest {
            method: "GET".to_string(),
            target: "/library?facet=albums".to_string(),
            path: "/library".to_string(),
            query,
            form: HashMap::new(),
            range_header: None,
            content_type: None,
            body: Vec::new(),
        };

        let html = render_library_page(&state, &request);

        assert!(html.contains("id=\"album_table\""));
        assert!(!html.contains("id=\"track_table\""));
        assert!(!html.contains("id=\"track_detail_panel\""));
        assert!(!html.contains("class=\"artist-list\""));
        assert!(html.contains("facet=tracks"));
    }

    #[test]
    fn library_tracks_facet_loads_rows_in_pages() {
        let tracks = (0..105)
            .map(|idx| {
                sample_track(
                    &format!("track-{idx}"),
                    Some(1),
                    Some(idx + 1),
                    &format!("Song {idx}"),
                )
            })
            .collect();
        let state = sample_state(tracks);
        let mut query = HashMap::new();
        query.insert("facet".to_string(), "tracks".to_string());
        let request = HttpRequest {
            method: "GET".to_string(),
            target: "/library?facet=tracks".to_string(),
            path: "/library".to_string(),
            query,
            form: HashMap::new(),
            range_header: None,
            content_type: None,
            body: Vec::new(),
        };

        let html = render_library_page(&state, &request);
        assert_eq!(html.matches("<tr data-search=").count(), 100);
        assert!(html.contains("data-library-loader"));
        assert!(html.contains("data-offset=\"100\""));

        let mut query = HashMap::new();
        query.insert("facet".to_string(), "tracks".to_string());
        query.insert("offset".to_string(), "100".to_string());
        let request = HttpRequest {
            method: "GET".to_string(),
            target: "/library/rows?facet=tracks&offset=100".to_string(),
            path: "/library/rows".to_string(),
            query,
            form: HashMap::new(),
            range_header: None,
            content_type: None,
            body: Vec::new(),
        };

        let json = render_library_rows_json(&state, &request);
        assert_eq!(json.matches("<tr data-search=").count(), 5);
        assert!(json.contains(r#""next_offset":105"#));
        assert!(json.contains(r#""has_more":false"#));
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
    fn likes_are_counted_once_per_client_and_item() {
        let track = sample_track("track-1", Some(1), Some(1), "Track 1");
        let album_id = track.album_id.clone();
        let state = sample_state(vec![track.clone()]);

        let first = state
            .like_item("track", &track.id, "client-a")
            .expect("first track like should save");
        assert!(first.created);
        assert_eq!(first.like_count, 1);

        let duplicate = state
            .like_item("track", &track.id, "client-a")
            .expect("duplicate track like should be idempotent");
        assert!(!duplicate.created);
        assert_eq!(duplicate.like_count, 1);

        let second_client = state
            .like_item("track", &track.id, "client-b")
            .expect("second client track like should save");
        assert!(second_client.created);
        assert_eq!(second_client.like_count, 2);

        let album_like = state
            .like_item("album", &album_id, "client-a")
            .expect("album like should save separately from track likes");
        assert!(album_like.created);
        assert_eq!(album_like.like_count, 1);
    }

    #[test]
    fn loads_recent_track_play_history_newest_first() {
        let config_path = temp_config_path("recent-track-play-history");
        let database = Database::open(&config_path).expect("database should open");
        let entries = (0..25)
            .map(|index| QueueMutationEntry {
                track_id: format!("track-{index:02}"),
                album_id: Some("album-1".to_string()),
                source_kind: "album".to_string(),
                source_ref: Some("album-1".to_string()),
            })
            .collect::<Vec<_>>();
        let queue = database
            .replace_queue("renderer-1", "Test Queue", &entries)
            .expect("queue should be created");

        for entry in &queue.entries {
            database
                .mark_queue_play_started(
                    "renderer-1",
                    entry.id,
                    &entry.track_id,
                    &format!("http://musicd.local/stream/track/{}", entry.track_id),
                    Some(180),
                )
                .expect("play should be recorded");
        }

        let history = database
            .load_recent_track_play_history(20)
            .expect("recent history should load");
        assert_eq!(history.len(), 20);
        assert_eq!(history[0].track_id, "track-24");
        assert_eq!(history[19].track_id, "track-05");
        assert!(history.windows(2).all(|window| window[0].id > window[1].id));

        let _ = std::fs::remove_dir_all(config_path);
    }

    #[test]
    fn persists_normalized_albums_and_artists() {
        let config_path = temp_config_path("normalized-library");
        let database = Database::open(&config_path).expect("database should open");

        let mut first = sample_track("track-1", Some(1), Some(1), "15 Step");
        first.artist = "Radiohead".to_string();
        first.album_artist = "Radiohead".to_string();
        first.album = "In Rainbows".to_string();
        first.album_id = stable_album_id(&first.album_artist, &first.album);
        first.artwork = Some(TrackArtwork {
            cache_key: "cover.jpg".to_string(),
            source: "Embedded artwork".to_string(),
            mime_type: "image/jpeg".to_string(),
        });
        first.metadata.musicbrainz_release_id =
            Some("6b6a3457-253e-4539-aee3-6279adf66c92".to_string());
        first.metadata.musicbrainz_release_group_id =
            Some("b1392450-e666-3926-a536-22c65f834433".to_string());
        first.metadata.release_date = Some("2007-10-10".to_string());
        first.metadata.release_country = Some("GB".to_string());
        first.metadata.release_type = Some("album".to_string());
        first.metadata.genres = vec!["Art Rock".to_string(), "Alternative".to_string()];

        let mut second = sample_track("track-2", Some(1), Some(2), "Bodysnatchers");
        second.artist = "Radiohead".to_string();
        second.album_artist = "Radiohead".to_string();
        second.album = "In Rainbows".to_string();
        second.album_id = stable_album_id(&second.album_artist, &second.album);

        let mut third = sample_track("track-3", Some(1), Some(1), "Everything In Its Right Place");
        third.artist = "Radiohead".to_string();
        third.album_artist = "Radiohead".to_string();
        third.album = "Kid A".to_string();
        third.album_id = stable_album_id(&third.album_artist, &third.album);

        let library = Library::build(
            PathBuf::from("/music"),
            vec![first.clone(), second.clone(), third.clone()],
            &[],
        );

        database
            .save_library(&library)
            .expect("library should be persisted");

        let tracks = database
            .load_library(PathBuf::from("/music"))
            .expect("library should reload")
            .tracks;
        let reloaded_first = tracks
            .iter()
            .find(|track| track.id == "track-1")
            .expect("track should reload");
        assert_eq!(
            reloaded_first.metadata.musicbrainz_release_id.as_deref(),
            Some("6b6a3457-253e-4539-aee3-6279adf66c92")
        );
        assert_eq!(
            reloaded_first.metadata.genres,
            vec!["Art Rock".to_string(), "Alternative".to_string()]
        );

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
        assert_eq!(
            in_rainbows.metadata.musicbrainz_release_id.as_deref(),
            Some("6b6a3457-253e-4539-aee3-6279adf66c92")
        );
        assert_eq!(
            in_rainbows.metadata.musicbrainz_release_group_id.as_deref(),
            Some("b1392450-e666-3926-a536-22c65f834433")
        );
        assert_eq!(
            in_rainbows.metadata.source_track_id.as_deref(),
            Some("track-1")
        );
        assert_eq!(
            in_rainbows.metadata.genres,
            vec!["Art Rock".to_string(), "Alternative".to_string()]
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
    fn incremental_library_changes_update_snapshot_and_database() {
        let mut updated = sample_track("track-updated", Some(1), Some(1), "Old title");
        updated.relative_path = "Artist/Album/01.flac".to_string();
        updated.path = PathBuf::from("/music/Artist/Album/01.flac");
        let mut removed = sample_track("track-removed", Some(1), Some(2), "Removed");
        removed.relative_path = "Artist/Album/02.flac".to_string();
        removed.path = PathBuf::from("/music/Artist/Album/02.flac");

        let state = sample_state(vec![updated.clone(), removed.clone()]);

        updated.title = "New title".to_string();
        updated.file_size = 456;
        updated.modified_unix_millis = 99;
        let mut added = sample_track("track-added", Some(1), Some(3), "Added");
        added.relative_path = "Artist/Album/03.flac".to_string();
        added.path = PathBuf::from("/music/Artist/Album/03.flac");

        let summary = state
            .apply_library_file_changes(
                vec![updated.clone(), added.clone()],
                vec![removed.relative_path.clone()],
            )
            .expect("incremental changes should apply");

        assert_eq!(summary.upserted, 2);
        assert_eq!(summary.removed, 1);
        assert_eq!(state.track_count(), 2);
        assert_eq!(
            state.find_track(&updated.id).map(|track| track.title),
            Some("New title".to_string())
        );
        assert!(state.find_track(&removed.id).is_none());

        let persisted = state
            .database
            .load_library(PathBuf::from("/music"))
            .expect("library should reload");
        assert_eq!(persisted.tracks.len(), 2);
        assert!(persisted.track_index.contains_key(&updated.id));
        assert!(persisted.track_index.contains_key(&added.id));
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
                visibility: "public".to_string(),
                owner_client_id: None,
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
            visibility: "public".to_string(),
            owner_client_id: None,
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
            visibility: "public".to_string(),
            owner_client_id: None,
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
            album_artist: "Artist".to_string(),
            disc_number,
            track_number,
            duration_seconds: Some(180),
            relative_path: format!("{title}.flac"),
            path: PathBuf::from(format!("/music/{title}.flac")),
            mime_type: "audio/flac".to_string(),
            file_size: 123,
            modified_unix_millis: 0,
            artwork: None,
            metadata: Default::default(),
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
                server_discovery_enabled: true,
                default_renderer_location: None,
                radio_browser_base_url: "https://de1.api.radio-browser.info".to_string(),
                debug_mode: false,
                skip_startup_scan: false,
                library_watch_enabled: true,
                library_watch_interval_ms: 10_000,
                library_watch_settle_ms: 3_000,
            },
            database,
            library: arc_swap::ArcSwap::from_pointee(Library::build(
                PathBuf::from("/music"),
                tracks,
                &[],
            )),
            renderer_backends: RendererBackends::default(),
            metrics: OnceLock::new(),
            events: crate::service::PlaybackEvents::new(),
            rescan_state: crate::service::RescanState::new(),
        }
    }

    fn sample_state_with_backend(
        tracks: Vec<LibraryTrack>,
        backend: Arc<dyn RendererBackend>,
    ) -> ServiceState {
        let mut state = sample_state(tracks);
        state.renderer_backends.test = Some(backend);
        state
    }

    fn stopped_near_end_snapshot(
        track: &LibraryTrack,
        position_seconds: u64,
        duration_seconds: u64,
    ) -> TransportSnapshot {
        transport_snapshot("STOPPED", track, position_seconds, duration_seconds)
    }

    fn completed_near_end_snapshot(
        track: &LibraryTrack,
        position_seconds: u64,
        duration_seconds: u64,
    ) -> TransportSnapshot {
        transport_snapshot("COMPLETED", track, position_seconds, duration_seconds)
    }

    fn playing_snapshot(
        track: &LibraryTrack,
        position_seconds: u64,
        duration_seconds: u64,
    ) -> TransportSnapshot {
        transport_snapshot("PLAYING", track, position_seconds, duration_seconds)
    }

    fn transport_snapshot(
        transport_state: &str,
        track: &LibraryTrack,
        position_seconds: u64,
        duration_seconds: u64,
    ) -> TransportSnapshot {
        TransportSnapshot {
            transport_info: TransportInfo {
                transport_state: transport_state.to_string(),
                transport_status: Some("OK".to_string()),
                current_speed: Some("1".to_string()),
            },
            position_info: PositionInfo {
                track_uri: Some(format!(
                    "http://192.168.1.10:7878/stream/track/{}",
                    track.id
                )),
                rel_time_seconds: Some(position_seconds),
                track_duration_seconds: Some(duration_seconds),
            },
        }
    }

    struct FakeRendererBackend {
        renderer: RendererRecord,
        snapshots: Mutex<VecDeque<TransportSnapshot>>,
        played_streams: Mutex<Vec<StreamResource>>,
        preloaded_streams: Mutex<Vec<StreamResource>>,
        seek_positions: Mutex<Vec<u64>>,
        cleared_next_count: Mutex<usize>,
        play_count: Mutex<usize>,
    }

    impl FakeRendererBackend {
        fn new(renderer_location: &str, snapshots: Vec<TransportSnapshot>) -> Self {
            Self {
                renderer: RendererRecord {
                    location: renderer_location.to_string(),
                    name: "Fake Renderer".to_string(),
                    manufacturer: Some("musicd test".to_string()),
                    model_name: Some("Queue Harness".to_string()),
                    av_transport_control_url: Some("http://renderer.local/avtransport".to_string()),
                    capabilities: RendererCapabilities {
                        av_transport_actions: Some(vec![
                            "Play".to_string(),
                            "Pause".to_string(),
                            "Stop".to_string(),
                            "Seek".to_string(),
                            "SetNextAVTransportURI".to_string(),
                        ]),
                        has_playlist_extension_service: Some(false),
                    },
                    visibility: "public".to_string(),
                    owner_client_id: None,
                    last_checked_unix: 1,
                    last_reachable_unix: Some(1),
                    last_error: None,
                    last_seen_unix: 1,
                },
                snapshots: Mutex::new(VecDeque::from(snapshots)),
                played_streams: Mutex::new(Vec::new()),
                preloaded_streams: Mutex::new(Vec::new()),
                seek_positions: Mutex::new(Vec::new()),
                cleared_next_count: Mutex::new(0),
                play_count: Mutex::new(0),
            }
        }

        fn played_streams(&self) -> Vec<StreamResource> {
            self.played_streams
                .lock()
                .expect("played streams should not be poisoned")
                .clone()
        }

        fn cleared_next_count(&self) -> usize {
            *self
                .cleared_next_count
                .lock()
                .expect("cleared next count should not be poisoned")
        }

        fn play_count(&self) -> usize {
            *self
                .play_count
                .lock()
                .expect("play count should not be poisoned")
        }

        fn seek_positions(&self) -> Vec<u64> {
            self.seek_positions
                .lock()
                .expect("seek positions should not be poisoned")
                .clone()
        }
    }

    impl RendererBackend for FakeRendererBackend {
        fn resolve_renderer(
            &self,
            cached: Option<&RendererRecord>,
            _renderer_location: &str,
        ) -> std::io::Result<RendererRecord> {
            Ok(cached.cloned().unwrap_or_else(|| self.renderer.clone()))
        }

        fn play_stream(
            &self,
            _renderer: &RendererRecord,
            resource: &StreamResource,
        ) -> std::io::Result<()> {
            self.played_streams
                .lock()
                .expect("played streams should not be poisoned")
                .push(resource.clone());
            Ok(())
        }

        fn preload_next(
            &self,
            _renderer: &RendererRecord,
            resource: &StreamResource,
        ) -> std::io::Result<()> {
            self.preloaded_streams
                .lock()
                .expect("preloaded streams should not be poisoned")
                .push(resource.clone());
            Ok(())
        }

        fn clear_next(&self, _renderer: &RendererRecord) -> std::io::Result<()> {
            *self
                .cleared_next_count
                .lock()
                .expect("cleared next count should not be poisoned") += 1;
            Ok(())
        }

        fn play(&self, _renderer: &RendererRecord) -> std::io::Result<()> {
            *self
                .play_count
                .lock()
                .expect("play count should not be poisoned") += 1;
            Ok(())
        }

        fn pause(&self, _renderer: &RendererRecord) -> std::io::Result<()> {
            Ok(())
        }

        fn stop(&self, _renderer: &RendererRecord) -> std::io::Result<()> {
            Ok(())
        }

        fn next(&self, _renderer: &RendererRecord) -> std::io::Result<()> {
            Ok(())
        }

        fn previous(&self, _renderer: &RendererRecord) -> std::io::Result<()> {
            Ok(())
        }

        fn seek(&self, _renderer: &RendererRecord, position_seconds: u64) -> std::io::Result<()> {
            self.seek_positions
                .lock()
                .expect("seek positions should not be poisoned")
                .push(position_seconds);
            Ok(())
        }

        fn transport_snapshot(
            &self,
            _renderer: &RendererRecord,
        ) -> std::io::Result<TransportSnapshot> {
            self.snapshots
                .lock()
                .expect("snapshots should not be poisoned")
                .pop_front()
                .ok_or_else(|| std::io::Error::other("fake renderer has no snapshot"))
        }
    }

    fn renderer_locations(locations: &[&str]) -> Vec<String> {
        locations
            .iter()
            .map(|location| (*location).to_string())
            .collect()
    }

    fn create_sample_renderer_group(
        state: &ServiceState,
        name: &str,
        members: &[&str],
    ) -> RendererGroup {
        state
            .create_renderer_group(name, &renderer_locations(members), None, None)
            .expect("sample renderer group should be created")
    }

    fn assert_group_members(group: &RendererGroup, expected: &[&str]) {
        assert_eq!(
            group
                .members
                .iter()
                .map(|member| member.renderer_location.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            group
                .members
                .iter()
                .map(|member| member.position)
                .collect::<Vec<_>>(),
            (1..=i64::try_from(expected.len()).unwrap_or(i64::MAX)).collect::<Vec<_>>()
        );
    }

    fn queue_entry_for_track(track: &LibraryTrack) -> QueueMutationEntry {
        QueueMutationEntry {
            track_id: track.id.clone(),
            album_id: Some(track.album_id.clone()),
            source_kind: "track".to_string(),
            source_ref: Some(track.id.clone()),
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
