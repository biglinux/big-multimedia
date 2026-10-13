// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Pixbuf / raw-RGBA → [`gtk::gdk::Texture`] helpers.
//!
//! Every BigLinux media surface (camera roll, file-manager preview, image
//! viewer, video poster) ends at a `gtk::Picture` fed a `gdk::Texture`. This is
//! the canonical decode→texture conversion so apps don't re-roll the
//! pixbuf→`MemoryTexture` boilerplate (and its rowstride / lifetime hazards) at
//! every call site. Depends on GTK only.

use gtk::prelude::*;
use relm4::gtk;

/// Decoded image as tightly packed RGBA8 plus its dimensions. `Send`, so it can
/// cross from a worker thread, where the `!Send` `Pixbuf` decode runs, back to
/// the GTK main thread, where [`rgba_to_texture`] wraps it in a `MemoryTexture`
/// (the one step that must run on the main thread). This is how a file manager
/// or gallery decodes thumbnails off the main loop without ever moving a
/// `Pixbuf`/`Texture` across threads.
pub struct RgbaImage {
    /// Tightly packed RGBA8 pixels: `width * height * 4` bytes, no row padding.
    pub rgba: Vec<u8>,
    /// Image width in pixels.
    pub width: i32,
    /// Image height in pixels.
    pub height: i32,
    /// The source had no alpha channel, so the alpha bytes above are all `255`
    /// filler. [`rgba_to_texture`] then tags the texture `R8g8b8x8`: GDK skips
    /// the premultiply pass on upload, the compositor skips the blend, and the
    /// texture stays eligible for a `GraphicsOffload` subsurface. Album art,
    /// posters and video thumbnails are all opaque.
    pub opaque: bool,
}

/// Pack a `Pixbuf` into tightly packed RGBA8 in [`gtk::gdk::MemoryTexture`] layout. No GTK
/// texture is created, so unlike [`pixbuf_to_texture`] this is safe to call
/// on a worker thread (it touches only the `Pixbuf` and a plain buffer).
///
/// Feeding GTK the pixbuf's padded rowstride (or borrowing its pixel bytes) is a
/// layout/lifetime mismatch that can corrupt the heap, so we copy row-by-row
/// into a packed owned buffer.
///
/// A source without alpha is widened to RGBA here instead of through
/// `Pixbuf::add_alpha`, which would allocate a second full-size pixbuf only to
/// be repacked immediately; the resulting image is flagged [`RgbaImage::opaque`].
#[must_use]
pub fn pixbuf_to_rgba(pixbuf: &gtk::gdk_pixbuf::Pixbuf) -> Option<RgbaImage> {
    let width = pixbuf.width();
    let height = pixbuf.height();
    if width <= 0 || height <= 0 {
        return None;
    }
    let channels = usize::try_from(pixbuf.n_channels()).ok()?;
    if channels != 3 && channels != 4 {
        return None;
    }
    let columns = usize::try_from(width).ok()?;
    let row = columns.checked_mul(4)?;
    let src_row = columns.checked_mul(channels)?;
    let src_stride = usize::try_from(pixbuf.rowstride()).ok()?;
    let rows = usize::try_from(height).ok()?;
    let src = pixbuf.read_pixel_bytes();
    let mut packed = vec![0_u8; row.checked_mul(rows)?];
    for y in 0..rows {
        let s = y.checked_mul(src_stride)?;
        let d = y * row;
        let source_row = src.get(s..s + src_row)?;
        let target_row = packed.get_mut(d..d + row)?;
        if channels == 4 {
            target_row.copy_from_slice(source_row);
        } else {
            for (target, source) in target_row
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(source_row.as_chunks::<3>().0)
            {
                target[..3].copy_from_slice(source);
                target[3] = u8::MAX;
            }
        }
    }
    Some(RgbaImage {
        rgba: packed,
        width,
        height,
        opaque: channels == 3,
    })
}

/// Convert a `Pixbuf` into a [`gtk::gdk::Texture`]. Main-thread only
/// (`MemoryTexture::new` asserts it); for large or networked files decode off
/// the main loop and hand the pixels to [`rgba_to_texture`] instead.
#[must_use]
pub fn pixbuf_to_texture(pixbuf: &gtk::gdk_pixbuf::Pixbuf) -> Option<gtk::gdk::Texture> {
    Some(rgba_to_texture(pixbuf_to_rgba(pixbuf)?))
}

/// Build a texture from an already-decoded, tightly packed RGBA8 image (e.g. the
/// `image` crate's `to_rgba8()` output wrapped in an [`RgbaImage`]).
/// `rgba.len()` must equal `width * height * 4`.
#[must_use]
pub fn rgba_to_texture(image: RgbaImage) -> gtk::gdk::Texture {
    let stride = usize::try_from(image.width).unwrap_or(0).saturating_mul(4);
    let format = if image.opaque {
        gtk::gdk::MemoryFormat::R8g8b8x8
    } else {
        gtk::gdk::MemoryFormat::R8g8b8a8
    };
    let bytes = gtk::glib::Bytes::from_owned(image.rgba);
    gtk::gdk::MemoryTexture::new(image.width, image.height, format, &bytes, stride).upcast()
}
