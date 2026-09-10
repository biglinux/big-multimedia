//! GResource illustration helpers shared by audio and video player dialogs.

use std::sync::OnceLock;

use gtk::prelude::*;

/// Build a fixed-size illustration image from `resource_prefix + filename`.
pub fn illustration(resource_prefix: &str, filename: &str) -> gtk::Image {
    let path = format!("{resource_prefix}{filename}");
    let image = gtk::Image::from_resource(&path);
    image.set_pixel_size(56);
    image.set_margin_top(6);
    image.set_margin_bottom(6);
    image
}

/// Detect the active player illustration resource prefix.
///
/// Video is probed first because some host binaries bundle both resource sets.
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

/// Build an illustration image using the detected player resource prefix.
pub fn illustration_auto(filename: &str) -> gtk::Image {
    illustration(illustrations_prefix(), filename)
}
