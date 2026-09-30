mod album_detail;
mod auth;
mod error;
pub(crate) mod json;
mod layout;
mod library;
mod queue;
mod track_detail;
mod welcome;

pub(crate) use album_detail::render_album_detail_page;
pub(crate) use auth::{render_change_password_page, render_login_page};
pub(crate) use library::{render_library_page, render_library_rows_json};
pub(crate) use queue::{render_queue_page, render_queue_panel_html};
pub(crate) use track_detail::render_track_detail_page;
pub(crate) use welcome::render_welcome_page;
