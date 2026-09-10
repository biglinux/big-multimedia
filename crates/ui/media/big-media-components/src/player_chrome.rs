//! Player-UI chrome helpers shared across media-player front-ends: off-thread
//! task spawn, play-button pulse, and the volume/speed/voice hover popovers.
//!
//! Extracted from `big-media-runtime` so reusable player panels (`big-player-ui`)
//! can use them without depending on the engine runtime. i18n routes through the
//! crate translator hook ([`crate::text::text`]); runtime re-exports these under
//! the legacy `shared::{spawn,play_button,popovers}` paths.

use std::time::Duration;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;

use crate::text::text as i18n;
use big_relm4_components::feedback::hover_popover::{
    build_vertical_popover_shell, next_cyclic_preset,
};
use big_relm4_components::input::scale_popover::{
    BigVerticalScalePopoverSpec, vertical_scale_popover,
};
use big_relm4_components::input::scale_trough::{
    update_scale_trough, voice_trough_color, volume_trough_color,
};

pub use big_relm4_components::feedback::hover_popover::{
    setup_exclusive_hover_popover_group, setup_hover_controller_exclusive,
};

const PLAY_BUTTON_PULSE_CSS_CLASS: &str = "play-pulse";
const PLAY_BUTTON_PULSE_MILLISECONDS: u64 = 250;
const PLAYER_SCALE_HEIGHT_REQUEST: i32 = 200;
const TROUGH_STYLE_PROVIDER_PRIORITY_OFFSET: u32 = 2;
const VOLUME_MINIMUM_PERCENT: f64 = 0.0;
const VOLUME_STEP_PERCENT: f64 = 1.0;
const VOLUME_REFERENCE_MARK_PERCENT: f64 = 100.0;
const SPEED_MINIMUM_MULTIPLIER: f64 = 0.25;
const SPEED_MAXIMUM_MULTIPLIER: f64 = 4.0;
const SPEED_STEP_MULTIPLIER: f64 = 0.25;
const SPEED_INITIAL_MULTIPLIER: f64 = 1.0;
const SPEED_REFERENCE_MARK_MULTIPLIER: f64 = 1.0;
const SPEED_PRESET_EPSILON: f64 = 0.01;
const VOICE_SHELL_SPACING: i32 = 2;
const VOICE_SETTINGS_ICON_NAME: &str = "emblem-system-symbolic";
const VOICE_MINIMUM_PERCENT: f64 = 0.0;
const VOICE_MAXIMUM_PERCENT: f64 = 100.0;
const VOICE_STEP_PERCENT: f64 = 1.0;
const VOICE_SCALE_WIDTH_REQUEST: i32 = -1;
const VOICE_REFERENCE_MARK_PERCENT: f64 = 50.0;
const VOICE_PRESET_EPSILON: f64 = 0.5;

#[derive(Debug, Clone, PartialEq)]
struct VoicePopoverSpec {
    title: String,
    shell_spacing: i32,
    settings_icon_name: &'static str,
    settings_accessible_label: String,
    scale_accessible_label: String,
    scale_minimum_percent: f64,
    scale_maximum_percent: f64,
    scale_step_percent: f64,
    scale_width_request: i32,
    scale_height_request: i32,
    reference_mark_percent: f64,
    initial_strength_percent: f64,
}

// ── off-thread task helpers ────────────────────────────────────────────────

// Promoted to `big_relm4_components::task` so non-media GTK apps reuse them;
// re-exported here for the existing media callers.
pub use big_relm4_components::task::{spawn_blocking_result, spawn_blocking_task};

// ── play button ────────────────────────────────────────────────────────────

/// Briefly pulse the play/pause button (CSS class for ~250ms).
pub fn pulse_play_button(button: &gtk::Button) {
    button.add_css_class(play_button_pulse_css_class());
    let pulsed_button = button.clone();
    glib::timeout_add_local_once(play_button_pulse_duration(), move || {
        pulsed_button.remove_css_class(play_button_pulse_css_class());
    });
}

// ── popovers ───────────────────────────────────────────────────────────────

/// Cycled by clicking the speed button while popover is open.
pub const SPEED_PRESETS: &[f64] = &[1.0, 1.25, 1.50, 1.75, 2.0];

/// Cycled by clicking the voice button while popover is open.
pub const VOICE_PRESETS: &[f64] = &[0.0, 50.0, 70.0, 80.0, 90.0, 100.0];

/// Trough gradient (accent → warning → error) wired internally; caller wires
/// `value-changed` for mpv/settings/mute.
#[must_use]
pub fn build_volume_popover(
    parent: &gtk::Box,
    maximum_volume_percent: f64,
    initial_volume_percent: f64,
) -> (gtk::Popover, gtk::Scale) {
    let (popover, scale) = vertical_scale_popover(
        parent,
        volume_scale_popover_spec(maximum_volume_percent, initial_volume_percent),
    );
    scale.add_css_class("volume-scale");
    scale.add_mark(
        volume_reference_mark_percent(),
        popover_scale_mark_position(),
        None,
    );

    let trough = gtk::CssProvider::new();
    update_scale_trough(
        &trough,
        "volume-scale",
        &volume_trough_color(initial_volume_percent, maximum_volume_percent),
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &trough,
            trough_style_provider_priority(),
        );
    }
    {
        let maximum_volume_for_trough = maximum_volume_percent;
        scale.connect_value_changed(move |s| {
            update_scale_trough(
                &trough,
                "volume-scale",
                &volume_trough_color(s.value(), maximum_volume_for_trough),
            );
        });
    }

    (popover, scale)
}

/// `speed_button` toggles the popover or cycles [`SPEED_PRESETS`] when visible.
/// Updates `speed_label` (`"{:.2}x"`) and invokes `on_speed_changed`.
pub fn build_speed_popover(
    parent: &gtk::Box,
    speed_button: &gtk::Button,
    speed_label: &gtk::Label,
    on_speed_changed: impl Fn(f64) + 'static,
) -> gtk::Popover {
    let (popover, scale) = vertical_scale_popover(parent, speed_scale_popover_spec());
    scale.add_mark(
        SPEED_REFERENCE_MARK_MULTIPLIER,
        popover_scale_mark_position(),
        None,
    );

    {
        let speed_value_label = speed_label.clone();
        scale.connect_value_changed(move |scale| {
            let speed = scale.value();
            speed_value_label.set_label(&speed_label_text(speed));
            on_speed_changed(speed);
        });
    }

    // Click always cycles the presets (the slider is reached by hovering the
    // control, like the voice/volume controls). `set_value` fires the scale's
    // `value_changed` handler, which updates the label and the mpv speed.
    {
        let speed_scale = scale;
        speed_button.connect_clicked(move |_| {
            let current_speed = speed_scale.value();
            if let Some(next_speed) = next_speed_preset(current_speed) {
                speed_scale.set_value(next_speed);
            }
        });
    }

    popover
}

/// Layout: title → gear ⚙ → vertical slider → value label. Trough + both
/// labels (in-popover + `voice_label`) auto-update. `on_value_changed` gets
/// the 0–100 pct; `on_settings_clicked` fires on gear press.
pub fn build_voice_popover(
    parent: &gtk::Box,
    voice_button: &gtk::Button,
    voice_label: &gtk::Label,
    initial_strength_percent: f64,
    on_value_changed: impl Fn(f64) + 'static,
    on_settings_clicked: impl Fn() + 'static,
) -> gtk::Popover {
    let voice_spec = voice_popover_spec(initial_strength_percent);
    let (popover, column) =
        build_vertical_popover_shell(parent, &voice_spec.title, voice_spec.shell_spacing);

    let gear = gtk::Button::builder()
        .icon_name(voice_spec.settings_icon_name)
        .has_frame(false)
        .halign(gtk::Align::Center)
        .build();
    gear.add_css_class("flat");
    big_relm4_components::feedback::tooltip::set(&gear, &voice_spec.settings_accessible_label);
    gear.update_property(&[gtk::accessible::Property::Label(
        &voice_spec.settings_accessible_label,
    )]);
    column.append(&gear);

    let scale = gtk::Scale::with_range(
        gtk::Orientation::Vertical,
        voice_spec.scale_minimum_percent,
        voice_spec.scale_maximum_percent,
        voice_spec.scale_step_percent,
    );
    scale.set_inverted(true);
    scale.set_size_request(
        voice_spec.scale_width_request,
        voice_spec.scale_height_request,
    );
    scale.set_draw_value(false);
    scale.add_css_class("voice-scale");
    scale.update_property(&[gtk::accessible::Property::Label(
        &voice_spec.scale_accessible_label,
    )]);
    scale.add_mark(
        voice_spec.reference_mark_percent,
        popover_scale_mark_position(),
        None,
    );
    scale.set_value(voice_spec.initial_strength_percent);
    column.append(&scale);

    let trough = gtk::CssProvider::new();
    update_scale_trough(
        &trough,
        "voice-scale",
        &voice_trough_color(voice_spec.initial_strength_percent),
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &trough,
            trough_style_provider_priority(),
        );
    }

    let initial_text = voice_strength_label_text(voice_spec.initial_strength_percent);
    let val_label = gtk::Label::new(Some(&initial_text));
    val_label.add_css_class("dim-label");
    column.append(&val_label);

    voice_label.set_label(&initial_text);

    {
        let external_voice_label = voice_label.clone();
        scale.connect_value_changed(move |scale| {
            let voice_strength = scale.value();
            update_scale_trough(&trough, "voice-scale", &voice_trough_color(voice_strength));
            let text = voice_strength_label_text(voice_strength);
            val_label.set_label(&text);
            external_voice_label.set_label(&text);
            on_value_changed(voice_strength);
        });
    }

    gear.connect_clicked(move |_| {
        on_settings_clicked();
    });

    // Click: cycle presets if visible, else show popover.
    {
        let voice_popover = popover.clone();
        let voice_scale = scale;
        voice_button.connect_clicked(move |_| {
            if voice_popover.is_visible() {
                let current_voice_strength = voice_scale.value();
                if let Some(next_voice_strength) = next_voice_preset(current_voice_strength) {
                    voice_scale.set_value(next_voice_strength);
                }
            } else {
                voice_popover.popup();
            }
        });
    }

    popover
}

fn play_button_pulse_css_class() -> &'static str {
    PLAY_BUTTON_PULSE_CSS_CLASS
}

fn play_button_pulse_duration() -> Duration {
    Duration::from_millis(PLAY_BUTTON_PULSE_MILLISECONDS)
}

fn volume_scale_popover_spec(
    maximum_volume_percent: f64,
    initial_volume_percent: f64,
) -> BigVerticalScalePopoverSpec {
    BigVerticalScalePopoverSpec::new(i18n("Volume"), i18n("Volume level"))
        .range(
            VOLUME_MINIMUM_PERCENT,
            maximum_volume_percent,
            VOLUME_STEP_PERCENT,
        )
        .initial(initial_volume_percent)
        .height_request(PLAYER_SCALE_HEIGHT_REQUEST)
}

fn speed_scale_popover_spec() -> BigVerticalScalePopoverSpec {
    BigVerticalScalePopoverSpec::new(i18n("Speed"), i18n("Playback speed"))
        .range(
            SPEED_MINIMUM_MULTIPLIER,
            SPEED_MAXIMUM_MULTIPLIER,
            SPEED_STEP_MULTIPLIER,
        )
        .initial(SPEED_INITIAL_MULTIPLIER)
        .height_request(PLAYER_SCALE_HEIGHT_REQUEST)
}

fn voice_popover_spec(initial_strength_percent: f64) -> VoicePopoverSpec {
    VoicePopoverSpec {
        title: i18n("Voice"),
        shell_spacing: VOICE_SHELL_SPACING,
        settings_icon_name: VOICE_SETTINGS_ICON_NAME,
        settings_accessible_label: i18n("Noise reduction settings"),
        scale_accessible_label: i18n("Voice enhancement"),
        scale_minimum_percent: VOICE_MINIMUM_PERCENT,
        scale_maximum_percent: VOICE_MAXIMUM_PERCENT,
        scale_step_percent: VOICE_STEP_PERCENT,
        scale_width_request: VOICE_SCALE_WIDTH_REQUEST,
        scale_height_request: PLAYER_SCALE_HEIGHT_REQUEST,
        reference_mark_percent: VOICE_REFERENCE_MARK_PERCENT,
        initial_strength_percent,
    }
}

fn volume_reference_mark_percent() -> f64 {
    VOLUME_REFERENCE_MARK_PERCENT
}

fn popover_scale_mark_position() -> gtk::PositionType {
    gtk::PositionType::Right
}

fn trough_style_provider_priority() -> u32 {
    gtk::STYLE_PROVIDER_PRIORITY_USER + TROUGH_STYLE_PROVIDER_PRIORITY_OFFSET
}

fn speed_label_text(speed: f64) -> String {
    format!("{speed:.2}x")
}

fn voice_strength_label_text(strength_percent: f64) -> String {
    format!("{strength_percent:.0}")
}

fn next_speed_preset(current_speed: f64) -> Option<f64> {
    next_cyclic_preset(current_speed, SPEED_PRESETS, SPEED_PRESET_EPSILON)
}

fn next_voice_preset(current_strength_percent: f64) -> Option<f64> {
    next_cyclic_preset(
        current_strength_percent,
        VOICE_PRESETS,
        VOICE_PRESET_EPSILON,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_button_pulse_contract_names_class_and_duration() {
        assert_eq!(play_button_pulse_css_class(), "play-pulse");
        assert_eq!(play_button_pulse_duration(), Duration::from_millis(250));
    }

    #[test]
    fn volume_popover_spec_exposes_accessible_scale_contract() {
        let spec = volume_scale_popover_spec(150.0, 75.0);

        assert_eq!(spec.title, "Volume");
        assert_eq!(spec.accessible_label, "Volume level");
        assert_eq!(spec.minimum, 0.0);
        assert_eq!(spec.maximum, 150.0);
        assert_eq!(spec.step, 1.0);
        assert_eq!(spec.initial, 75.0);
        assert_eq!(spec.height_request, 200);
        assert!(spec.inverted);
        assert_eq!(volume_reference_mark_percent(), 100.0);
        assert_eq!(popover_scale_mark_position(), gtk::PositionType::Right);
    }

    #[test]
    fn trough_style_provider_priority_stays_above_user_css() {
        assert_eq!(
            trough_style_provider_priority(),
            gtk::STYLE_PROVIDER_PRIORITY_USER + 2
        );
        assert!(trough_style_provider_priority() > gtk::STYLE_PROVIDER_PRIORITY_USER);
    }

    #[test]
    fn speed_popover_spec_and_preset_cycle_match_player_controls() {
        let spec = speed_scale_popover_spec();

        assert_eq!(spec.title, "Speed");
        assert_eq!(spec.accessible_label, "Playback speed");
        assert_eq!(spec.minimum, 0.25);
        assert_eq!(spec.maximum, 4.0);
        assert_eq!(spec.step, 0.25);
        assert_eq!(spec.initial, 1.0);
        assert_eq!(spec.height_request, 200);
        assert_eq!(SPEED_REFERENCE_MARK_MULTIPLIER, 1.0);
        assert_eq!(speed_label_text(1.25), "1.25x");
        assert_eq!(speed_label_text(1.0), "1.00x");
        assert_eq!(next_speed_preset(1.0), Some(1.25));
        assert_eq!(next_speed_preset(1.249), Some(1.5));
        assert_eq!(next_speed_preset(2.0), Some(1.0));
    }

    #[test]
    fn voice_popover_spec_and_preset_cycle_match_noise_controls() {
        let spec = voice_popover_spec(70.0);

        assert_eq!(spec.title, "Voice");
        assert_eq!(spec.shell_spacing, 2);
        assert_eq!(spec.settings_icon_name, "emblem-system-symbolic");
        assert_eq!(spec.settings_accessible_label, "Noise reduction settings");
        assert_eq!(spec.scale_accessible_label, "Voice enhancement");
        assert_eq!(spec.scale_minimum_percent, 0.0);
        assert_eq!(spec.scale_maximum_percent, 100.0);
        assert_eq!(spec.scale_step_percent, 1.0);
        assert_eq!(spec.scale_width_request, -1);
        assert_eq!(spec.scale_height_request, 200);
        assert_eq!(spec.reference_mark_percent, 50.0);
        assert_eq!(spec.initial_strength_percent, 70.0);
        assert_eq!(voice_strength_label_text(70.0), "70");
        assert_eq!(voice_strength_label_text(70.6), "71");
        assert_eq!(next_voice_preset(70.0), Some(80.0));
        assert_eq!(next_voice_preset(99.6), Some(0.0));
        assert_eq!(next_voice_preset(100.0), Some(0.0));
    }
}
