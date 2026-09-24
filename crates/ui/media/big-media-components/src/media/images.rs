// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Texture creation, thumbnail scaling, and dominant color extraction.

use gtk::{gdk, gio, glib};

const COLOR_BUCKET_SIZE: u8 = 64;

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

/// Compute the dominant color from a raw pixel buffer.
///
/// `pixels` is read in row-major order using `rowstride` and `n_channels`
/// (3 or 4). Pixels with maximum-channel value `< 30` or minimum-channel
/// value `> 225` are skipped before voting.
#[must_use]
pub fn dominant_color_from_pixels(
    pixels: &[u8],
    width: usize,
    height: usize,
    n_channels: usize,
    rowstride: usize,
) -> Option<(u8, u8, u8)> {
    let mut buckets = std::collections::HashMap::<(u8, u8, u8), u32>::new();

    for y in 0..height {
        for x in 0..width {
            let offset = y * rowstride + x * n_channels;
            if offset + 2 >= pixels.len() {
                continue;
            }
            let r = pixels[offset];
            let g = pixels[offset + 1];
            let b = pixels[offset + 2];

            let max_c = r.max(g).max(b);
            let min_c = r.min(g).min(b);
            if max_c < 30 || min_c > 225 {
                continue;
            }

            let key = (
                r / COLOR_BUCKET_SIZE,
                g / COLOR_BUCKET_SIZE,
                b / COLOR_BUCKET_SIZE,
            );
            *buckets.entry(key).or_insert(0) += 1;
        }
    }

    buckets
        .into_iter()
        .max_by_key(|&(_, count)| count)
        .map(|((rb, gb, bb), _)| {
            (
                rb * COLOR_BUCKET_SIZE + 32,
                gb * COLOR_BUCKET_SIZE + 32,
                bb * COLOR_BUCKET_SIZE + 32,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_pixels(r: u8, g: u8, b: u8, width: usize, height: usize) -> Vec<u8> {
        let mut buf = Vec::with_capacity(width * height * 3);
        for _ in 0..(width * height) {
            buf.push(r);
            buf.push(g);
            buf.push(b);
        }
        buf
    }

    #[test]
    fn dominant_solid_red_uses_bucket_center() {
        let px = solid_pixels(200, 50, 50, 4, 4);
        assert_eq!(
            dominant_color_from_pixels(&px, 4, 4, 3, 4 * 3),
            Some((224, 32, 32))
        );
    }

    #[test]
    fn dominant_filters_black_and_white() {
        let black = solid_pixels(10, 10, 10, 4, 4);
        let white = solid_pixels(230, 230, 230, 4, 4);
        assert_eq!(dominant_color_from_pixels(&black, 4, 4, 3, 4 * 3), None);
        assert_eq!(dominant_color_from_pixels(&white, 4, 4, 3, 4 * 3), None);
    }

    #[test]
    fn dominant_majority_wins() {
        let mut px = solid_pixels(200, 50, 50, 4, 4);
        for i in 12..16 {
            let offset = i * 3;
            px[offset] = 50;
            px[offset + 1] = 50;
            px[offset + 2] = 200;
        }
        assert_eq!(
            dominant_color_from_pixels(&px, 4, 4, 3, 4 * 3),
            Some((224, 32, 32))
        );
    }

    #[test]
    fn dominant_handles_rowstride_padding() {
        let mut px = vec![0_u8; 8 * 2];
        for offset in [0, 3, 8, 11] {
            px[offset] = 150;
            px[offset + 1] = 80;
            px[offset + 2] = 80;
        }
        assert_eq!(
            dominant_color_from_pixels(&px, 2, 2, 3, 8),
            Some((160, 96, 96))
        );
    }

    #[test]
    fn dominant_ignores_alpha_channel() {
        let px = vec![
            200, 50, 50, 255, 200, 50, 50, 128, 200, 50, 50, 0, 200, 50, 50, 255,
        ];
        assert_eq!(
            dominant_color_from_pixels(&px, 2, 2, 4, 8),
            Some((224, 32, 32))
        );
    }
}
