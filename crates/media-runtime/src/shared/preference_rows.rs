//! Settings-bound adapters for generic media preference rows.

use crate::settings::Settings;

pub fn add_switch_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    key: &str,
    settings: &Settings,
    on_change: impl Fn(bool) + 'static,
) -> adw::SwitchRow {
    let s = settings.clone();
    let k = key.to_string();
    big_media_components::preference_rows::switch_row(
        group,
        title,
        subtitle,
        settings.boolean(key),
        move |active| {
            let _ = s.set_boolean(&k, active);
            on_change(active);
        },
    )
}

pub fn add_combo_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    labels: &[&str],
    values: &'static [&'static str],
    key: &str,
    settings: &Settings,
    on_change: impl Fn(&str) + 'static,
) -> adw::ComboRow {
    let current = settings.string(key);
    let idx = values
        .iter()
        .position(|v| *v == current.as_str())
        .unwrap_or(0) as u32;
    let s = settings.clone();
    let k = key.to_string();
    big_media_components::preference_rows::combo_row(
        group,
        title,
        subtitle,
        labels,
        idx,
        move |selected| {
            if let Some(val) = values.get(selected as usize) {
                let _ = s.set_string(&k, val);
                on_change(val);
            }
        },
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the component row it binds"
)]
pub fn add_spin_int_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    key: &str,
    min: f64,
    max: f64,
    step: f64,
    settings: &Settings,
    on_change: impl Fn(i64) + 'static,
) -> adw::SpinRow {
    let s = settings.clone();
    let k = key.to_string();
    big_media_components::preference_rows::spin_int_row(
        group,
        title,
        subtitle,
        settings.int(key),
        min,
        max,
        step,
        move |value| {
            let _ = s.set_int(&k, value);
            on_change(i64::from(value));
        },
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the component row it binds"
)]
pub fn add_spin_double_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    key: &str,
    min: f64,
    max: f64,
    step: f64,
    digits: u32,
    settings: &Settings,
    on_change: impl Fn(f64) + 'static,
) -> adw::SpinRow {
    let s = settings.clone();
    let k = key.to_string();
    big_media_components::preference_rows::spin_double_row(
        group,
        title,
        subtitle,
        settings.double(key),
        min,
        max,
        step,
        digits,
        move |value| {
            let _ = s.set_double(&k, value);
            on_change(value);
        },
    )
}

pub fn add_entry_row(
    group: &adw::PreferencesGroup,
    title: &str,
    tooltip: &str,
    key: &str,
    settings: &Settings,
    on_change: impl Fn(&str) + 'static,
) -> adw::EntryRow {
    let s = settings.clone();
    let k = key.to_string();
    big_media_components::preference_rows::entry_row(
        group,
        title,
        tooltip,
        &settings.string(key),
        move |text| {
            let _ = s.set_string(&k, text);
            on_change(text);
        },
    )
}

pub fn add_color_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    key: &str,
    settings: &Settings,
    on_change: impl Fn(&str) + 'static,
) -> adw::ActionRow {
    let s = settings.clone();
    let k = key.to_string();
    big_media_components::preference_rows::mpv_color_row(
        group,
        title,
        subtitle,
        &settings.string(key),
        move |color| {
            let _ = s.set_string(&k, color);
            on_change(color);
        },
    )
}
