//! Video-player appearance application: reads the `ui-*` palette + OSD layout
//! settings and pushes the resulting CSS into one app-owned `CssProvider` slot.
//! Covers the palette foundation, OSD widget styles, gradient/button/window
//! customizations, and the high-contrast override — kept app-local because the
//! OSD class names
//! (`osd-controls`, `osd-btn`, …) are video-specific.
//!
//! The provider slot lives on the `VideoShell`; a re-apply (boot or the
//! Appearance dialog's `on_changed`) replaces the slot, never stacks.
//!
//! The controls bar exposes bottom OSD button-zone handles, so the Appearance
//! dialog can reapply `ui-zone-bottom-*` and `ui-show-*` live while this module
//! refreshes the CSS provider.

use crate::settings::{Settings, keys};

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
            win_btn_custom: settings.get_bool(keys::UI_WINDOW_BUTTON_CUSTOM_SIZE),
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
button.adjustment-reset {{
  background: alpha(@window_fg_color, 0.10);
  border-radius: 999px;
  min-height: 30px;
  padding: 0 14px;
  font-weight: bold;
}}
.osd-header headerbar button.header-pill {{
  background: @accent_bg_color;
  color: @accent_fg_color;
  border-radius: 999px;
  min-height: 24px;
  padding: 0 10px;
  margin: 0 4px;
  font-weight: bold;
}}
.osd-header headerbar button.header-pill:hover {{
  background: @accent_bg_color;
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

/// The volume control while no sound plays (muted or at 0): a pill in the
/// theme's warning colours, after every palette rule so no customization
/// dims it. Its text ("No sound") and shape say it too, not colour alone.
const SOUND_OFF_CSS: &str = "
.sound-off {
  background: @warning_bg_color;
  border-radius: 999px;
  padding-right: 8px;
}
.sound-off label,
.sound-off button.osd-btn,
.sound-off button.osd-btn:hover,
.sound-off button.osd-btn:active {
  color: @warning_fg_color;
  text-shadow: none;
}
/* The TV/PC mark beside the level, dimmed elsewhere: dimmed on the
   warning colour it fell short of contrast. */
.sound-off label.dim-label {
  opacity: 1;
}
.sound-off .osd-value-label {
  font-weight: bold;
}
.sound-off .osd-btn image {
  -gtk-icon-shadow: none;
}
";

/// The settings panel: a see-through card (no blur, no shadow, so it adds
/// nothing per video frame) at 80%. What holds controls — blocks, lists,
/// sliders — carries 55% more of its own, 91% in all. That keeps a slider's
/// trough at 3:1 over any frame and its fill at 3:1 but for the light
/// scheme over black, 2.94:1 (60% gave 3.02:1; the card alone at 72% gave
/// 1.7:1 over white). Text straight on the card is at 90% even when
/// secondary, which reads at 5.4:1 or more there. Measured over white and
/// black video in both schemes.
const PANEL_CSS: &str = "
.settings-panel {
  background: alpha(@window_bg_color, 0.80);
  --dim-opacity: 90%;
  color: @window_fg_color;
  border: 1px solid alpha(@window_fg_color, 0.12);
  border-radius: 14px;
}
.settings-panel button.settings-block,
.settings-panel button.settings-link,
.settings-panel button.colour-choice,
.settings-panel button.recent-card,
.settings-panel list.boxed-list,
.settings-panel .panel-backed {
  background-color: alpha(@window_bg_color, 0.55);
}
.settings-panel list.boxed-list {
  background-image: linear-gradient(@card_bg_color, @card_bg_color);
}
.settings-panel .panel-backed {
  border-radius: 10px;
  padding: 8px 10px;
}
.settings-panel button.settings-block {
  background-image: linear-gradient(alpha(@window_fg_color, 0.08), alpha(@window_fg_color, 0.08));
  border-radius: 10px;
  padding: 8px 12px;
  min-height: 52px;
}
.settings-panel .block-state {
  font-weight: normal;
}
.settings-panel button.colour-choice {
  background-image: linear-gradient(alpha(@window_fg_color, 0.08), alpha(@window_fg_color, 0.08));
  border-radius: 10px;
  padding: 8px 10px;
}
.settings-panel button.colour-choice:checked {
  background: @accent_bg_color;
  color: @accent_fg_color;
}
.settings-panel button.settings-link {
  background-image: linear-gradient(alpha(@window_fg_color, 0.05), alpha(@window_fg_color, 0.05));
  border-radius: 8px;
  min-height: 30px;
  padding: 2px 8px;
  font-weight: normal;
}
.settings-panel button.settings-link:hover {
  background-image: linear-gradient(alpha(@window_fg_color, 0.12), alpha(@window_fg_color, 0.12));
}
.settings-panel.compact button.settings-block {
  min-height: 40px;
  padding: 4px 10px;
}
.settings-panel button.settings-block:hover {
  background-image: linear-gradient(alpha(@window_fg_color, 0.14), alpha(@window_fg_color, 0.14));
}
.settings-panel .block-state.block-changed {
  font-weight: bold;
}
.settings-panel button.recent-card {
  background-image: linear-gradient(alpha(@window_fg_color, 0.08), alpha(@window_fg_color, 0.08));
  border-radius: 10px;
  padding: 6px;
  font-weight: normal;
}
.settings-panel button.recent-card:hover {
  background-image: linear-gradient(alpha(@window_fg_color, 0.14), alpha(@window_fg_color, 0.14));
}
.settings-panel .recent-thumb {
  border-radius: 7px;
  background-color: alpha(black, 0.35);
}
.settings-panel .recent-length {
  background: alpha(black, 0.75);
  color: white;
  border-radius: 6px;
  padding: 1px 6px;
  font-size: smaller;
  font-weight: bold;
}
.settings-panel scale trough,
.osd-controls scale trough {
  background-color: alpha(currentColor, 0.55);
}
";

/// Secondary text (descriptions, subtitles, captions) at 75% instead of
/// Adwaita's 55%: 55% reads at about 3.2:1 even on the plain window colour,
/// under WCAG's 4.5:1; 75% gives 5.6:1 there, and over the see-through
/// panel and bars on the worst frame.
const DIM_TEXT_CSS: &str = "
window {
  --dim-opacity: 75%;
}
";

fn build_osd_css(settings: &Settings, osd_css_config: &OsdCssConfig) -> String {
    let mut css = osd_css_config.css_color_foundation(settings);
    css += &osd_css_config.css_widget_styles();
    css += &osd_css_config.css_customizations();
    css += SOUND_OFF_CSS;
    css += PANEL_CSS;
    css += DIM_TEXT_CSS;

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
    if let Some(old) = provider_slot.as_ref()
        && let Some(display) = gtk::gdk::Display::default()
    {
        gtk::style_context_remove_provider_for_display(&display, old);
    }

    let osd_css_config = OsdCssConfig::from_settings(settings);
    crate::shared::theming::apply_color_scheme(&osd_css_config.color_source);
    let css = build_osd_css(settings, &osd_css_config);

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
