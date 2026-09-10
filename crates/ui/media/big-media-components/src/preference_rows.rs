//! Small libadwaita preference-row builders for media apps.
//!
//! These helpers know GTK/libadwaita layout only. Apps pass current values and
//! callbacks, so the same rows work with players, converters, and daemons with
//! optional UI.

use adw::prelude::*;
use big_relm4_components::feedback::tooltip;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::list::button_strip_row::{BigButtonStripRow, BigButtonStripRowSpec};
use gtk::gdk;
use std::cell::RefCell;
use std::rc::Rc;

/// Shared mutable list of widgets toggled by an app-level Advanced switch.
///
/// Apps push widgets that should only be visible in "Advanced" mode into
/// this list and then flip them through a single toggle handler.
pub type AdvancedWidgets = Rc<RefCell<Vec<gtk::Widget>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreferenceSwitchRowSpec {
    title: String,
    subtitle: Option<String>,
    active: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct PreferenceSpinRowSpec {
    title: String,
    subtitle: Option<String>,
    value: f64,
    minimum: f64,
    maximum: f64,
    step: f64,
    page_increment: f64,
    digits: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreferenceEntryRowSpec {
    title: String,
    tooltip_text: Option<String>,
    text: String,
}

// mpv subtitle colors use #AARRGGBB; alpha first.
fn rgba_to_mpv(rgba: gdk::RGBA) -> String {
    let r = (rgba.red() * 255.0) as u8;
    let g = (rgba.green() * 255.0) as u8;
    let b = (rgba.blue() * 255.0) as u8;
    let a = (rgba.alpha() * 255.0) as u8;
    format!("#{a:02X}{r:02X}{g:02X}{b:02X}")
}

fn mpv_to_rgba(color: &str) -> gdk::RGBA {
    let hex = color.trim_start_matches('#');
    if hex.len() == 8 {
        let a = f32::from(u8::from_str_radix(&hex[0..2], 16).unwrap_or(255)) / 255.0;
        let r = f32::from(u8::from_str_radix(&hex[2..4], 16).unwrap_or(255)) / 255.0;
        let g = f32::from(u8::from_str_radix(&hex[4..6], 16).unwrap_or(255)) / 255.0;
        let b = f32::from(u8::from_str_radix(&hex[6..8], 16).unwrap_or(255)) / 255.0;
        gdk::RGBA::new(r, g, b, a)
    } else if hex.len() == 6 {
        let r = f32::from(u8::from_str_radix(&hex[0..2], 16).unwrap_or(255)) / 255.0;
        let g = f32::from(u8::from_str_radix(&hex[2..4], 16).unwrap_or(255)) / 255.0;
        let b = f32::from(u8::from_str_radix(&hex[4..6], 16).unwrap_or(255)) / 255.0;
        gdk::RGBA::new(r, g, b, 1.0)
    } else {
        gdk::RGBA::new(1.0, 1.0, 1.0, 1.0)
    }
}

/// Append an [`adw::SwitchRow`] bound to `on_change` to `group`.
pub fn switch_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    active: bool,
    on_change: impl Fn(bool) + 'static,
) -> adw::SwitchRow {
    let spec = switch_row_spec(title, subtitle, active);
    let row = adw::SwitchRow::builder()
        .title(&spec.title)
        .active(spec.active)
        .build();
    if let Some(subtitle) = spec.subtitle.as_deref() {
        row.set_subtitle(subtitle);
    }
    row.connect_active_notify(move |r| on_change(r.is_active()));
    group.add(&row);
    row
}

/// Append a shared dropdown preference row with `labels` and selection `selected`.
pub fn combo_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    labels: &[&str],
    selected: u32,
    on_change: impl Fn(u32) + 'static,
) -> BigDropdownRow {
    let spec = combo_row_spec(title, subtitle, labels, selected);
    let row = BigDropdownRow::new(spec);
    row.dropdown()
        .connect_selected_notify(move |dropdown| on_change(dropdown.selected()));
    group.add(row.root());
    row
}

/// Append an integer-valued [`adw::SpinRow`] over `[min, max]` with step `step`.
#[allow(clippy::too_many_arguments)]
pub fn spin_int_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    value: i32,
    min: f64,
    max: f64,
    step: f64,
    on_change: impl Fn(i32) + 'static,
) -> adw::SpinRow {
    let spec = spin_int_row_spec(title, subtitle, value, min, max, step);
    let adj = gtk::Adjustment::new(
        spec.value,
        spec.minimum,
        spec.maximum,
        spec.step,
        spec.page_increment,
        0.0,
    );
    let row = adw::SpinRow::builder()
        .title(&spec.title)
        .adjustment(&adj)
        .build();
    if let Some(subtitle) = spec.subtitle.as_deref() {
        row.set_subtitle(subtitle);
    }
    adj.connect_value_changed(move |a| on_change(a.value() as i32));
    group.add(&row);
    row
}

/// Append a floating-point [`adw::SpinRow`] showing `digits` decimal places.
#[allow(clippy::too_many_arguments)]
pub fn spin_double_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    digits: u32,
    on_change: impl Fn(f64) + 'static,
) -> adw::SpinRow {
    let spec = spin_double_row_spec(title, subtitle, value, min, max, step, digits);
    let adj = gtk::Adjustment::new(
        spec.value,
        spec.minimum,
        spec.maximum,
        spec.step,
        spec.page_increment,
        0.0,
    );
    let row = adw::SpinRow::builder()
        .title(&spec.title)
        .adjustment(&adj)
        .digits(spec.digits.unwrap_or_default())
        .build();
    if let Some(subtitle) = spec.subtitle.as_deref() {
        row.set_subtitle(subtitle);
    }
    adj.connect_value_changed(move |a| on_change(a.value()));
    group.add(&row);
    row
}

/// Append an [`adw::EntryRow`] with optional `tooltip_text` to `group`.
pub fn entry_row(
    group: &adw::PreferencesGroup,
    title: &str,
    tooltip_text: &str,
    text: &str,
    on_change: impl Fn(&str) + 'static,
) -> adw::EntryRow {
    let spec = entry_row_spec(title, tooltip_text, text);
    let row = adw::EntryRow::builder()
        .title(&spec.title)
        .text(&spec.text)
        .build();
    if let Some(tooltip_text) = spec.tooltip_text.as_deref() {
        tooltip::set(&row, tooltip_text);
    }
    row.connect_changed(move |r| on_change(&r.text()));
    group.add(&row);
    row
}

/// Append a color picker row that reads and writes mpv's `#AARRGGBB` format.
///
/// mpv subtitle colors put alpha first, which neither `gdk::RGBA` nor most
/// GTK widgets do natively. This helper translates between the two and
/// wires the change callback.
pub fn mpv_color_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    color: &str,
    on_change: impl Fn(&str) + 'static,
) -> adw::ActionRow {
    let dialog = gtk::ColorDialog::builder().with_alpha(true).build();
    let button = gtk::ColorDialogButton::new(Some(dialog));
    button.set_rgba(&mpv_to_rgba(color));
    let spec = mpv_color_row_spec(title, subtitle);
    let row_builder = BigButtonStripRow::new(spec);
    row_builder.append(&button);
    row_builder.root().set_activatable_widget(Some(&button));
    button.connect_rgba_notify(move |b| {
        let color = rgba_to_mpv(b.rgba());
        on_change(&color);
    });
    let row = row_builder.into_root();
    group.add(&row);
    row
}

fn switch_row_spec(title: &str, subtitle: &str, active: bool) -> PreferenceSwitchRowSpec {
    PreferenceSwitchRowSpec {
        title: title.to_string(),
        subtitle: optional_non_empty_text(subtitle),
        active,
    }
}

fn combo_row_spec(
    title: &str,
    subtitle: &str,
    labels: &[&str],
    selected: u32,
) -> BigDropdownRowSpec {
    let mut spec = BigDropdownRowSpec::new(title, labels.iter().copied()).selected(selected);
    if let Some(subtitle) = optional_non_empty_text(subtitle) {
        spec = spec.subtitle(subtitle);
    }
    spec
}

fn spin_int_row_spec(
    title: &str,
    subtitle: &str,
    value: i32,
    minimum: f64,
    maximum: f64,
    step: f64,
) -> PreferenceSpinRowSpec {
    spin_row_spec(
        title,
        subtitle,
        f64::from(value),
        minimum,
        maximum,
        step,
        None,
    )
}

fn spin_double_row_spec(
    title: &str,
    subtitle: &str,
    value: f64,
    minimum: f64,
    maximum: f64,
    step: f64,
    digits: u32,
) -> PreferenceSpinRowSpec {
    spin_row_spec(title, subtitle, value, minimum, maximum, step, Some(digits))
}

fn spin_row_spec(
    title: &str,
    subtitle: &str,
    value: f64,
    minimum: f64,
    maximum: f64,
    step: f64,
    digits: Option<u32>,
) -> PreferenceSpinRowSpec {
    PreferenceSpinRowSpec {
        title: title.to_string(),
        subtitle: optional_non_empty_text(subtitle),
        value,
        minimum,
        maximum,
        step,
        page_increment: spin_page_increment(step),
        digits,
    }
}

fn entry_row_spec(title: &str, tooltip_text: &str, text: &str) -> PreferenceEntryRowSpec {
    PreferenceEntryRowSpec {
        title: title.to_string(),
        tooltip_text: optional_non_empty_text(tooltip_text),
        text: text.to_string(),
    }
}

fn mpv_color_row_spec(title: &str, subtitle: &str) -> BigButtonStripRowSpec {
    let mut spec = BigButtonStripRowSpec::new(title);
    if let Some(subtitle) = optional_non_empty_text(subtitle) {
        spec = spec.subtitle(subtitle);
    }
    spec
}

fn optional_non_empty_text(text: &str) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

fn spin_page_increment(step: f64) -> f64 {
    step * 10.0
}

#[cfg(test)]
mod tests {
    use super::{
        combo_row_spec, entry_row_spec, mpv_color_row_spec, mpv_to_rgba, optional_non_empty_text,
        rgba_to_mpv, spin_double_row_spec, spin_int_row_spec, spin_page_increment, switch_row_spec,
    };

    #[test]
    fn mpv_color_roundtrip_keeps_alpha_first_format() {
        let rgba = mpv_to_rgba("#80402010");
        assert_eq!(rgba_to_mpv(rgba), "#80402010");
    }

    #[test]
    fn short_mpv_color_defaults_to_opaque() {
        let rgba = mpv_to_rgba("#102030");
        assert_eq!(rgba_to_mpv(rgba), "#FF102030");
    }

    #[test]
    fn optional_text_contract_omits_empty_subtitles_and_tooltips() {
        assert_eq!(optional_non_empty_text(""), None);
        assert_eq!(optional_non_empty_text("Shown"), Some("Shown".to_string()));
    }

    #[test]
    fn switch_and_combo_specs_preserve_subtitle_and_selection_contracts() {
        let switch_spec = switch_row_spec("Shuffle", "Keep current track first", true);
        assert_eq!(switch_spec.title, "Shuffle");
        assert_eq!(
            switch_spec.subtitle,
            Some("Keep current track first".to_string())
        );
        assert!(switch_spec.active);

        let combo_spec = combo_row_spec("Output", "", &["Join", "Separate"], 1);
        assert_eq!(combo_spec.title, "Output");
        assert_eq!(combo_spec.subtitle, None);
        assert_eq!(combo_spec.labels, ["Join", "Separate"]);
        assert_eq!(combo_spec.selected, 1);
    }

    #[test]
    fn spin_row_specs_use_page_increment_and_decimal_contracts() {
        let integer_spec = spin_int_row_spec("Volume", "Percent", 50, 0.0, 150.0, 5.0);
        assert_eq!(integer_spec.title, "Volume");
        assert_eq!(integer_spec.subtitle, Some("Percent".to_string()));
        assert_eq!(integer_spec.value, 50.0);
        assert_eq!(integer_spec.minimum, 0.0);
        assert_eq!(integer_spec.maximum, 150.0);
        assert_eq!(integer_spec.step, 5.0);
        assert_eq!(integer_spec.page_increment, 50.0);
        assert_eq!(integer_spec.digits, None);
        assert_eq!(spin_page_increment(0.25), 2.5);

        let double_spec = spin_double_row_spec("Speed", "", 1.25, 0.25, 4.0, 0.25, 2);
        assert_eq!(double_spec.subtitle, None);
        assert_eq!(double_spec.value, 1.25);
        assert_eq!(double_spec.page_increment, 2.5);
        assert_eq!(double_spec.digits, Some(2));
    }

    #[test]
    fn entry_and_color_specs_keep_optional_text_contracts() {
        let entry_spec = entry_row_spec("Command", "Shell-free argv", "mpv");
        assert_eq!(entry_spec.title, "Command");
        assert_eq!(entry_spec.tooltip_text, Some("Shell-free argv".to_string()));
        assert_eq!(entry_spec.text, "mpv");

        let color_spec = mpv_color_row_spec("Subtitle color", "Alpha first");
        let resolved = color_spec.resolved();
        assert_eq!(resolved.title, "Subtitle color");
        assert_eq!(resolved.subtitle, Some("Alpha first".to_string()));

        let no_subtitle_color_spec = mpv_color_row_spec("Subtitle color", "");
        assert_eq!(no_subtitle_color_spec.resolved().subtitle, None);
    }
}
