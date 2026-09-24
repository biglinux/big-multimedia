//! An RGB (no-alpha) pixbuf is widened to RGBA in place, keeping channel order
//! and padding the alpha, and is reported opaque so the texture can skip the
//! premultiply pass. A swapped channel here would tint every album art and
//! video poster, and a wrong `opaque` flag would misdeclare the memory format.

use big_relm4_components::display::texture::{pixbuf_to_rgba, pixbuf_to_texture};
use relm4::gtk::gdk::prelude::TextureExtManual;
use relm4::gtk::gdk_pixbuf::{Colorspace, Pixbuf};

fn filled(has_alpha: bool, channels: &[u8]) -> Pixbuf {
    let pixbuf = Pixbuf::new(Colorspace::Rgb, has_alpha, 8, 2, 2).expect("pixbuf");
    // fill() takes 0xRRGGBBAA and ignores the alpha byte on an RGB pixbuf.
    let packed = u32::from_be_bytes([channels[0], channels[1], channels[2], channels[3]]);
    pixbuf.fill(packed);
    pixbuf
}

#[test]
fn rgb_source_is_widened_with_opaque_alpha() {
    let image = pixbuf_to_rgba(&filled(false, &[10, 20, 30, 0])).expect("packed image");

    assert_eq!(image.width, 2);
    assert_eq!(image.height, 2);
    assert!(image.opaque, "an RGB source carries no transparency");
    assert_eq!(image.rgba, [10, 20, 30, 255].repeat(4));
}

#[test]
fn rgba_source_keeps_its_alpha_and_is_not_opaque() {
    let image = pixbuf_to_rgba(&filled(true, &[10, 20, 30, 128])).expect("packed image");

    assert!(!image.opaque, "an RGBA source may carry transparency");
    assert_eq!(image.rgba, [10, 20, 30, 128].repeat(4));
}

/// The opaque tag is a memory-format declaration, not a colour change: a fully
/// opaque colour must download identically whether it arrived as RGB or RGBA.
#[test]
fn the_opaque_format_tag_does_not_alter_the_downloaded_colour() {
    if relm4::gtk::init().is_err() {
        eprintln!("no display: skipping the texture download comparison");
        return;
    }
    let mut from_rgb = vec![0_u8; 2 * 2 * 4];
    let mut from_rgba = vec![0_u8; 2 * 2 * 4];
    pixbuf_to_texture(&filled(false, &[10, 20, 30, 0]))
        .expect("texture")
        .download(&mut from_rgb, 2 * 4);
    pixbuf_to_texture(&filled(true, &[10, 20, 30, 255]))
        .expect("texture")
        .download(&mut from_rgba, 2 * 4);

    assert_eq!(from_rgb, from_rgba);
}
