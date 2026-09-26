//! Shared audio preferences page for the audio and video players.

use std::rc::Rc;

use adw::prelude::*;
use big_media_components::preference_rows::{combo_row, entry_row, spin_int_row, switch_row};
use big_media_data::i18n::i18n;
use big_media_data::settings::{Settings, keys};

/// Owned audio-effect values emitted by the generic preferences page.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerAudioEffects {
    /// Whether loudness normalization is enabled.
    pub normalize: bool,
    /// Whether noise reduction is enabled.
    pub nr_enabled: bool,
    /// Noise reduction strength.
    pub nr_strength: f64,
    /// Selected noise reduction model.
    pub nr_model: i32,
    /// Whether model blend is enabled.
    pub nr_model_blend: bool,
    /// Voice recovery amount.
    pub nr_voice_recovery: f64,
    /// Equalizer band pairs `(frequency_hz, gain_db)`.
    pub eq_bands: Vec<(f64, f64)>,
}

/// Live-player callbacks invoked by the generic audio preferences page.
#[derive(Clone)]
pub struct PlayerAudioPreferenceActions {
    set_audio_channels: Rc<dyn Fn(&str)>,
    set_audio_pitch_correction: Rc<dyn Fn(bool)>,
    set_volume_max: Rc<dyn Fn(i64)>,
    set_preferred_audio_lang: Rc<dyn Fn(&str)>,
    apply_audio_effects: Rc<dyn Fn(PlayerAudioEffects)>,
}

impl PlayerAudioPreferenceActions {
    /// Create callbacks that bridge the generic page to an app-specific player.
    pub fn new(
        set_audio_channels: impl Fn(&str) + 'static,
        set_audio_pitch_correction: impl Fn(bool) + 'static,
        set_volume_max: impl Fn(i64) + 'static,
        set_preferred_audio_lang: impl Fn(&str) + 'static,
        apply_audio_effects: impl Fn(PlayerAudioEffects) + 'static,
    ) -> Self {
        Self {
            set_audio_channels: Rc::new(set_audio_channels),
            set_audio_pitch_correction: Rc::new(set_audio_pitch_correction),
            set_volume_max: Rc::new(set_volume_max),
            set_preferred_audio_lang: Rc::new(set_preferred_audio_lang),
            apply_audio_effects: Rc::new(apply_audio_effects),
        }
    }

    fn set_audio_channels(&self, value: &str) {
        (self.set_audio_channels)(value);
    }

    fn set_audio_pitch_correction(&self, value: bool) {
        (self.set_audio_pitch_correction)(value);
    }

    fn set_volume_max(&self, value: i64) {
        (self.set_volume_max)(value);
    }

    fn set_preferred_audio_lang(&self, value: &str) {
        (self.set_preferred_audio_lang)(value);
    }

    fn apply_audio_effects(&self, effects: PlayerAudioEffects) {
        (self.apply_audio_effects)(effects);
    }
}

/// Build the shared audio preferences page.
pub fn create_player_audio_page(
    settings: &Settings,
    actions: Option<PlayerAudioPreferenceActions>,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Audio"))
        .icon_name("audio-speakers-symbolic")
        .build();

    let sound_group = adw::PreferencesGroup::builder()
        .title(i18n("Sound"))
        .build();

    let normalize_row = adw::SwitchRow::builder()
        .title(i18n("Even Out Loudness"))
        .subtitle(i18n("Quiet and loud media play at a similar volume"))
        .active(settings.get_bool(keys::AUDIO_NORMALIZE))
        .build();
    {
        let settings = settings.clone();
        let actions = actions.clone();
        normalize_row.connect_active_notify(move |row| {
            let _ = settings.put_bool(keys::AUDIO_NORMALIZE, row.is_active());
            if let Some(actions) = &actions {
                actions.apply_audio_effects(read_audio_effects(&settings, row.is_active()));
            }
        });
    }
    sound_group.add(&normalize_row);

    let pitch_actions = actions.clone();
    add_switch_row(
        &sound_group,
        &i18n("Keep Voices Natural"),
        &i18n("Voices keep their tone when playing faster or slower"),
        "audio-pitch-correction",
        settings,
        move |value| {
            if let Some(actions) = &pitch_actions {
                actions.set_audio_pitch_correction(value);
            }
        },
    );

    let volume_actions = actions.clone();
    add_spin_int_row(
        &sound_group,
        &i18n("Maximum Volume"),
        &i18n("Above 100% helps with quiet recordings but may distort"),
        "volume-max",
        100.0,
        200.0,
        5.0,
        settings,
        move |value| {
            if let Some(actions) = &volume_actions {
                actions.set_volume_max(value);
            }
        },
    );
    page.add(&sound_group);

    let language_group = adw::PreferencesGroup::builder()
        .title(i18n("Language"))
        .description(i18n(
            "For media with several audio tracks. Use language codes separated by commas, in order of preference, such as pt,en",
        ))
        .build();
    let language_actions = actions.clone();
    add_entry_row(
        &language_group,
        &i18n("Preferred Languages"),
        "",
        "preferred-audio-lang",
        settings,
        move |value| {
            if let Some(actions) = &language_actions {
                actions.set_preferred_audio_lang(value);
            }
        },
    );
    page.add(&language_group);

    let output_group = adw::PreferencesGroup::builder()
        .title(i18n("Sound System"))
        .description(i18n("Automatic works for most computers"))
        .build();
    add_combo_row(
        &output_group,
        &i18n("Audio Output"),
        &i18n("Takes effect after reopening the player"),
        &[&i18n("Automatic"), "PipeWire", "PulseAudio", "ALSA", "JACK"],
        &["", "pipewire", "pulse", "alsa", "jack"],
        "audio-output",
        settings,
        |_| {},
    );
    let channel_actions = actions.clone();
    add_combo_row(
        &output_group,
        &i18n("Speakers"),
        &i18n("Mixes surround sound down for stereo speakers or headphones"),
        &[&i18n("Automatic"), &i18n("Stereo"), &i18n("Mono")],
        &["", "stereo", "mono"],
        "audio-channels",
        settings,
        move |value| {
            if let Some(actions) = &channel_actions {
                actions.set_audio_channels(value);
            }
        },
    );
    page.add(&output_group);

    page
}

fn read_audio_effects(settings: &Settings, normalize: bool) -> PlayerAudioEffects {
    let nr_strength = settings.get_double(keys::NOISE_STRENGTH);
    let eq_bands = parse_eq_bands(&settings.get_string(keys::EQ_BANDS));

    PlayerAudioEffects {
        normalize,
        nr_enabled: nr_strength.abs() > 0.0,
        nr_strength,
        nr_model: settings.get_int(keys::NOISE_MODEL),
        nr_model_blend: settings.get_bool(keys::NOISE_MODEL_BLEND),
        nr_voice_recovery: settings.get_double(keys::NOISE_VOICE_RECOVERY),
        eq_bands,
    }
}

fn parse_eq_bands(eq_bands: &str) -> Vec<(f64, f64)> {
    if eq_bands.is_empty() {
        return Vec::new();
    }

    eq_bands
        .split(';')
        .filter_map(|entry| entry.parse::<f64>().ok())
        .enumerate()
        .filter_map(|(index, gain)| {
            big_audio_effects::equalizer::EQ_BAND_FREQUENCIES_F64
                .get(index)
                .map(|&frequency| (frequency, gain))
        })
        .collect()
}

fn add_switch_row(
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    key: &str,
    settings: &Settings,
    on_change: impl Fn(bool) + 'static,
) -> adw::SwitchRow {
    let settings = settings.clone();
    let key = key.to_string();
    switch_row(
        group,
        title,
        subtitle,
        settings.boolean(&key),
        move |active| {
            let _ = settings.set_boolean(&key, active);
            on_change(active);
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn add_combo_row(
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
    let selected = values
        .iter()
        .position(|value| *value == current.as_str())
        .unwrap_or(0) as u32;
    let settings = settings.clone();
    let key = key.to_string();
    combo_row(group, title, subtitle, labels, selected, move |selected| {
        if let Some(value) = values.get(selected as usize) {
            let _ = settings.set_string(&key, value);
            on_change(value);
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn add_spin_int_row(
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
    let settings = settings.clone();
    let key = key.to_string();
    spin_int_row(
        group,
        title,
        subtitle,
        settings.int(&key),
        min,
        max,
        step,
        move |value| {
            let _ = settings.set_int(&key, value);
            on_change(i64::from(value));
        },
    )
}

fn add_entry_row(
    group: &adw::PreferencesGroup,
    title: &str,
    tooltip: &str,
    key: &str,
    settings: &Settings,
    on_change: impl Fn(&str) + 'static,
) -> adw::EntryRow {
    let settings = settings.clone();
    let key = key.to_string();
    entry_row(group, title, tooltip, &settings.string(&key), move |text| {
        let _ = settings.set_string(&key, text);
        on_change(text);
    })
}

#[cfg(test)]
mod tests {
    use super::parse_eq_bands;

    #[test]
    fn parse_eq_bands_pairs_known_frequencies() {
        let bands = parse_eq_bands("1.0;0.5;-2.0");
        assert_eq!(bands, vec![(31.0, 1.0), (63.0, 0.5), (125.0, -2.0)]);
    }

    #[test]
    fn parse_eq_bands_skips_invalid_entries() {
        let bands = parse_eq_bands("1.0;invalid;2.0");
        assert_eq!(bands, vec![(31.0, 1.0), (63.0, 2.0)]);
    }
}
