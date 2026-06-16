//! Embedded static frontend assets.
//!
//! Content is baked into the binary at compile time via `include_str!`, so
//! there's no runtime filesystem lookup. Each asset URL carries a `?v=`
//! query string derived from `CARGO_PKG_VERSION` and the embedded asset body;
//! browsers cache the body for a year via `Cache-Control: public,
//! max-age=31536000, immutable`, and the content-bumped URL invalidates the
//! cache when the shipped asset changes.

use std::hash::{Hash, Hasher};

pub(crate) const HOME_CSS: &str = include_str!("../assets/home.css");
pub(crate) const HOME_JS: &str = include_str!("../assets/home.js");
pub(crate) const ALBUM_DETAIL_CSS: &str = include_str!("../assets/album_detail.css");
pub(crate) const TRACK_DETAIL_CSS: &str = include_str!("../assets/track_detail.css");

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) fn asset_version(body: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    VERSION.hash(&mut hasher);
    body.hash(&mut hasher);
    format!("{VERSION}-{:016x}", hasher.finish())
}
