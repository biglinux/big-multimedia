//! Canonical media surface API for BigLinux media apps.
//!
//! This module names the existing shared media-window implementation as the
//! media-domain surface contract. Apps should import from here when migrating
//! fullscreen, OSD, overlay layering, idle chrome, or sidebar behavior instead
//! of rebuilding those policies locally.

use gtk::prelude::*;

pub use big_relm4_components::layout::media_window::{
    BigMediaChrome, BigMediaChromePlacement, BigMediaImmersionController, BigMediaOverlayStack,
    BigMediaSidebarShell, BigMediaSidebarSpec, MediaNotice, create_media_root,
};

/// Toggle fullscreen on the toplevel window that owns `widget`.
///
/// Returns the new fullscreen state, or `None` when the widget is not mounted in
/// a `gtk::Window` yet.
#[must_use]
pub fn toggle_surface_fullscreened(widget: &impl IsA<gtk::Widget>) -> Option<bool> {
    let window = widget.root().and_downcast::<gtk::Window>()?;
    let is_fullscreened = !window.is_fullscreen();
    window.set_fullscreened(is_fullscreened);
    Some(is_fullscreened)
}
