use std::io;

use crate::renderer::renderer_group_queue_key;
use crate::types::{QueueMutationEntry, RendererGroup};

use super::ServiceState;

impl ServiceState {
    pub(crate) fn renderer_group_snapshot(&self) -> Vec<RendererGroup> {
        self.database.list_renderer_groups().unwrap_or_default()
    }

    pub(crate) fn create_renderer_group(
        &self,
        name: &str,
        members: &[String],
        source_renderer_location: Option<&str>,
    ) -> io::Result<RendererGroup> {
        let group = self.database.create_renderer_group(name, members)?;
        let group_queue_key = renderer_group_queue_key(&group.id);
        if let Some(source_renderer_location) = source_renderer_location
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let Some(source_queue) = self.database.load_queue(source_renderer_location)? else {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "source queue not found",
                ));
            };
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
}
