//! Canonical media surface API for BigLinux media apps.
//!
//! This module names the existing shared media-window implementation as the
//! media-domain surface contract. Apps should import from here when migrating
//! fullscreen, OSD, overlay layering, idle chrome, or sidebar behavior instead
//! of rebuilding those policies locally.

use gtk::prelude::*;

pub use big_relm4_components::layout::media_window::{
    BigMediaChrome, BigMediaChromePlacement, BigMediaImmersionController, BigMediaMenuSpec,
    BigMediaOverlayLayerRole, BigMediaOverlayLayerSpec, BigMediaOverlayStack, BigMediaSidebarShell,
    BigMediaSidebarSpec, create_media_toast_overlay_root, create_media_toast_overlay_with_content,
    media_chrome_subtree_has_focus, mount_media_toast_content,
};

/// Set fullscreen on the toplevel window that owns `widget`.
///
/// Returns `false` when the widget is not mounted in a `gtk::Window` yet. Media
/// components use this instead of app-local `root().and_downcast::<gtk::Window>()`
/// fragments so fullscreen policy has one public owner.
pub fn set_surface_fullscreened(widget: &impl IsA<gtk::Widget>, is_fullscreened: bool) -> bool {
    let Some(window) = widget.root().and_downcast::<gtk::Window>() else {
        return false;
    };
    window.set_fullscreened(is_fullscreened);
    true
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_reexports_overlay_policy_contract() {
        let media_layer = BigMediaOverlayLayerRole::PrimaryMediaSurface.layer_spec();
        assert!(media_layer.should_clip_layer());
        assert!(!media_layer.should_measure_layer());

        let osd_layer = BigMediaOverlayLayerRole::ViewerAdornmentOverlay.layer_spec();
        assert!(!osd_layer.should_clip_layer());
        assert!(!osd_layer.should_measure_layer());
        assert!(
            BigMediaOverlayLayerRole::PrimaryMediaSurface.stacking_order()
                < BigMediaOverlayLayerRole::ViewerAdornmentOverlay.stacking_order()
        );
        assert!(matches!(
            BigMediaChromePlacement::HeaderThenBottom,
            BigMediaChromePlacement::HeaderThenBottom
        ));
    }
}
