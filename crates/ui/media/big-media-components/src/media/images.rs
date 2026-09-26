// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Texture creation, thumbnail scaling, and dominant color extraction.

use gtk::{gdk, gio, glib};

/// Decode `image_bytes` into a thumbnail texture bounded by `size` on each edge.
///
/// Aspect ratio is preserved. Returns [`None`] on decode failure.
#[must_use]
pub fn bytes_to_thumbnail(image_bytes: &[u8], size: i32) -> Option<gdk::Texture> {
    let bytes = glib::Bytes::from(image_bytes);
    let stream = gio::MemoryInputStream::from_bytes(&bytes);
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_stream_at_scale(
        &stream,
        size,
        size,
        true,
        gio::Cancellable::NONE,
    )
    .ok()?;
    big_relm4_components::display::texture::pixbuf_to_texture(&pixbuf)
}
