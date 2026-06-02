use std::io;
use std::time::Duration;

use musicd_upnp::{StreamResource, TransportSnapshot};

use crate::renderer::{
    RendererKind, renderer_group_id_from_location, renderer_group_queue_key,
    renderer_kind_for_location,
};
use crate::types::{PlaybackQueue, PlaybackSession, RendererGroup, RendererRecord};

use super::ServiceState;
use super::poll::{
    next_queue_entry_after, queue_status_for_transport, should_adopt_preloaded_next_entry,
    should_auto_advance,
};

pub(crate) struct GroupFanOutResult {
    succeeded: Vec<RendererRecord>,
    failed: Vec<String>,
    total: usize,
}

impl GroupFanOutResult {
    pub(crate) fn succeeded_count(&self) -> usize {
        self.succeeded.len()
    }

    pub(crate) fn total_count(&self) -> usize {
        self.total
    }

    pub(crate) fn warning_message(&self, action_label: &str) -> Option<String> {
        (!self.failed.is_empty()).then(|| {
            format!(
                "Group {action_label} partially failed on {} of {} renderers: {}",
                self.failed.len(),
                self.total,
                self.failed.join("; ")
            )
        })
    }
}

impl ServiceState {
    pub(crate) fn renderer_group_snapshot(&self) -> Vec<RendererGroup> {
        self.database.list_renderer_groups().unwrap_or_default()
    }

    pub(crate) fn create_renderer_group(
        &self,
        name: &str,
        members: &[String],
        source_renderer_location: Option<&str>,
        client_id: Option<&str>,
    ) -> io::Result<RendererGroup> {
        reject_nested_group_members(members)?;
        self.check_private_renderer_additions_owned(members, &[], client_id)?;
        let trimmed_source = source_renderer_location
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let (source_queue, source_session) = match trimmed_source {
            Some(source) => {
                self.check_direct_renderer_access(source, client_id)?;
                let queue = self.database.load_queue(source)?;
                let session = self.playback_session(source);
                (queue, session)
            }
            None => (None, None),
        };

        let group = self.database.create_renderer_group(name, members)?;
        let group_queue_key = renderer_group_queue_key(&group.id);
        match (trimmed_source, source_queue.as_ref()) {
            (Some(source), Some(source_queue)) => {
                // Move (rather than copy) so entry_status / completed_unix /
                // session state survive — copying via replace_queue would
                // resurface already-played tracks under "Up Next".
                self.database
                    .move_queue(source, &group_queue_key, Some(&source_queue.name))?;
            }
            _ => {
                self.database
                    .replace_queue(&group_queue_key, &group.name, &[])?;
            }
        }

        if let (Some(source), Some(_source_queue), Some(source_session)) =
            (trimmed_source, source_queue, source_session)
        {
            if let Err(error) = self.sync_active_source_into_group(
                source,
                &source_session,
                &group,
                &group_queue_key,
            ) {
                eprintln!(
                    "[musicd][group-create-sync] group={} source={} error={}",
                    group.id, source, error
                );
            }
        }

        Ok(group)
    }

    pub(crate) fn update_renderer_group_by_queue_key(
        &self,
        renderer_location: &str,
        name: &str,
        members: &[String],
        client_id: Option<&str>,
    ) -> io::Result<RendererGroup> {
        reject_nested_group_members(members)?;
        let group_before = self.load_renderer_group_for_queue(renderer_location)?;
        let existing_members = group_before
            .members
            .iter()
            .map(|member| member.renderer_location.clone())
            .collect::<Vec<_>>();
        self.check_private_renderer_additions_owned(members, &existing_members, client_id)?;
        let updated_group = self
            .database
            .update_renderer_group(&group_before.id, name, members)?;

        let newly_added: Vec<String> = updated_group
            .members
            .iter()
            .filter(|member| {
                !existing_members
                    .iter()
                    .any(|previous| previous == &member.renderer_location)
            })
            .map(|member| member.renderer_location.clone())
            .collect();

        eprintln!(
            "[musicd][group-update] group={} new_members={:?} existing={:?}",
            updated_group.id, newly_added, existing_members
        );

        if !newly_added.is_empty() {
            if let Err(error) =
                self.sync_new_group_members(renderer_location, &updated_group, &newly_added)
            {
                eprintln!(
                    "[musicd][group-add-sync] group={} error={}",
                    updated_group.id, error
                );
            }
        }

        Ok(updated_group)
    }

    pub(crate) fn load_renderer_group_for_queue(
        &self,
        renderer_location: &str,
    ) -> io::Result<RendererGroup> {
        self.database
            .load_renderer_group_by_queue_key(renderer_location)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "renderer group not found"))
    }

    pub(crate) fn delete_renderer_group_by_queue_key(
        &self,
        renderer_location: &str,
        inheritor_renderer_location: Option<&str>,
    ) -> io::Result<RendererGroup> {
        let group = self.load_renderer_group_for_queue(renderer_location)?;

        if let Some(inheritor) = inheritor_renderer_location
            .map(str::trim)
            .filter(|loc| !loc.is_empty())
        {
            if let Err(error) =
                self.transfer_group_playback_to_member(renderer_location, &group, inheritor)
            {
                eprintln!(
                    "[musicd][group-delete-sync] group={} inheritor={} error={}",
                    group.id, inheritor, error
                );
            }
        }

        if self.database.delete_renderer_group(&group.id)? {
            Ok(group)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "renderer group not found",
            ))
        }
    }

    pub(crate) fn play_stream_on_group_members(
        &self,
        group: &RendererGroup,
        resource: &StreamResource,
    ) -> io::Result<GroupFanOutResult> {
        let mut started = Vec::new();
        let mut errors = Vec::new();
        for member in &group.members {
            let renderer_location = &member.renderer_location;
            match self.play_stream_on_group_member(renderer_location, resource) {
                Ok(renderer) => started.push(renderer),
                Err(error) => errors.push(format!("{renderer_location}: {error}")),
            }
        }

        if started.is_empty() {
            return Err(io::Error::other(format!(
                "group playback failed on all members: {}",
                errors.join("; ")
            )));
        }

        let result = GroupFanOutResult {
            succeeded: started,
            failed: errors,
            total: group.members.len(),
        };
        self.record_group_fanout_warning(group, "start", &result);
        Ok(result)
    }

    pub(crate) fn fan_out_group_transport_action(
        &self,
        group: &RendererGroup,
        action_label: &str,
        apply: impl Fn(&ServiceState, &str, &RendererRecord) -> io::Result<()>,
    ) -> io::Result<GroupFanOutResult> {
        let mut succeeded = Vec::new();
        let mut errors = Vec::new();
        for member in &group.members {
            let renderer_location = &member.renderer_location;
            match self
                .resolve_renderer(renderer_location)
                .and_then(|renderer| {
                    apply(self, renderer_location, &renderer)?;
                    Ok(renderer)
                }) {
                Ok(renderer) => {
                    let _ = self.mark_renderer_reachable(&renderer);
                    succeeded.push(renderer);
                }
                Err(error) => {
                    let _ = self.mark_group_member_unreachable(renderer_location, &error);
                    errors.push(format!("{renderer_location}: {error}"));
                }
            }
        }

        if succeeded.is_empty() {
            return Err(io::Error::other(format!(
                "group {action_label} failed on all members: {}",
                errors.join("; ")
            )));
        }

        let result = GroupFanOutResult {
            succeeded,
            failed: errors,
            total: group.members.len(),
        };
        self.record_group_fanout_warning(group, action_label, &result);
        Ok(result)
    }

    pub(crate) fn record_group_session_warning(
        &self,
        renderer_location: &str,
        action_label: &str,
        result: &GroupFanOutResult,
    ) {
        if let Some(warning) = result.warning_message(action_label) {
            let _ = self
                .database
                .record_playback_session_warning(renderer_location, &warning);
        }
    }

    pub(crate) fn group_fanout_message(
        &self,
        action_label: &str,
        result: &GroupFanOutResult,
    ) -> String {
        if result.succeeded_count() == result.total_count() {
            format!(
                "Group playback {action_label} on {} renderers.",
                result.succeeded_count()
            )
        } else {
            format!(
                "Group playback {action_label} on {} of {} renderers.",
                result.succeeded_count(),
                result.total_count()
            )
        }
    }

    fn record_group_fanout_warning(
        &self,
        group: &RendererGroup,
        action_label: &str,
        result: &GroupFanOutResult,
    ) {
        if let Some(warning) = result.warning_message(action_label) {
            self.debug_log(
                "group-partial-action",
                format!(
                    "group={} action={} warning={}",
                    group.id, action_label, warning
                ),
            );
        }
    }

    pub(crate) fn preload_next_group_queue_entry(
        &self,
        group_location: &str,
        group: &RendererGroup,
        queue: &PlaybackQueue,
        current_entry_id: i64,
        force_clear_no_successor: bool,
    ) -> io::Result<()> {
        let session = self.playback_session(group_location);
        if group.members.iter().any(|member| {
            self.resolve_renderer(&member.renderer_location)
                .map(|renderer| renderer.capabilities.has_playlist_extension_service == Some(true))
                .unwrap_or(false)
        }) {
            if session
                .as_ref()
                .and_then(|session| session.next_queue_entry_id)
                .is_some()
            {
                self.database
                    .mark_next_queue_entry_preloaded(group_location, None)?;
            }
            self.debug_log(
                "group-preload-next-skipped",
                format!(
                    "renderer={} reason=playlist-extension-queue current_entry={} force_clear_no_successor={}",
                    group_location, current_entry_id, force_clear_no_successor
                ),
            );
            return Ok(());
        }
        if !self.config.native_next_preload_enabled {
            let had_preloaded_next = session
                .as_ref()
                .and_then(|session| session.next_queue_entry_id)
                .is_some();
            if force_clear_no_successor || had_preloaded_next {
                let cleared = self.clear_group_next_queue_entry(
                    group_location,
                    group,
                    "native-next-disabled",
                );
                if cleared {
                    self.database
                        .mark_next_queue_entry_preloaded(group_location, None)?;
                }
            }
            self.debug_log(
                "group-preload-next-skipped",
                format!("renderer={} reason=native-next-disabled", group_location),
            );
            return Ok(());
        }

        let Some(next_entry) = next_queue_entry_after(queue, current_entry_id) else {
            let had_preloaded_next = self
                .playback_session(group_location)
                .and_then(|session| session.next_queue_entry_id)
                .is_some();
            if force_clear_no_successor || had_preloaded_next {
                let cleared =
                    self.clear_group_next_queue_entry(group_location, group, "no-successor");
                if cleared {
                    self.database
                        .mark_next_queue_entry_preloaded(group_location, None)?;
                }
            }
            return Ok(());
        };

        if session
            .as_ref()
            .and_then(|session| session.next_queue_entry_id)
            == Some(next_entry.id)
        {
            return Ok(());
        }

        let track = self.find_track(&next_entry.track_id).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "queued next track not found")
        })?;
        let resource = self.stream_resource_for_track(&track);
        let mut preloaded = 0usize;
        let mut errors = Vec::new();
        for member in &group.members {
            let renderer_location = &member.renderer_location;
            let renderer = match self.resolve_renderer(renderer_location) {
                Ok(renderer) => renderer,
                Err(error) => {
                    errors.push(format!("{renderer_location}: {error}"));
                    continue;
                }
            };
            if renderer.capabilities.has_playlist_extension_service == Some(true) {
                continue;
            }
            if renderer.capabilities.supports_set_next_av_transport_uri() == Some(false) {
                continue;
            }
            match self.run_renderer_action_with_private_queue_log(
                renderer_location,
                &renderer,
                "group-set-next-avtransport-uri",
                |backend| backend.preload_next(&renderer, &resource),
            ) {
                Ok(()) => {
                    preloaded += 1;
                    let _ = self.mark_renderer_reachable(&renderer);
                }
                Err(error) => {
                    let _ = self.mark_group_member_unreachable(renderer_location, &error);
                    errors.push(format!("{renderer_location}: {error}"));
                }
            }
        }

        self.database.mark_next_queue_entry_preloaded(
            group_location,
            (preloaded > 0).then_some(next_entry.id),
        )?;
        if preloaded > 0 || errors.is_empty() {
            return Ok(());
        }
        Err(io::Error::other(format!(
            "group next-track preload failed: {}",
            errors.join("; ")
        )))
    }

    fn clear_group_next_queue_entry(
        &self,
        group_location: &str,
        group: &RendererGroup,
        reason: &str,
    ) -> bool {
        let mut failed = false;
        for member in &group.members {
            let renderer_location = &member.renderer_location;
            let renderer = match self.resolve_renderer(renderer_location) {
                Ok(renderer) => renderer,
                Err(error) => {
                    failed = true;
                    self.debug_log(
                        "group-clear-next-failed",
                        format!(
                            "group={} renderer={} reason={} resolve_error={}",
                            group_location, renderer_location, reason, error
                        ),
                    );
                    continue;
                }
            };
            if renderer.capabilities.has_playlist_extension_service == Some(true) {
                self.debug_log(
                    "group-clear-next-skipped",
                    format!(
                        "group={} renderer={} reason={} skipped_reason=playlist-extension-queue",
                        group_location, renderer_location, reason
                    ),
                );
                continue;
            }
            if renderer.capabilities.supports_set_next_av_transport_uri() == Some(false) {
                continue;
            }
            match self.run_renderer_action_with_private_queue_log(
                renderer_location,
                &renderer,
                "group-clear-next-avtransport-uri",
                |backend| backend.clear_next(&renderer),
            ) {
                Ok(()) => {
                    let _ = self.mark_renderer_reachable(&renderer);
                    self.debug_log(
                        "group-clear-next",
                        format!(
                            "group={} renderer={} reason={}",
                            group_location, renderer_location, reason
                        ),
                    );
                }
                Err(error) => {
                    failed = true;
                    self.debug_log(
                        "group-clear-next-failed",
                        format!(
                            "group={} renderer={} reason={} error={}",
                            group_location, renderer_location, reason, error
                        ),
                    );
                }
            }
        }
        !failed
    }

    pub(crate) fn poll_renderer_group_queue(&self, group_location: &str) -> io::Result<()> {
        let mut queue = match self.queue_snapshot(group_location) {
            Some(queue) => queue,
            None => return Ok(()),
        };
        let session = self.playback_session(group_location);
        let group = self.load_renderer_group_for_queue(group_location)?;
        let snapshot = match self.group_leader_transport_snapshot(&group) {
            Ok(snapshot) => snapshot,
            Err(error) if error.kind() == io::ErrorKind::Unsupported => return Ok(()),
            Err(error) => {
                let _ = self
                    .database
                    .record_transport_poll_error(group_location, &error.to_string());
                self.debug_log(
                    "group-queue-poll-error",
                    format!(
                        "renderer={} members={} queue_status={} current_entry={:?} session_state={} error={}",
                        group_location,
                        group.members.len(),
                        queue.status,
                        queue.current_entry_id,
                        session
                            .as_ref()
                            .map(|session| session.transport_state.as_str())
                            .unwrap_or("<none>"),
                        error
                    ),
                );
                return Err(error);
            }
        };

        self.database.record_transport_snapshot(
            group_location,
            &snapshot.transport_info.transport_state,
            snapshot.position_info.track_uri.as_deref(),
            snapshot.position_info.rel_time_seconds,
            snapshot.position_info.track_duration_seconds,
        )?;
        self.database.sync_queue_status(
            group_location,
            queue_status_for_transport(&snapshot.transport_info.transport_state),
        )?;
        self.debug_log(
            "group-queue-poll-snapshot",
            format!(
                "renderer={} members={} state={} status={} queue_status={} queue_version={} current_entry={:?} next_entry={:?} session_state={} uri={:?} position={:?} duration={:?}",
                group_location,
                group.members.len(),
                snapshot.transport_info.transport_state,
                snapshot
                    .transport_info
                    .transport_status
                    .as_deref()
                    .unwrap_or("<none>"),
                queue.status,
                queue.version,
                queue.current_entry_id,
                session.as_ref().and_then(|session| session.next_queue_entry_id),
                session
                    .as_ref()
                    .map(|session| session.transport_state.as_str())
                    .unwrap_or("<none>"),
                snapshot.position_info.track_uri,
                snapshot.position_info.rel_time_seconds,
                snapshot.position_info.track_duration_seconds
            ),
        );
        let previous_state = session
            .as_ref()
            .map(|session| session.transport_state.as_str())
            .unwrap_or("<none>");
        if snapshot.transport_info.transport_state == "PAUSED_PLAYBACK"
            && previous_state != "PAUSED_PLAYBACK"
        {
            self.debug_log(
                "group-renderer-pause-observed",
                format!(
                    "renderer={} members={} previous_state={} queue_status={} current_entry={:?} uri={:?} position={:?} duration={:?}",
                    group_location,
                    group.members.len(),
                    previous_state,
                    queue.status,
                    queue.current_entry_id,
                    snapshot.position_info.track_uri,
                    snapshot.position_info.rel_time_seconds,
                    snapshot.position_info.track_duration_seconds
                ),
            );
        }
        if queue.status == "playing"
            && !matches!(
                snapshot.transport_info.transport_state.as_str(),
                "PLAYING" | "TRANSITIONING"
            )
        {
            self.debug_log(
                "group-renderer-nonplaying-observed",
                format!(
                    "renderer={} members={} observed_state={} previous_state={} queue_status={} current_entry={:?} uri={:?} position={:?} duration={:?}",
                    group_location,
                    group.members.len(),
                    snapshot.transport_info.transport_state,
                    previous_state,
                    queue.status,
                    queue.current_entry_id,
                    snapshot.position_info.track_uri,
                    snapshot.position_info.rel_time_seconds,
                    snapshot.position_info.track_duration_seconds
                ),
            );
        }

        let adopted_renderer_advance =
            self.adopt_group_advanced_entry(group_location, &queue, &snapshot)?;
        if adopted_renderer_advance {
            self.debug_log(
                "group-queue-adopt-renderer-advance",
                format!(
                    "renderer={} previous_entry={:?} reported_uri={:?}",
                    group_location, queue.current_entry_id, snapshot.position_info.track_uri
                ),
            );
            if let Some(updated_queue) = self.queue_snapshot(group_location) {
                queue = updated_queue;
            }
        }

        if let Some(current_entry_id) = queue.current_entry_id.filter(|_| {
            matches!(
                snapshot.transport_info.transport_state.as_str(),
                "PLAYING" | "TRANSITIONING"
            )
        }) {
            let adopted_final_entry = adopted_renderer_advance
                && next_queue_entry_after(&queue, current_entry_id).is_none();
            if !adopted_final_entry {
                if let Err(error) = self.preload_next_group_queue_entry(
                    group_location,
                    &group,
                    &queue,
                    current_entry_id,
                    false,
                ) {
                    eprintln!(
                        "group next-track preload refresh failed for {group_location}: {error}"
                    );
                }
            }
        }

        if should_auto_advance(&queue, session.as_ref(), &snapshot, self) {
            self.debug_log(
                "group-auto-advance",
                format!(
                    "renderer={} current_entry={:?} position={:?} duration={:?}",
                    group_location,
                    queue.current_entry_id,
                    snapshot.position_info.rel_time_seconds,
                    snapshot.position_info.track_duration_seconds
                ),
            );
            let next_entry_id = self
                .database
                .advance_queue_after_completion(group_location)?;
            if next_entry_id.is_some() {
                let _ = self.start_current_queue_entry(group_location)?;
            }
            self.events.touch(group_location);
        }

        Ok(())
    }

    fn sync_active_source_into_group(
        &self,
        source_location: &str,
        source_session: &PlaybackSession,
        group: &RendererGroup,
        group_queue_key: &str,
    ) -> io::Result<()> {
        if !matches!(
            source_session.transport_state.as_str(),
            "PLAYING" | "TRANSITIONING"
        ) {
            eprintln!(
                "[musicd][group-create-sync] group={} source={} skip reason=source_state={}",
                group.id, source_location, source_session.transport_state
            );
            self.events.touch(group_queue_key);
            self.events.touch(source_location);
            return Ok(());
        }

        // The queue (and its session) was just moved to group_queue_key, so re-read
        // from there to find the now-current entry.
        let Some(group_queue) = self.queue_snapshot(group_queue_key) else {
            return Ok(());
        };
        let Some(group_current_id) = group_queue.current_entry_id else {
            return Ok(());
        };
        let Some(group_current) = group_queue
            .entries
            .iter()
            .find(|entry| entry.id == group_current_id)
        else {
            return Ok(());
        };

        let Some(track) = self.find_track(&group_current.track_id) else {
            return Ok(());
        };
        let resource = self.stream_resource_for_track(&track);
        let target_position = source_session.position_seconds;

        eprintln!(
            "[musicd][group-create-sync] group={} source={} entry={} track={} target_position={:?} stream_url={}",
            group.id,
            source_location,
            group_current.id,
            track.title,
            target_position,
            resource.stream_url
        );

        for member in &group.members {
            if member.renderer_location == source_location {
                continue;
            }
            eprintln!(
                "[musicd][group-create-sync] group={} member={} target_position={:?} starting catch_up",
                group.id, member.renderer_location, target_position
            );
            if let Err(error) =
                self.catch_up_group_member(&member.renderer_location, &resource, target_position)
            {
                eprintln!(
                    "[musicd][group-create-sync] group={} member={} error={}",
                    group.id, member.renderer_location, error
                );
            }
        }

        self.events.touch(group_queue_key);
        self.events.touch(source_location);
        Ok(())
    }

    fn transfer_group_playback_to_member(
        &self,
        group_location: &str,
        group: &RendererGroup,
        inheritor: &str,
    ) -> io::Result<()> {
        if !group
            .members
            .iter()
            .any(|member| member.renderer_location == inheritor)
        {
            eprintln!(
                "[musicd][group-delete-sync] group={} inheritor={} skip reason=not_a_member",
                group.id, inheritor
            );
            return Ok(());
        }

        let Some(group_queue) = self.queue_snapshot(group_location) else {
            return Ok(());
        };
        if group_queue.entries.is_empty() {
            return Ok(());
        }

        let group_session = self.playback_session(group_location);

        // Move (rather than copy) so entry_status / completed_unix / session state
        // survive the transfer. Uses the existing queue name unless we want to rename.
        self.database
            .move_queue(group_location, inheritor, Some(&group_queue.name))?;

        eprintln!(
            "[musicd][group-delete-sync] group={} inheritor={} entries={} status={:?}",
            group.id,
            inheritor,
            group_queue.entries.len(),
            group_session
                .as_ref()
                .map(|session| session.transport_state.as_str())
        );

        self.events.touch(inheritor);
        Ok(())
    }

    fn play_stream_on_group_member(
        &self,
        renderer_location: &str,
        resource: &StreamResource,
    ) -> io::Result<RendererRecord> {
        let renderer = self.resolve_renderer(renderer_location)?;
        if let Err(error) = self.run_renderer_action_with_private_queue_log(
            renderer_location,
            &renderer,
            "group-set-avtransport-uri-play",
            |backend| backend.play_stream(&renderer, resource),
        ) {
            let _ = self.mark_group_member_unreachable(renderer_location, &error);
            return Err(error);
        }
        let _ = self.mark_renderer_reachable(&renderer);
        Ok(renderer)
    }

    fn sync_new_group_members(
        &self,
        group_location: &str,
        group: &RendererGroup,
        new_member_locations: &[String],
    ) -> io::Result<()> {
        let Some(queue) = self.queue_snapshot(group_location) else {
            return Ok(());
        };
        let Some(current_entry_id) = queue.current_entry_id else {
            return Ok(());
        };
        let Some(current_entry) = queue
            .entries
            .iter()
            .find(|entry| entry.id == current_entry_id)
        else {
            return Ok(());
        };
        let Some(track) = self.find_track(&current_entry.track_id) else {
            return Ok(());
        };

        let snapshot = self.group_leader_transport_snapshot(group).ok();
        let leader_state = snapshot
            .as_ref()
            .map(|snapshot| snapshot.transport_info.transport_state.as_str())
            .unwrap_or("");
        if !matches!(leader_state, "PLAYING" | "TRANSITIONING") {
            eprintln!(
                "[musicd][group-add-sync] group={} skip reason=leader_state={}",
                group.id, leader_state
            );
            return Ok(());
        }

        let resource = self.stream_resource_for_track(&track);
        let target_position = snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.position_info.rel_time_seconds);

        for renderer_location in new_member_locations {
            eprintln!(
                "[musicd][group-add-sync] group={} member={} target_position={:?} stream_url={}",
                group.id, renderer_location, target_position, resource.stream_url
            );
            if let Err(error) =
                self.catch_up_group_member(renderer_location, &resource, target_position)
            {
                eprintln!(
                    "[musicd][group-add-sync] group={} member={} error={}",
                    group.id, renderer_location, error
                );
            }
        }

        self.events.touch(group_location);
        Ok(())
    }

    fn catch_up_group_member(
        &self,
        renderer_location: &str,
        resource: &StreamResource,
        target_position_seconds: Option<u64>,
    ) -> io::Result<()> {
        let renderer = self.play_stream_on_group_member(renderer_location, resource)?;
        eprintln!("[musicd][group-add-sync] renderer={renderer_location} stage=play_stream_ok");

        let Some(seconds) = target_position_seconds.filter(|seconds| *seconds > 0) else {
            return Ok(());
        };
        if renderer.capabilities.supports_seek() == Some(false) {
            eprintln!(
                "[musicd][group-add-seek] renderer={renderer_location} stage=skipped reason=unsupported"
            );
            return Ok(());
        }

        // Local renderers don't report transport state — skip the wait, the seek call
        // is a no-op on the server (the client manages its own position).
        let kind = renderer_kind_for_location(renderer_location);
        let needs_wait = !matches!(kind, RendererKind::AndroidLocal | RendererKind::CliLocal);
        if needs_wait {
            // The renderer is typically in TRANSITIONING right after Play returns.
            // Issuing Seek mid-transition can revert some devices back to STOPPED,
            // so wait briefly for a stable transport state before seeking.
            let waited = self.wait_for_transport_state(
                renderer_location,
                &["PLAYING", "PAUSED_PLAYBACK"],
                8,
                Duration::from_millis(150),
            );
            match waited {
                Ok(snapshot) => eprintln!(
                    "[musicd][group-add-seek] renderer={} stage=wait_ok state={} target={}",
                    renderer_location, snapshot.transport_info.transport_state, seconds
                ),
                Err(error) => eprintln!(
                    "[musicd][group-add-seek] renderer={renderer_location} stage=wait_failed error={error}"
                ),
            }
        }

        match self.run_renderer_action_with_private_queue_log(
            renderer_location,
            &renderer,
            "group-seek",
            |backend| backend.seek(&renderer, seconds),
        ) {
            Ok(()) => eprintln!(
                "[musicd][group-add-seek] renderer={renderer_location} stage=seek_ok target={seconds}"
            ),
            Err(error) => eprintln!(
                "[musicd][group-add-seek] renderer={renderer_location} stage=seek_failed target={seconds} error={error}"
            ),
        }
        Ok(())
    }

    fn group_leader_transport_snapshot(
        &self,
        group: &RendererGroup,
    ) -> io::Result<TransportSnapshot> {
        let mut errors = Vec::new();
        let mut pollable_members = 0usize;
        for member in &group.members {
            let renderer_location = &member.renderer_location;
            if matches!(
                renderer_kind_for_location(renderer_location),
                RendererKind::AndroidLocal | RendererKind::CliLocal
            ) {
                continue;
            }
            pollable_members += 1;
            match self.refresh_transport_state(renderer_location) {
                Ok(snapshot) => return Ok(snapshot),
                Err(error) => errors.push(format!("{renderer_location}: {error}")),
            }
        }
        if pollable_members == 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "group has no pollable transport leader",
            ));
        }
        Err(io::Error::other(format!(
            "no group members reported transport state: {}",
            errors.join("; ")
        )))
    }

    fn adopt_group_advanced_entry(
        &self,
        group_location: &str,
        queue: &PlaybackQueue,
        snapshot: &TransportSnapshot,
    ) -> io::Result<bool> {
        let Some(current_entry_id) = queue.current_entry_id else {
            return Ok(false);
        };
        let Some(next_entry) = next_queue_entry_after(queue, current_entry_id) else {
            return Ok(false);
        };
        if self
            .playback_session(group_location)
            .and_then(|session| session.next_queue_entry_id)
            != Some(next_entry.id)
        {
            return Ok(false);
        }
        let Some(track_uri) = snapshot.position_info.track_uri.as_deref() else {
            return Ok(false);
        };

        let next_track = self.find_track(&next_entry.track_id).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "queued next track not found")
        })?;
        let expected_stream_url = self.stream_resource_for_track(&next_track).stream_url;
        if !should_adopt_preloaded_next_entry(queue, snapshot, Some(&expected_stream_url)) {
            return Ok(false);
        }

        self.database.adopt_next_queue_entry_as_current(
            group_location,
            next_entry.id,
            &next_track.id,
            track_uri,
            next_track.duration_seconds,
        )?;
        if next_queue_entry_after(queue, next_entry.id).is_none() {
            match self.load_renderer_group_for_queue(group_location) {
                Ok(group) => {
                    self.clear_group_next_queue_entry(group_location, &group, "adopted-final");
                }
                Err(error) => self.debug_log(
                    "group-clear-next-failed",
                    format!(
                        "group={} reason=adopted-final load_group_error={}",
                        group_location, error
                    ),
                ),
            }
        }
        Ok(true)
    }

    fn mark_group_member_unreachable(
        &self,
        renderer_location: &str,
        error: &io::Error,
    ) -> io::Result<()> {
        if renderer_group_id_from_location(renderer_location).is_some() {
            return Ok(());
        }
        self.mark_renderer_unreachable(renderer_location, error)
    }
}

fn reject_nested_group_members(members: &[String]) -> io::Result<()> {
    if members
        .iter()
        .any(|member| renderer_group_id_from_location(member.trim()).is_some())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "renderer groups cannot contain other groups",
        ));
    }
    Ok(())
}
