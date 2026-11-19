// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared libadwaita color-scheme, CSS, and custom palette helpers.

use relm4::gtk;

/// HIGH CONTRAST CSS constant.
pub const HIGH_CONTRAST_CSS: &str = "\
window { --dim-opacity: 100%; }
.dim-label { opacity: 1.0; }
.subtitle { opacity: 1.0; }
.caption { opacity: 1.0; }
row.property > box.header > box.title > .subtitle { opacity: 1.0; color: @window_fg_color; }
list.navigation-sidebar row label.subtitle { opacity: 1.0; }
headerbar .subtitle { opacity: 1.0; }
";

/// Color palette value used across BigLinux apps.
pub struct ColorPalette {
    /// Window/background color (sRGB 8-bit triplet).
    pub bg: (u8, u8, u8),
    /// Foreground/text color (sRGB 8-bit triplet).
    pub fg: (u8, u8, u8),
    /// Symbolic-icon tint color (sRGB 8-bit triplet).
    pub icon: (u8, u8, u8),
    /// Dialog/popover background color (sRGB 8-bit triplet).
    pub dialog: (u8, u8, u8),
}

/// Render the libadwaita CSS override block for the supplied palette.
///
/// Emits `@define-color` declarations and `:root` custom properties covering
/// window, headerbar, popover, dialog, card, view, sidebar surfaces, plus
/// the BigLinux-specific `player_icon_color`. Load the result via
/// [`CssSlot::set`].
#[must_use]
pub fn build_adwaita_color_overrides(palette: &ColorPalette) -> String {
    let (bg_r, bg_g, bg_b) = palette.bg;
    let (fg_r, fg_g, fg_b) = palette.fg;
    let (ic_r, ic_g, ic_b) = palette.icon;
    let (dl_r, dl_g, dl_b) = palette.dialog;
    // The sidebar sits a shade toward the text color, as libadwaita's does.
    let toward_fg = |bg: u8, fg: u8| {
        let shifted = (i16::from(fg) - i16::from(bg)) * 6 / 100 + i16::from(bg);
        u8::try_from(shifted).unwrap_or(bg)
    };
    let (sb_r, sb_g, sb_b) = (
        toward_fg(bg_r, fg_r),
        toward_fg(bg_g, fg_g),
        toward_fg(bg_b, fg_b),
    );

    format!(
        "@define-color window_bg_color rgb({bg_r},{bg_g},{bg_b});\n\
         @define-color window_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color headerbar_bg_color rgb({bg_r},{bg_g},{bg_b});\n\
         @define-color headerbar_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color popover_bg_color rgb({dl_r},{dl_g},{dl_b});\n\
         @define-color popover_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color dialog_bg_color rgb({dl_r},{dl_g},{dl_b});\n\
         @define-color dialog_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color card_bg_color rgb({dl_r},{dl_g},{dl_b});\n\
         @define-color card_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color view_bg_color rgb({bg_r},{bg_g},{bg_b});\n\
         @define-color view_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color sidebar_bg_color rgb({sb_r},{sb_g},{sb_b});\n\
         @define-color sidebar_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color sidebar_backdrop_color rgb({sb_r},{sb_g},{sb_b});\n\
         @define-color secondary_sidebar_bg_color rgb({sb_r},{sb_g},{sb_b});\n\
         @define-color secondary_sidebar_fg_color rgb({fg_r},{fg_g},{fg_b});\n\
         @define-color secondary_sidebar_backdrop_color rgb({sb_r},{sb_g},{sb_b});\n\
         @define-color headerbar_backdrop_color rgb({bg_r},{bg_g},{bg_b});\n\
         :root {{\n\
         --window-bg-color: rgb({bg_r},{bg_g},{bg_b});\n\
         --window-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --headerbar-bg-color: rgb({bg_r},{bg_g},{bg_b});\n\
         --headerbar-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --popover-bg-color: rgb({dl_r},{dl_g},{dl_b});\n\
         --popover-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --dialog-bg-color: rgb({dl_r},{dl_g},{dl_b});\n\
         --dialog-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --card-bg-color: rgb({dl_r},{dl_g},{dl_b});\n\
         --card-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --view-bg-color: rgb({bg_r},{bg_g},{bg_b});\n\
         --view-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --sidebar-bg-color: rgb({sb_r},{sb_g},{sb_b});\n\
         --sidebar-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --sidebar-backdrop-color: rgb({sb_r},{sb_g},{sb_b});\n\
         --secondary-sidebar-bg-color: rgb({sb_r},{sb_g},{sb_b});\n\
         --secondary-sidebar-fg-color: rgb({fg_r},{fg_g},{fg_b});\n\
         --secondary-sidebar-backdrop-color: rgb({sb_r},{sb_g},{sb_b});\n\
         --headerbar-backdrop-color: rgb({bg_r},{bg_g},{bg_b});\n\
         }}\n\
         @define-color player_icon_color rgb({ic_r},{ic_g},{ic_b});\n"
    )
}

/// Switch the default [`adw::StyleManager`] color scheme by string key.
///
/// Maps `"gtk-light"` to `ForceLight`, `"gtk-dark"` and `"gradient"` to
/// `ForceDark`, `"custom"` to the scheme `custom_bg` reads as, and any other
/// value to `Default` (follow system).
pub fn apply_color_scheme(color_source: &str, custom_bg: (u8, u8, u8)) {
    let scheme = match color_source {
        "gtk-light" => adw::ColorScheme::ForceLight,
        "gtk-dark" | "gradient" => adw::ColorScheme::ForceDark,
        "custom" if is_dark(custom_bg) => adw::ColorScheme::ForceDark,
        "custom" => adw::ColorScheme::ForceLight,
        _ => adw::ColorScheme::Default,
    };
    adw::StyleManager::default().set_color_scheme(scheme);
}

/// Whether `rgb` is darker than the point where black and white text contrast
/// with it equally: WCAG relative luminance below 0.179.
fn is_dark((r, g, b): (u8, u8, u8)) -> bool {
    let linear = |channel: u8| {
        let value = f64::from(channel) / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b) < 0.179
}

/// Display CSS owned by one widget: registered on its display until the widget
/// is destroyed, and replaced in place by [`CssSlot::set`].
///
/// Clones share the registration, so a closure can keep one without keeping
/// the widget.
#[derive(Clone)]
pub struct CssSlot(gtk::CssProvider);

impl CssSlot {
    /// Register an empty slot at `priority` on `owner`'s display for `owner`'s lifetime.
    #[must_use]
    pub fn new(owner: &impl gtk::prelude::IsA<gtk::Widget>, priority: u32) -> Self {
        let provider = gtk::CssProvider::new();
        big_gtk_kit::css::add_provider_for_widget_lifetime(owner, &provider, priority);
        Self(provider)
    }

    /// Replace the slot's rules with `css`; an empty string clears them.
    pub fn set(&self, css: &str) {
        self.0.load_from_string(css);
    }
}

/// Decode a `#RRGGBB` or `RRGGBB` hex string into an `(R, G, B)` tuple.
///
/// The leading `#` is optional. Strings shorter than 6 hex digits, or with
/// non-hex characters, fall back to the BigLinux placeholder `(42, 42, 48)`
/// so callers always receive a renderable triple. The shorthand `#RGB`
/// form is **not** expanded.
#[must_use]
pub fn parse_hex_color(hex_str: &str) -> (u8, u8, u8) {
    let hex = hex_str.trim_start_matches('#');
    // ASCII only: byte slicing inside a multi-byte character would panic.
    if hex.len() >= 6 && hex.is_ascii() {
        (
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(42),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(42),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(48),
        )
    } else {
        (42, 42, 48)
    }
}

/// Decode a hex color string into an opaque [`gtk::gdk::RGBA`].
///
/// Delegates to [`parse_hex_color`], divides each channel by 255, and sets
/// alpha to `1.0`. Inherits the same malformed-input fallback.
#[must_use]
pub fn parse_hex_to_rgba(hex_str: &str) -> gtk::gdk::RGBA {
    let (r, g, b) = parse_hex_color(hex_str);
    gtk::gdk::RGBA::new(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        1.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::prelude::*;

    #[test]
    #[ignore = "requires an isolated GTK session"]
    fn css_slot_leaves_with_its_widget() {
        gtk::init().expect("isolated GTK session");
        // Only widgets under a toplevel restyle when the display's providers change.
        let probe = gtk::Label::new(None);
        probe.add_css_class("css-slot-probe");
        let window = gtk::Window::builder().child(&probe).build();
        window.present();
        let color = || {
            let context = gtk::glib::MainContext::default();
            while context.iteration(false) {}
            probe.color()
        };
        let unstyled = color();
        let red = gtk::gdk::RGBA::new(1.0, 0.0, 0.0, 1.0);
        let blue = gtk::gdk::RGBA::new(0.0, 0.0, 1.0, 1.0);
        for _ in 0..25 {
            let owner = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let slot = CssSlot::new(&owner, gtk::STYLE_PROVIDER_PRIORITY_USER);
            slot.set(".css-slot-probe { color: rgb(255, 0, 0); }");
            assert_eq!(color(), red);
            slot.set("");
            assert_eq!(color(), unstyled, "cleared rules stayed");
            slot.set(".css-slot-probe { color: rgb(0, 0, 255); }");
            assert_eq!(color(), blue);
            let provider = slot.0.downgrade();
            drop((slot, owner));
            assert!(provider.upgrade().is_none(), "the display kept the slot");
            assert_eq!(color(), unstyled, "rules outlived their widget");
        }
        window.destroy();
    }

    #[test]
    fn parse_hex_color_accepts_hash_and_plain() {
        assert_eq!(parse_hex_color("#FF8000"), (255, 128, 0));
        assert_eq!(parse_hex_color("00FF80"), (0, 255, 128));
    }

    #[test]
    fn rgba_channels_scale_to_unit() {
        let rgba = parse_hex_to_rgba("#FF3366");
        assert_eq!((rgba.red(), rgba.green(), rgba.blue()), (1.0, 0.2, 0.4));
        assert_eq!(rgba.alpha(), 1.0);
    }

    #[test]
    fn parse_hex_color_uses_visible_fallback() {
        assert_eq!(parse_hex_color("#FFF"), (42, 42, 48));
        assert_eq!(parse_hex_color(""), (42, 42, 48));
    }

    #[test]
    fn build_overrides_contains_color_contract() {
        let palette = ColorPalette {
            bg: (30, 30, 30),
            fg: (220, 220, 220),
            icon: (100, 150, 200),
            dialog: (40, 40, 40),
        };
        let css = build_adwaita_color_overrides(&palette);
        assert!(css.contains("@define-color window_bg_color rgb(30,30,30);"));
        assert!(css.contains("@define-color player_icon_color rgb(100,150,200);"));
        assert!(css.contains("--dialog-fg-color: rgb(220,220,220);"));
        // 6% of the way from 30 to 220.
        assert!(css.contains("--sidebar-bg-color: rgb(41,41,41);"));
    }

    /// Oracle: WCAG 2 relative luminance against the 0.179 midpoint.
    #[test]
    fn presets_read_as_their_scheme() {
        for dark in [(0x1C, 0x1C, 0x1C), (0x00, 0x2B, 0x36), (0x75, 0x75, 0x75)] {
            assert!(is_dark(dark), "{dark:?}");
        }
        for light in [(0xFF, 0xFF, 0xFF), (0xFD, 0xF6, 0xE3), (0x76, 0x76, 0x76)] {
            assert!(!is_dark(light), "{light:?}");
        }
    }
}
