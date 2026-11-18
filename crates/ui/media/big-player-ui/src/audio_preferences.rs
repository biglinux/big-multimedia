//! Shared audio preferences page for the audio and video players.

use std::rc::Rc;

use adw::prelude::*;
use big_media_data::i18n::i18n;
use big_media_data::settings::{Settings, keys};

use crate::preference_rows::{add_combo_row, add_entry_row, add_spin_int_row, add_switch_row};

/// Owned audio-effect values emitted by the generic preferences page.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerAudioEffects {
    /// Whether loudness normalization is enabled.
    pub normalize: bool,
    /// Noise reduction strength; 0 turns it off.
    pub nr_strength: f64,
    /// Selected noise reduction model.
    pub nr_model: i32,
    /// Mix to mono before the denoiser.
    pub nr_mono: bool,
    /// Keep only speech, at this voice sensitivity (1 to 5).
    pub nr_voice_only: Option<u8>,
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
        keys::AUDIO_PITCH_CORRECTION,
        settings,
        move |value| {
            if let Some(actions) = &pitch_actions {
                actions.set_audio_pitch_correction(value);
            }
        },
    );

    let volume_actions = actions.clone();
    let volume_max_range = Settings::VOLUME_MAX_PERCENT_RANGE;
    add_spin_int_row(
        &sound_group,
        &i18n("Maximum Volume"),
        &i18n("Above 100% helps with quiet recordings but may distort"),
        keys::VOLUME_MAX,
        f64::from(*volume_max_range.start()),
        f64::from(*volume_max_range.end()),
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
        keys::PREFERRED_AUDIO_LANG,
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
        keys::AUDIO_OUTPUT,
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
        keys::AUDIO_CHANNELS,
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
    let gains = big_media_components::audio::equalizer_controls::parse_clamped_band_gains::<10>(
        &settings.get_string(keys::EQ_BANDS),
        -12.0,
        12.0,
    );
    let eq_bands = big_audio_effects::equalizer::EQ_BAND_FREQUENCIES_F64
        .into_iter()
        .zip(gains)
        .collect();

    PlayerAudioEffects {
        normalize,
        nr_strength: settings.get_double(keys::NOISE_STRENGTH).max(0.0),
        nr_model: settings.get_int(keys::NOISE_MODEL),
        nr_mono: settings.get_bool(keys::NOISE_MONO),
        nr_voice_only: settings.noise_voice_only(),
        eq_bands,
    }
}
