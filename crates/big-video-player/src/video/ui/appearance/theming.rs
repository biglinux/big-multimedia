//! Video-player appearance application: reads the `ui-*` palette + OSD layout
//! settings and pushes the resulting CSS into one app-owned `CssProvider` slot.
//! Ported from the old god-object's `window/theming.rs` (same palette
//! foundation, OSD widget styles, gradient/button/window customizations, and
//! high-contrast override) — kept app-local because the OSD class names
//! (`osd-controls`, `osd-btn`, …) are video-specific.
//!
//! The provider slot lives on the `VideoShell`; a re-apply (boot or the
//! Appearance dialog's `on_changed`) replaces the slot, never stacks.
//!
//! The controls bar exposes bottom OSD button-zone handles, so the Appearance
//! dialog can reapply `ui-zone-bottom-*` and `ui-show-*` live while this module
//! refreshes the CSS provider.

use crate::settings::{keys, Settings};

/// All `ui-*` settings the OSD CSS reads, snapshotted once per apply.
struct OsdCssConfig {
    color_source: String,
    bg_opacity: f64,
    menu_opacity: f64,
    ctrl_radius: i32,
    margin_bottom: i32,
    margin_side: i32,
    pad_top: i32,
    pad_bottom: i32,
    pad_left: i32,
    pad_right: i32,
    btn_custom: bool,
    btn_size: i32,
    btn_radius: i32,
    win_radius_custom: bool,
    win_radius: i32,
    win_btn_custom: bool,
    osd_shadow: bool,
}

impl OsdCssConfig {
    fn from_settings(settings: &Settings) -> Self {
        Self {
            color_source: settings.get_string(keys::UI_COLOR_SOURCE),
            bg_opacity: settings.get_double(keys::UI_BG_OPACITY),
            menu_opacity: settings.get_double(keys::UI_MENU_OPACITY),
            ctrl_radius: settings.get_int(keys::UI_CONTROLS_RADIUS),
            margin_bottom: settings.get_int(keys::UI_CONTROLS_MARGIN_BOTTOM),
            margin_side: settings.get_int(keys::UI_CONTROLS_MARGIN_SIDE),
            pad_top: settings.get_int(keys::UI_CONTROLS_PADDING_TOP),
            pad_bottom: settings.get_int(keys::UI_CONTROLS_PADDING_BOTTOM),
            pad_left: settings.get_int(keys::UI_CONTROLS_PADDING_LEFT),
            pad_right: settings.get_int(keys::UI_CONTROLS_PADDING_RIGHT),
            btn_custom: settings.get_bool(keys::UI_BUTTON_CUSTOM),
            btn_size: settings.get_int(keys::UI_BUTTON_SIZE),
            btn_radius: settings.get_int(keys::UI_BUTTON_RADIUS),
            win_radius_custom: settings.get_bool(keys::UI_WINDOW_RADIUS_CUSTOM),
            win_radius: settings.get_int(keys::UI_WINDOW_RADIUS),
            win_btn_custom: settings.get_bool(keys::UI_WINDOW_BTN_CUSTOM_SIZE),
            osd_shadow: settings.get_bool(keys::UI_OSD_SHADOW),
        }
    }

    // Theme mode aliases `player_icon_color` to `window_fg_color`.
    fn css_color_foundation(&self, settings: &Settings) -> String {
        crate::shared::theming::build_custom_palette_css(
            settings,
            &self.color_source,
            keys::UI_BG_COLOR,
            keys::UI_FONT_COLOR,
            keys::UI_ICON_COLOR,
            keys::UI_DIALOG_COLOR,
        )
        .unwrap_or_else(|| "@define-color player_icon_color @window_fg_color;\n".to_string())
    }

    fn css_widget_styles(&self) -> String {
        format!(
            r"
.osd-controls {{
  background: alpha(@window_bg_color, {bg_op:.2});
  border-radius: {cr}px;
  margin-bottom: {mb}px;
  margin-left: {ms}px;
  margin-right: {ms}px;
  padding-top: {pt}px;
  padding-bottom: {pb}px;
  padding-left: {pl}px;
  padding-right: {pr}px;
}}
.osd-header {{
  background: alpha(@headerbar_bg_color, {bg_op:.2});
}}
.osd-btn,
.osd-btn > button,
menubutton.osd-btn > button {{
  background: transparent;
  color: @player_icon_color;
}}
button.osd-btn:hover,
menubutton.osd-btn > button:hover {{
  background: alpha(@window_fg_color, 0.12);
  color: @player_icon_color;
}}
button.osd-btn:active, button.osd-btn:checked,
menubutton.osd-btn > button:active,
menubutton.osd-btn > button:checked {{
  background: alpha(@window_fg_color, 0.20);
  color: @player_icon_color;
}}
.osd-btn-play,
.osd-btn-play > button {{
  background: transparent;
  color: @player_icon_color;
}}
.osd-btn-play:hover,
.osd-btn-play > button:hover {{
  background: alpha(@window_fg_color, 0.12);
  color: @player_icon_color;
}}
.osd-btn-play:active,
.osd-btn-play > button:active {{
  background: alpha(@window_fg_color, 0.20);
  color: @player_icon_color;
}}
.time-label, .osd-value-label, .title-label {{
  color: @window_fg_color;
}}
.osd-header headerbar {{
  min-height: 0;
  padding-top: 0;
  padding-bottom: 0;
}}
.osd-header headerbar button.osd-btn,
.osd-header headerbar menubutton.osd-btn > button {{
  min-width: 24px;
  min-height: 24px;
  padding: 4px;
  -gtk-icon-size: 16px;
}}
.osd-header headerbar windowcontrols,
.osd-header windowcontrols {{
  background: transparent;
  color: @player_icon_color;
}}
windowcontrols button image {{
  color: @player_icon_color;
}}
popover > contents {{
  background: alpha(@popover_bg_color, {menu_op:.2});
}}
popover > arrow {{
  background: alpha(@popover_bg_color, {menu_op:.2});
}}
",
            bg_op = self.bg_opacity,
            menu_op = self.menu_opacity,
            cr = self.ctrl_radius,
            mb = self.margin_bottom,
            ms = self.margin_side,
            pt = self.pad_top,
            pb = self.pad_bottom,
            pl = self.pad_left,
            pr = self.pad_right,
        )
    }

    fn css_customizations(&self) -> String {
        let mut css = String::new();

        if self.color_source == "gradient" {
            css += &format!(
                ".osd-controls {{\n\
                   background: linear-gradient(to top, rgba(0,0,0,{op:.2}), transparent);\n\
                   padding-top: 80px;\n\
                 }}\n\
                 .osd-header {{\n\
                   background: linear-gradient(to bottom, rgba(0,0,0,{op:.2}), transparent);\n\
                   padding-bottom: 80px;\n\
                 }}\n",
                op = self.bg_opacity,
            );
        }

        if self.btn_custom {
            let icon_sz = (self.btn_size / 2).max(12);
            let play_sz = (self.btn_size * 4 / 3).max(32);
            let play_isz = (play_sz / 2).max(16);
            css += &format!(
                ".osd-btn,\n\
                 .osd-btn > button,\n\
                 menubutton.osd-btn > button {{\n\
                   min-width: {bsz}px;\n\
                   min-height: {bsz}px;\n\
                   border-radius: {brad}px;\n\
                   padding: 0;\n\
                   -gtk-icon-size: {isz}px;\n\
                 }}\n\
                 .osd-header headerbar button.osd-btn,\n\
                 .osd-header headerbar menubutton.osd-btn > button {{\n\
                   min-width: {bsz}px;\n\
                   min-height: {bsz}px;\n\
                   padding: 0;\n\
                   -gtk-icon-size: {isz}px;\n\
                 }}\n\
                 .osd-btn-play,\n\
                 .osd-btn-play > button {{\n\
                   min-width: {psz}px;\n\
                   min-height: {psz}px;\n\
                   -gtk-icon-size: {pisz}px;\n\
                 }}\n",
                bsz = self.btn_size,
                brad = self.btn_radius,
                isz = icon_sz,
                psz = play_sz,
                pisz = play_isz
            );
        }

        if self.win_radius_custom {
            css += &format!(
                "window.csd:not(.maximized):not(.fullscreen) {{\n\
                   border-radius: {}px;\n\
                 }}\n",
                self.win_radius
            );
        }

        if self.win_btn_custom {
            css += &format!(
                "windowcontrols button {{\n\
                   min-width: {bsz}px;\n\
                   min-height: {bsz}px;\n\
                   border-radius: {brad}px;\n\
                 }}\n",
                bsz = self.btn_size,
                brad = self.btn_radius
            );
        }

        // 8-position text-shadow simulates a 1px stroke (no real CSS stroke).
        // `@window_bg_color` auto-adapts to light/dark/custom theme.
        if self.osd_shadow {
            css += "\
                 .time-label, .osd-value-label, .title-label {\n\
                   text-shadow:\n\
                     -1px -1px 0 @window_bg_color,  1px -1px 0 @window_bg_color,\n\
                     -1px  1px 0 @window_bg_color,  1px  1px 0 @window_bg_color,\n\
                      0px -1px 0 @window_bg_color,  0px  1px 0 @window_bg_color,\n\
                     -1px  0px 0 @window_bg_color,  1px  0px 0 @window_bg_color;\n\
                 }\n\
                 .osd-btn image,\n\
                 .osd-btn-play image,\n\
                 menubutton.osd-btn > button image,\n\
                 windowcontrols button image,\n\
                 .osd-header headerbar .osd-btn image {\n\
                   -gtk-icon-shadow:\n\
                     -1px -1px 0 @window_bg_color,  1px -1px 0 @window_bg_color,\n\
                     -1px  1px 0 @window_bg_color,  1px  1px 0 @window_bg_color,\n\
                      0px -1px 0 @window_bg_color,  0px  1px 0 @window_bg_color,\n\
                     -1px  0px 0 @window_bg_color,  1px  0px 0 @window_bg_color;\n\
                 }\n";
        }

        css
    }
}

fn build_osd_css(settings: &Settings, cfg: &OsdCssConfig) -> String {
    let mut css = cfg.css_color_foundation(settings);
    css += &cfg.css_widget_styles();
    css += &cfg.css_customizations();

    if settings.get_bool(keys::AUDIO_HIGH_CONTRAST) {
        css += crate::shared::theming::HIGH_CONTRAST_CSS;
    }

    css
}

/// Apply the saved colour source, OSD palette + layout CSS, and high-contrast
/// override. `provider_slot` is the persistent provider slot (replaced in
/// place, never stacked). Call at boot and from the Appearance dialog's
/// `on_changed`.
pub(in crate::video) fn apply_appearance(
    settings: &Settings,
    provider_slot: &mut Option<gtk::CssProvider>,
) {
    if let Some(old) = provider_slot.as_ref() {
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_remove_provider_for_display(&display, old);
        }
    }

    let cfg = OsdCssConfig::from_settings(settings);
    crate::shared::theming::apply_color_scheme(&cfg.color_source);
    let css = build_osd_css(settings, &cfg);

    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css);
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER + 1,
        );
    }
    *provider_slot = Some(provider);
}
