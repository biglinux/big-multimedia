//! Preference rows that show a stored setting and write each change back
//! before calling `on_change`.
#![expect(
    clippy::disallowed_methods,
    reason = "this module owns the settings binding of the component rows"
)]

use big_media_components::preference_rows::{
    combo_row, entry_row, mpv_color_row, spin_double_row, spin_int_row, switch_row,
};
use big_media_data::settings::Settings;
use big_media_data::settings::keys::{BoolKey, DoubleKey, IntKey, StringKey};

/// Append a switch bound to `key`.
pub fn add_switch_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    key: BoolKey,
    settings: &Settings,
    on_change: impl Fn(bool) + 'static,
) -> adw::SwitchRow {
    let settings = settings.clone();
    switch_row(
        group,
        title,
        subtitle,
        settings.get_bool(key),
        move |active| {
            let _ = settings.put_bool(key, active);
            on_change(active);
        },
    )
}

/// Append a combo listing `labels` that stores the matching entry of `values`
/// in `key`; a stored value outside `values` shows the first label.
pub fn add_combo_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    labels: &[&str],
    values: &'static [&'static str],
    key: StringKey,
    settings: &Settings,
    on_change: impl Fn(&str) + 'static,
) -> adw::ComboRow {
    let current = settings.get_string(key);
    let selected = values
        .iter()
        .position(|value| *value == current.as_str())
        .and_then(|index| u32::try_from(index).ok())
        .unwrap_or(0);
    let settings = settings.clone();
    combo_row(group, title, subtitle, labels, selected, move |selected| {
        if let Some(value) = values.get(selected as usize) {
            let _ = settings.put_string(key, value);
            on_change(value);
        }
    })
}

/// Append a combo listing `labels` that stores the selected index in `key`.
pub fn add_index_combo_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    labels: &[&str],
    key: IntKey,
    settings: &Settings,
    on_change: impl Fn(i32) + 'static,
) -> adw::ComboRow {
    let selected = u32::try_from(settings.get_int(key)).unwrap_or(0);
    let settings = settings.clone();
    combo_row(group, title, subtitle, labels, selected, move |selected| {
        let index = i32::try_from(selected).unwrap_or(0);
        let _ = settings.put_int(key, index);
        on_change(index);
    })
}

/// Append an integer spin row over `[min, max]` bound to `key`.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the component row it binds"
)]
pub fn add_spin_int_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    key: IntKey,
    min: f64,
    max: f64,
    step: f64,
    settings: &Settings,
    on_change: impl Fn(i64) + 'static,
) -> adw::SpinRow {
    let settings = settings.clone();
    spin_int_row(
        group,
        title,
        subtitle,
        settings.get_int(key),
        min,
        max,
        step,
        move |value| {
            let _ = settings.put_int(key, value);
            on_change(i64::from(value));
        },
    )
}

/// Append a spin row over `[min, max]` with `digits` decimals bound to `key`.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors the component row it binds"
)]
pub fn add_spin_double_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    key: DoubleKey,
    min: f64,
    max: f64,
    step: f64,
    digits: u32,
    settings: &Settings,
    on_change: impl Fn(f64) + 'static,
) -> adw::SpinRow {
    let settings = settings.clone();
    spin_double_row(
        group,
        title,
        subtitle,
        settings.get_double(key),
        min,
        max,
        step,
        digits,
        move |value| {
            let _ = settings.put_double(key, value);
            on_change(value);
        },
    )
}

/// Append a text entry bound to `key`, saved on every edit.
pub fn add_entry_row(
    group: &adw::PreferencesGroup,
    title: &str,
    tooltip: &str,
    key: StringKey,
    settings: &Settings,
    on_change: impl Fn(&str) + 'static,
) -> adw::EntryRow {
    let settings = settings.clone();
    entry_row(
        group,
        title,
        tooltip,
        &settings.get_string(key),
        move |text| {
            let _ = settings.put_string(key, text);
            on_change(text);
        },
    )
}

/// Append a color picker bound to `key`, stored in mpv's `#AARRGGBB` form.
pub fn add_mpv_color_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    key: StringKey,
    settings: &Settings,
    on_change: impl Fn(&str) + 'static,
) -> adw::ActionRow {
    let settings = settings.clone();
    mpv_color_row(
        group,
        title,
        subtitle,
        &settings.get_string(key),
        move |color| {
            let _ = settings.put_string(key, color);
            on_change(color);
        },
    )
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use adw::prelude::*;
    use big_media_data::settings::keys;

    use super::*;

    const SPEAKERS: &[&str] = &["Auto", "Stereo", "Mono"];
    const CHANNELS: &[&str] = &["", "stereo", "mono"];

    #[test]
    #[ignore = "requires an isolated GTK session"]
    fn rows_store_through_their_key() {
        gtk::init().expect("isolated GTK session");
        let group = adw::PreferencesGroup::new();
        let settings = Settings::memory_only();
        let seen = Rc::new(RefCell::new(Vec::new()));

        let _ = settings.put_string(keys::AUDIO_CHANNELS, "mono");
        let combo = {
            let seen = seen.clone();
            let on_change = move |value: &str| seen.borrow_mut().push(value.to_owned());
            let key = keys::AUDIO_CHANNELS;
            add_combo_row(
                &group, "", "", SPEAKERS, CHANNELS, key, &settings, on_change,
            )
        };
        assert_eq!(combo.selected(), 2);
        combo.set_selected(1);
        assert_eq!(settings.get_string(keys::AUDIO_CHANNELS), "stereo");
        assert_eq!(*seen.borrow(), ["stereo"]);

        let _ = settings.put_string(keys::AUDIO_CHANNELS, "surround");
        let key = keys::AUDIO_CHANNELS;
        let unknown = add_combo_row(&group, "", "", SPEAKERS, CHANNELS, key, &settings, |_| {});
        assert_eq!(
            unknown.selected(),
            0,
            "an unknown value shows the first label"
        );

        let _ = settings.put_int(keys::ICC_INTENT, -4);
        let intent = add_index_combo_row(
            &group,
            "",
            "",
            SPEAKERS,
            keys::ICC_INTENT,
            &settings,
            |_| {},
        );
        assert_eq!(intent.selected(), 0);
        intent.set_selected(2);
        assert_eq!(settings.get_int(keys::ICC_INTENT), 2);

        let switch = add_switch_row(&group, "", "", keys::SUB_BOLD, &settings, |_| {});
        let stored = settings.get_bool(keys::SUB_BOLD);
        assert_eq!(switch.is_active(), stored);
        switch.set_active(!stored);
        assert_eq!(settings.get_bool(keys::SUB_BOLD), !stored);

        let key = keys::PREFERRED_SUB_LANG;
        let entry = add_entry_row(&group, "", "", key, &settings, |_| {});
        entry.set_text("pt,en");
        assert_eq!(settings.get_string(key), "pt,en");
        entry.set_text("");
        assert_eq!(settings.get_string(key), "");
    }
}
