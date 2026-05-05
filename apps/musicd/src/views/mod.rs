mod album_detail;
mod error;
mod home;
pub(crate) mod json;
mod queue_panel;
mod track_detail;

pub(crate) use album_detail::render_album_detail_page;
pub(crate) use home::render_home_page;
pub(crate) use queue_panel::render_queue_panel_html;
pub(crate) use track_detail::render_track_detail_page;
