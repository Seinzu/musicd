use std::io;

use musicd_upnp::{StreamResource, TransportSnapshot};

use crate::renderer::{
    RendererKind, renderer_group_id_from_location, renderer_group_queue_key,
    renderer_kind_for_location,
};
use crate::types::{PlaybackQueue, QueueMutationEntry, RendererGroup, RendererRecord};

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
        let source_queue = if let Some(source_renderer_location) = source_renderer_location
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            self.check_direct_renderer_access(source_renderer_location, client_id)?;
            self.database.load_queue(source_renderer_location)?
        } else {
            None
        };

        let group = self.database.create_renderer_group(name, members)?;
        let group_queue_key = renderer_group_queue_key(&group.id);
        if let Some(source_queue) = source_queue {
            let entries = source_queue
                .entries
                .into_iter()
                .map(|entry| QueueMutationEntry {
                    track_id: entry.track_id,
                    album_id: entry.album_id,
                    source_kind: entry.source_kind,
                    source_ref: entry.source_ref,
                })
                .collect::<Vec<_>>();
            self.database
                .replace_queue(&group_queue_key, &source_queue.name, &entries)?;
        } else {
            self.database
                .replace_queue(&group_queue_key, &group.name, &[])?;
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
        let group = self.load_renderer_group_for_queue(renderer_location)?;
        let existing_members = group
            .members
            .iter()
            .map(|member| member.renderer_location.clone())
            .collect::<Vec<_>>();
        self.check_private_renderer_additions_owned(members, &existing_members, client_id)?;
        self.database
            .update_renderer_group(&group.id, name, members)
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
    ) -> io::Result<RendererGroup> {
        let group = self.load_renderer_group_for_queue(renderer_location)?;
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
    ) -> io::Result<()> {
        let Some(next_entry) = next_queue_entry_after(queue, current_entry_id) else {
            self.database
                .mark_next_queue_entry_preloaded(group_location, None)?;
            return Ok(());
        };

        if self
            .playback_session(group_location)
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
            if renderer.capabilities.supports_set_next_av_transport_uri() == Some(false) {
                continue;
            }
            match self
                .renderer_backend(renderer_location)?
                .preload_next(&renderer, &resource)
            {
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

        if self.adopt_group_advanced_entry(group_location, &queue, &snapshot)? {
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
            if let Err(error) = self.preload_next_group_queue_entry(
                group_location,
                &group,
                &queue,
                current_entry_id,
            ) {
                eprintln!("group next-track preload refresh failed for {group_location}: {error}");
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

    fn play_stream_on_group_member(
        &self,
        renderer_location: &str,
        resource: &StreamResource,
    ) -> io::Result<RendererRecord> {
        let renderer = self.resolve_renderer(renderer_location)?;
        if let Err(error) = self
            .renderer_backend(renderer_location)?
            .play_stream(&renderer, resource)
        {
            let _ = self.mark_group_member_unreachable(renderer_location, &error);
            return Err(error);
        }
        let _ = self.mark_renderer_reachable(&renderer);
        Ok(renderer)
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
                RendererKind::AndroidLocal
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
