// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Texture creation, thumbnail scaling, and dominant color extraction.

use gtk::{gdk, gio, glib};

/// Whether `bytes` start like a PNG, JPEG, WebP, GIF, AVIF or SVG image.
///
/// Album art and channel logos come from tags and the network. gdk-pixbuf picks
/// a loader by sniffing, and besides the sandboxed glycin loaders it still runs
/// legacy ones in-process (WMF through libwmf, BMP, ICO, TGA, XPM…): a forged
/// cover must not reach them. These formats all go to glycin.
fn is_known_image(bytes: &[u8]) -> bool {
    let text = bytes
        .strip_prefix(b"\xef\xbb\xbf")
        .unwrap_or(bytes)
        .trim_ascii_start();
    text.starts_with(b"<svg")
        || text.starts_with(b"<?xml")
        || bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(&[0xff, 0xd8, 0xff])
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
        || (bytes.get(4..8) == Some(b"ftyp") && matches!(bytes.get(8..12), Some(b"avif" | b"avis")))
}

/// Decode `image_bytes` into a thumbnail texture bounded by `size` on each edge.
///
/// Aspect ratio is preserved. Returns [`None`] on decode failure and for any
/// format other than PNG, JPEG, WebP, GIF, AVIF or SVG.
#[must_use]
pub fn bytes_to_thumbnail(image_bytes: &[u8], size: i32) -> Option<gdk::Texture> {
    if !is_known_image(image_bytes) {
        return None;
    }
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

#[cfg(test)]
mod tests {
    use super::is_known_image;

    #[test]
    fn only_the_sandboxed_formats_pass() {
        for known in [
            &b"\x89PNG\r\n\x1a\n...."[..],
            &[0xff, 0xd8, 0xff, 0xe0],
            b"GIF89a....",
            b"RIFF\0\0\0\0WEBPVP8 ",
            b"\0\0\0\x1cftypavif",
            b"\xef\xbb\xbf  <?xml version",
            b"<svg xmlns=",
        ] {
            assert!(is_known_image(known), "{known:?}");
        }
        for unknown in [
            &b"\xd7\xcd\xc6\x9a\0\0"[..],
            b"BM\0\0\0\0",
            b"/* XPM */",
            b"RIFF\0\0\0\0WAVE",
            b"\0\0\0\x1cftypisom",
            b"\x89PN",
            b"",
        ] {
            assert!(!is_known_image(unknown), "{unknown:?}");
        }
    }
}
