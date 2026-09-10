//! Illustration image builders backed by GResource icons, with auto-detection
//! of the active app's resource prefix (audio vs video player).
//! bigagents: app-local-adapter; resource-prefix probing is app bundle policy.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

use gtk::prelude::*;

thread_local! {
    /// Decoded illustration textures keyed by resource path.
    ///
    /// `gtk::Image::from_resource` re-decodes the SVG (librsvg, on the main
    /// thread) on every call and GTK keeps no cache; the preferences dialog
    /// builds ~60 of these at once, stalling the GLArea video render for a
    /// moment. Caching the decoded `GdkTexture` parses each SVG at most once
    /// per process, so repeat opens are free and the first is cheaper.
    static TEXTURE_CACHE: RefCell<HashMap<String, gtk::gdk::Texture>> =
        RefCell::new(HashMap::new());
}

pub fn illustration(resource_prefix: &str, filename: &str) -> gtk::Image {
    let path = format!("{resource_prefix}{filename}");
    let texture = TEXTURE_CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry(path.clone())
            .or_insert_with(|| gtk::gdk::Texture::from_resource(&path))
            .clone()
    });
    let img = gtk::Image::from_paintable(Some(&texture));
    img.set_pixel_size(56);
    img.set_margin_top(6);
    img.set_margin_bottom(6);
    img
}

/// Probes Video first because the audio_converter binary bundles both
/// resources and its video/preferences pages need the video illustrations.
pub fn illustrations_prefix() -> &'static str {
    static PREFIX: OnceLock<&str> = OnceLock::new();
    PREFIX.get_or_init(|| {
        if gtk::gio::resources_lookup_data(
            "/br/com/biglinux/VideoPlayer/illustrations/normalize.svg",
            gtk::gio::ResourceLookupFlags::NONE,
        )
        .is_ok()
        {
            "/br/com/biglinux/VideoPlayer/illustrations/"
        } else {
            "/br/com/biglinux/AudioPlayer/illustrations/"
        }
    })
}

pub fn illustration_auto(filename: &str) -> gtk::Image {
    illustration(illustrations_prefix(), filename)
}
