//! Meson UI helpers — [`MesonImg`] wraps Orbital `Image` with virus-scan sync.
//!
//! # Features
//!
//! - **MesonImg** — File id in → Orbital Image when Available; Skeleton while
//!   pending; empty when quarantined/denied. [Get started](#mesonimg)
//!
//! # MesonImg
//!
//! Pass a File id (not a URL). Photon `meson.file.updated` refetches so the
//! frame updates after scan/promote. Hosts must mount Photon WS at
//! `/ws/meson-files`.

#![deny(missing_docs)]

mod meson_img;
mod server;
mod status;

pub use meson_img::MesonImg;
pub use server::{
    get_meson_image_src, meson_file_live_sync, subscribe_meson_file_live_sync, MesonImageSrc,
};
pub use status::{is_pending_status, meson_image_src_for_status, status_badge_label};
