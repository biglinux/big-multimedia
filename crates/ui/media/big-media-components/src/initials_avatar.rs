//! GTK avatar placeholders rendered from deterministic initials and colors.
//!
//! Gives radio stations and other media library entries a stable visual
//! identity when the artwork is missing. The hash-to-HSL mapping is
//! deterministic: the same input always produces the same hue.

use gtk::glib;
use gtk::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
struct AvatarCircle {
    center_x: f64,
    center_y: f64,
    radius: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AvatarTextExtents {
    width: f64,
    height: f64,
    x_bearing: f64,
    y_bearing: f64,
}

/// Render an initials avatar into a HiDPI [`gtk::gdk::Texture`].
///
/// The texture is rasterized at `2× size` pixels using cairo and wrapped in
/// a [`gtk::gdk::MemoryTexture`]. Returns [`None`] if surface allocation or
/// the cairo context fails.
pub fn initials_texture(name: &str, size: i32) -> Option<gtk::gdk::Texture> {
    let initials = extract_initials(name);
    let px = avatar_texture_pixel_size(size);

    let mut surface = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, px, px).ok()?;
    let cr = gtk::cairo::Context::new(&surface).ok()?;
    let (r, g, b) = color_for_name(name);

    let circle = avatar_circle(px, px, 1.0);
    cr.arc(
        circle.center_x,
        circle.center_y,
        circle.radius,
        0.0,
        full_circle_radians(),
    );
    cr.set_source_rgb(r, g, b);
    let _ = cr.fill();

    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.select_font_face(
        "sans-serif",
        gtk::cairo::FontSlant::Normal,
        gtk::cairo::FontWeight::Bold,
    );
    cr.set_font_size(avatar_font_size(px));
    let extents = cr.text_extents(&initials).ok()?;
    let (x, y) = centered_text_origin(
        circle.center_x,
        circle.center_y,
        AvatarTextExtents {
            width: extents.width(),
            height: extents.height(),
            x_bearing: extents.x_bearing(),
            y_bearing: extents.y_bearing(),
        },
    );
    cr.move_to(x, y);
    let _ = cr.show_text(&initials);

    drop(cr);
    surface.flush();

    let surface_pixels = surface.data().ok()?;
    let bytes = glib::Bytes::from(&*surface_pixels);
    Some(
        gtk::gdk::MemoryTexture::new(
            px,
            px,
            gtk::gdk::MemoryFormat::B8g8r8a8Premultiplied,
            &bytes,
            avatar_texture_stride(px),
        )
        .upcast(),
    )
}

fn avatar_texture_pixel_size(size: i32) -> i32 {
    size * 2
}

fn avatar_texture_stride(pixel_size: i32) -> usize {
    (pixel_size * 4) as usize
}

fn avatar_circle(width: i32, height: i32, radius_inset: f64) -> AvatarCircle {
    let center_x = f64::from(width) / 2.0;
    let center_y = f64::from(height) / 2.0;
    AvatarCircle {
        center_x,
        center_y,
        radius: center_x.min(center_y) - radius_inset,
    }
}

fn avatar_font_size(pixel_width: i32) -> f64 {
    f64::from(pixel_width) * 0.42
}

fn full_circle_radians() -> f64 {
    2.0 * std::f64::consts::PI
}

fn centered_text_origin(center_x: f64, center_y: f64, extents: AvatarTextExtents) -> (f64, f64) {
    (
        center_x - extents.width / 2.0 - extents.x_bearing,
        center_y - extents.height / 2.0 - extents.y_bearing,
    )
}

/// Extract up to two uppercase letters from `name` for use as avatar initials.
///
/// Splits `name` on whitespace and takes the first character from each of
/// the first two suitable words. Falls back to the first non-whitespace
/// character of `name`, or `"R"` if `name` is empty.
#[must_use]
pub fn extract_initials(name: &str) -> String {
    let initials: String = name
        .split_whitespace()
        .filter(|word| word.len() > 1 || word.chars().next().is_some_and(char::is_alphanumeric))
        .take(2)
        .filter_map(|word| word.chars().next())
        .map(|character| character.to_uppercase().to_string())
        .collect();
    if initials.is_empty() {
        name.chars()
            .next()
            .unwrap_or('R')
            .to_uppercase()
            .to_string()
    } else {
        initials
    }
}

/// Convert an HSL color to an RGB tuple in `[0.0, 1.0]^3`.
///
/// `hue` is expected in `[0.0, 360.0)`, `saturation` and `lightness` in
/// `[0.0, 1.0]`. Derives the avatar colors from the hash-to-HSL mapping.
#[must_use]
pub fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> (f64, f64, f64) {
    let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let h2 = hue / 60.0;
    let x = c * (1.0 - (h2 % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h2 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = lightness - c / 2.0;
    (r1 + m, g1 + m, b1 + m)
}

fn color_for_name(name: &str) -> (f64, f64, f64) {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    hsl_to_rgb((hasher.finish() % 360) as f64, 0.45, 0.55)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hash::{Hash, Hasher};

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 0.01,
            "expected {actual} to be close to {expected}"
        );
    }

    fn assert_rgb_close(actual: (f64, f64, f64), expected: (f64, f64, f64)) {
        assert_close(actual.0, expected.0);
        assert_close(actual.1, expected.1);
        assert_close(actual.2, expected.2);
    }

    #[test]
    fn initials_cover_radio_station_name_examples() {
        assert_eq!(extract_initials("Big Radio"), "BR");
        assert_eq!(extract_initials("Jazz"), "J");
        assert_eq!(extract_initials(""), "R");
        assert_eq!(extract_initials("Classic Rock FM"), "CR");
        assert_eq!(extract_initials("Rádio Brasil"), "RB");
        assert_eq!(extract_initials("101.5 FM"), "1F");
        assert_eq!(extract_initials("   "), " ");
        assert_eq!(extract_initials("A B"), "AB");
        assert_eq!(extract_initials("-- Radio"), "-R");
        assert_eq!(extract_initials("! ?"), "!");
    }

    #[test]
    fn avatar_layout_helpers_center_circle_and_text() {
        assert_eq!(
            avatar_circle(40, 30, 0.5),
            AvatarCircle {
                center_x: 20.0,
                center_y: 15.0,
                radius: 14.5,
            }
        );
        assert_eq!(avatar_texture_pixel_size(16), 32);
        assert_eq!(avatar_texture_stride(32), 128);
        assert_close(avatar_font_size(50), 21.0);
        assert_close(full_circle_radians(), 2.0 * std::f64::consts::PI);
        assert_eq!(
            centered_text_origin(
                20.0,
                15.0,
                AvatarTextExtents {
                    width: 8.0,
                    height: 10.0,
                    x_bearing: -1.0,
                    y_bearing: 2.0,
                },
            ),
            (17.0, 8.0)
        );
    }

    // Cairo image surface creation is C FFI. The pure avatar geometry and color
    // helpers above remain covered by Miri; this rendered texture check runs normally.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn initials_texture_returns_hidpi_square_texture() {
        let texture = initials_texture("Big Radio", 16).expect("avatar texture should render");
        assert_eq!(texture.width(), 32);
        assert_eq!(texture.height(), 32);
    }

    #[test]
    fn hsl_primary_and_neutral_colors() {
        assert_rgb_close(hsl_to_rgb(0.0, 1.0, 0.5), (1.0, 0.0, 0.0));
        assert_rgb_close(hsl_to_rgb(60.0, 1.0, 0.5), (1.0, 1.0, 0.0));
        assert_rgb_close(hsl_to_rgb(120.0, 1.0, 0.5), (0.0, 1.0, 0.0));
        assert_rgb_close(hsl_to_rgb(180.0, 1.0, 0.5), (0.0, 1.0, 1.0));
        assert_rgb_close(hsl_to_rgb(240.0, 1.0, 0.5), (0.0, 0.0, 1.0));
        assert_rgb_close(hsl_to_rgb(300.0, 1.0, 0.5), (1.0, 0.0, 1.0));
        assert_rgb_close(hsl_to_rgb(30.0, 1.0, 0.5), (1.0, 0.5, 0.0));
        assert_rgb_close(hsl_to_rgb(0.0, 0.0, 0.5), (0.5, 0.5, 0.5));
        assert_rgb_close(hsl_to_rgb(0.0, 1.0, 0.25), (0.5, 0.0, 0.0));
    }

    #[test]
    fn color_for_name_uses_bounded_deterministic_hue() {
        let color = color_for_name("Big Radio");
        let same_color = color_for_name("Big Radio");
        let other_color = color_for_name("Jazz Station");

        assert_eq!(color, same_color);
        assert_ne!(color, other_color);
        for channel in [color.0, color.1, color.2] {
            assert!((0.0..=1.0).contains(&channel));
        }

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        "Big Radio".hash(&mut hasher);
        let expected_hue = (hasher.finish() % 360) as f64;
        assert_rgb_close(color, hsl_to_rgb(expected_hue, 0.45, 0.55));
    }
}
