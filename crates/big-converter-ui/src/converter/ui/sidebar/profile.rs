//! Sidebar profile build/apply: collects every combo + audio-effects switch
//! into a Profile. Audio mode picks codec+container from the format combo.
//! Force handling=reencode whenever any audio effect is on (copy can't filter).

use adw::prelude::*;

use crate::converter::profiles::{
    AudioConfig, AudioEffectsConfig, GpuConfig, NoiseReductionConfig, OutputConfig, Profile,
    ProfileMeta, SubtitleConfig, VideoConfig, VideoFilters,
};
use crate::shared::audio_effects::noise_model_index;
use big_audio_effects::equalizer::{EQ_PRESET_IDS, eq_preset_index};

use super::{
    AUDIO_BITRATE_IDS, AUDIO_BITRATE_IDS_A, AUDIO_CHANNELS_IDS_A, AUDIO_CODEC_IDS,
    AUDIO_FORMAT_IDS, AUDIO_HANDLING_IDS, CODEC_IDS, CONTAINER_IDS, PRESET_IDS, QUALITY_IDS,
    RESOLUTION_IDS, SUBTITLE_IDS,
};
use super::{Sidebar, combo_id, set_combo_by_id};

impl Sidebar {
    /// Root [`gtk::Box`] of the sidebar; embed in the window split.
    #[must_use]
    pub fn widget(&self) -> &gtk::Box {
        &self.container
    }

    /// Switch the sidebar to the per-clip editing page.
    pub fn show_editing_page(&self) {
        self.view_stack.set_visible_child_name("editing");
    }

    /// Switch the sidebar to the global conversion-settings page.
    pub fn show_conversion_page(&self) {
        self.view_stack.set_visible_child_name("conversion");
    }

    /// Refresh the GPU selectors with detected device labels and
    /// hide the group when there are two or fewer entries
    /// (CPU + a single GPU is not worth a chooser).
    pub fn set_gpus(&self, labels: &[String]) {
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let model = gtk::StringList::new(&labels);
        self.gpu_combo.set_model(Some(&model));
        self.gpu_device_combo.set_model(Some(&model));
        self.gpu_group.set_visible(labels.len() > 2);
    }

    /// Snapshot every sidebar widget into a [`Profile`] suitable for
    /// persistence or for handing to the conversion queue.
    #[must_use]
    pub fn build_profile(&self) -> Profile {
        if self.mode.is_audio() {
            return self.build_audio_profile();
        }

        let mut profile = build_profile_from_combos(
            &self.video_codec_combo,
            &self.video_quality_combo,
            &self.preset_combo,
            &self.resolution_combo,
            &self.output_format_combo,
            &self.audio_handling_combo,
            &self.audio_codec_combo,
            &self.audio_bitrate_combo,
            &self.subtitle_combo,
        );

        profile.audio.noise_reduction = NoiseReductionConfig {
            enabled: self.nr_enabled_switch.is_active(),
            strength: self.nr_strength_adj.value(),
            model: self.nr_model_combo.selected() as i32,
            speech_strength: self.nr_speech_adj.value(),
            voice_recovery: self.nr_recovery_adj.value(),
        };
        profile.audio.effects = AudioEffectsConfig {
            gate_enabled: self.gate_enabled_switch.is_active(),
            gate_intensity: self.gate_intensity_adj.value(),
            hpf_enabled: self.hpf_enabled_switch.is_active(),
            hpf_frequency: self.hpf_freq_adj.value() as u32,
            normalize: self.normalize_switch.is_active(),
            compressor_enabled: self.compressor_enabled_switch.is_active(),
            compressor_intensity: self.compressor_intensity_adj.value(),
            eq_enabled: self.eq_enabled_switch.is_active(),
            eq_preset: EQ_PRESET_IDS
                .get(self.eq_preset_combo.selected() as usize)
                .unwrap_or(&"flat")
                .to_string(),
            eq_bands: self
                .eq_band_adjs
                .iter()
                .map(gtk::prelude::AdjustmentExt::value)
                .collect(),
            volume: self.audio_vol_adj.value(),
            speed: self.audio_speed_adj.value(),
        };

        let has_effects = profile.audio.noise_reduction.enabled
            || profile.audio.effects.gate_enabled
            || profile.audio.effects.hpf_enabled
            || profile.audio.effects.normalize
            || profile.audio.effects.compressor_enabled
            || profile.audio.effects.eq_enabled;
        if has_effects && profile.audio.handling == "copy" {
            profile.audio.handling = "reencode".into();
        }

        profile
    }

    fn build_audio_profile(&self) -> Profile {
        let fmt = AUDIO_FORMAT_IDS
            .get(self.audio_fmt_combo.selected() as usize)
            .copied()
            .unwrap_or("mp3");
        let (handling, codec, container): (&str, &str, &str) = match fmt {
            "copy" => ("copy", "copy", ""),
            "ogg" => ("reencode", "vorbis", "ogg"),
            "flac" => ("reencode", "flac", "flac"),
            "wav" => ("reencode", "pcm", "wav"),
            "aac" => ("reencode", "aac", "aac"),
            "opus" => ("reencode", "opus", "opus"),
            _ => ("reencode", "mp3", "mp3"),
        };
        let bitrate = AUDIO_BITRATE_IDS_A
            .get(self.audio_br_combo.selected() as usize)
            .copied()
            .unwrap_or("128k")
            .to_string();
        let channels = AUDIO_CHANNELS_IDS_A
            .get(self.audio_ch_combo.selected() as usize)
            .copied()
            .unwrap_or("auto")
            .to_string();

        let mut profile = Profile {
            profile: ProfileMeta::default(),
            video: VideoConfig {
                codec: "copy".into(),
                ..VideoConfig::default()
            },
            audio: AudioConfig {
                handling: handling.into(),
                codec: codec.into(),
                bitrate,
                channels,
                noise_reduction: NoiseReductionConfig {
                    enabled: self.nr_enabled_switch.is_active(),
                    strength: self.nr_strength_adj.value(),
                    model: self.nr_model_combo.selected() as i32,
                    speech_strength: self.nr_speech_adj.value(),
                    voice_recovery: self.nr_recovery_adj.value(),
                },
                effects: AudioEffectsConfig {
                    gate_enabled: self.gate_enabled_switch.is_active(),
                    gate_intensity: self.gate_intensity_adj.value(),
                    hpf_enabled: self.hpf_enabled_switch.is_active(),
                    hpf_frequency: self.hpf_freq_adj.value() as u32,
                    normalize: self.normalize_switch.is_active(),
                    compressor_enabled: self.compressor_enabled_switch.is_active(),
                    compressor_intensity: self.compressor_intensity_adj.value(),
                    eq_enabled: self.eq_enabled_switch.is_active(),
                    eq_preset: "flat".into(),
                    eq_bands: self
                        .eq_band_adjs
                        .iter()
                        .map(gtk::prelude::AdjustmentExt::value)
                        .collect(),
                    volume: self.audio_vol_adj.value(),
                    speed: self.audio_speed_adj.value(),
                },
            },
            subtitles: SubtitleConfig::default(),
            output: OutputConfig {
                container: container.into(),
                ..OutputConfig::default()
            },
        };

        let has_effects = profile.audio.noise_reduction.enabled
            || profile.audio.effects.gate_enabled
            || profile.audio.effects.hpf_enabled
            || profile.audio.effects.normalize
            || profile.audio.effects.compressor_enabled
            || profile.audio.effects.eq_enabled;
        if has_effects && profile.audio.handling == "copy" {
            profile.audio.handling = "reencode".into();
            profile.audio.codec = "mp3".into();
            profile.output.container = "mp3".into();
        }

        profile
    }

    /// Apply the values from `profile` to every sidebar widget,
    /// overwriting current selections.
    pub fn apply_profile(&self, profile: &Profile) {
        set_combo_by_id(&self.video_codec_combo, CODEC_IDS, &profile.video.codec);
        set_combo_by_id(
            &self.video_quality_combo,
            QUALITY_IDS,
            &profile.video.quality,
        );
        set_combo_by_id(&self.preset_combo, PRESET_IDS, &profile.video.preset);
        set_combo_by_id(
            &self.resolution_combo,
            RESOLUTION_IDS,
            &profile.video.filters.resolution,
        );
        set_combo_by_id(
            &self.output_format_combo,
            CONTAINER_IDS,
            &profile.output.container,
        );
        set_combo_by_id(
            &self.audio_handling_combo,
            AUDIO_HANDLING_IDS,
            &profile.audio.handling,
        );
        set_combo_by_id(
            &self.audio_codec_combo,
            AUDIO_CODEC_IDS,
            &profile.audio.codec,
        );
        set_combo_by_id(
            &self.audio_bitrate_combo,
            AUDIO_BITRATE_IDS,
            &profile.audio.bitrate,
        );
        set_combo_by_id(
            &self.subtitle_combo,
            SUBTITLE_IDS,
            &profile.subtitles.handling,
        );

        match profile.video.codec.as_str() {
            "copy" => self.radio_copy.set_active(true),
            "h264" => self.radio_universal.set_active(true),
            "h265" => self.radio_smaller.set_active(true),
            "av1" => self.radio_quality.set_active(true),
            _ => self.radio_custom.set_active(true),
        }

        let noise_reduction = &profile.audio.noise_reduction;
        self.nr_enabled_switch.set_active(noise_reduction.enabled);
        self.nr_speech_adj
            .set_value(noise_reduction.speech_strength);
        self.nr_model_combo
            .set_selected(noise_model_index(noise_reduction.model));
        self.nr_recovery_adj
            .set_value(noise_reduction.voice_recovery);

        let effects = &profile.audio.effects;
        self.gate_enabled_switch.set_active(effects.gate_enabled);
        self.gate_intensity_adj.set_value(effects.gate_intensity);
        self.hpf_enabled_switch.set_active(effects.hpf_enabled);
        self.hpf_freq_adj
            .set_value(f64::from(effects.hpf_frequency));
        self.normalize_switch.set_active(effects.normalize);
        self.compressor_enabled_switch
            .set_active(effects.compressor_enabled);
        self.compressor_intensity_adj
            .set_value(effects.compressor_intensity);
        self.eq_enabled_switch.set_active(effects.eq_enabled);
        self.eq_preset_combo
            .set_selected(eq_preset_index(&effects.eq_preset) as u32);
        for (adjustment, &value) in self.eq_band_adjs.iter().zip(effects.eq_bands.iter()) {
            adjustment.set_value(value);
        }

        self.update_audio_encoding_subtitle();
    }

    pub(super) fn update_audio_encoding_subtitle(&self) {
        let idx = self.audio_handling_combo.selected() as usize;
        let is_reencode = idx == 1;
        let handling_labels = big_media_runtime::ui_labels::audio_handling_labels();
        let handling = handling_labels
            .get(idx)
            .cloned()
            .unwrap_or_else(|| crate::i18n::i18n("Copy (no change)"));
        if is_reencode {
            let codec_labels = big_media_runtime::ui_labels::audio_codec_labels();
            let codec = codec_labels
                .get(self.audio_codec_combo.selected() as usize)
                .cloned()
                .unwrap_or_else(|| crate::i18n::i18n("AAC"));
            self.audio_encoding_row
                .set_subtitle(&format!("{handling} · {codec}"));
        } else {
            self.audio_encoding_row.set_subtitle(&handling);
        }
        self.nr_sidebar_group.set_visible(is_reencode);
    }

    /// Set the visible output-directory entry text to `path`.
    pub fn set_output_dir(&self, path: &str) {
        self.folder_entry.set_text(path);
    }

    /// Action row that contains the output-directory controls;
    /// exposed so the window can react to clicks on it.
    #[must_use]
    pub fn output_row(&self) -> &adw::ActionRow {
        &self.output_row
    }

    /// Set the callback fired when the user picks a different
    /// preset profile in the sidebar radio group.
    pub fn connect_profile_selected<F: Fn(super::PresetProfile) + 'static>(&self, f: F) {
        *self.on_profile_selected.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired when the user opens the
    /// video-encoding dialog.
    pub fn connect_show_encoding_dialog<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_show_encoding_dialog.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired when the user opens the
    /// audio-encoding dialog.
    pub fn connect_show_audio_dialog<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_show_audio_dialog.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired when the user opens the
    /// noise-reduction / cleanup dialog.
    pub fn connect_show_noise_dialog<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_show_noise_dialog.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired when the user opens the
    /// subtitles dialog.
    pub fn connect_show_subtitles_dialog<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_show_subtitles_dialog.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired when the user opens the
    /// extra-options dialog.
    pub fn connect_show_extra_dialog<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_show_extra_dialog.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired when the user activates the
    /// "Browse output folder" button or row.
    pub fn connect_output_dir_clicked<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_output_dir_clicked.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired whenever any sidebar setting widget
    /// changes value (used to persist the profile).
    pub fn connect_settings_changed<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_settings_changed.borrow_mut() = Some(Box::new(f));
    }
}

#[allow(clippy::too_many_arguments)]
fn build_profile_from_combos(
    codec: &adw::ComboRow,
    quality: &adw::ComboRow,
    preset: &adw::ComboRow,
    resolution: &adw::ComboRow,
    container: &adw::ComboRow,
    audio_handling: &adw::ComboRow,
    audio_codec: &adw::ComboRow,
    audio_bitrate: &adw::ComboRow,
    subtitle: &adw::ComboRow,
) -> Profile {
    let filters = VideoFilters {
        resolution: combo_id(resolution, RESOLUTION_IDS),
        ..VideoFilters::default()
    };

    Profile {
        profile: ProfileMeta::default(),
        video: VideoConfig {
            codec: combo_id(codec, CODEC_IDS),
            quality: combo_id(quality, QUALITY_IDS),
            preset: combo_id(preset, PRESET_IDS),
            pixel_format: "auto".into(),
            container: "auto".into(),
            gpu: GpuConfig::default(),
            filters,
        },
        audio: AudioConfig {
            handling: combo_id(audio_handling, AUDIO_HANDLING_IDS),
            codec: combo_id(audio_codec, AUDIO_CODEC_IDS),
            bitrate: combo_id(audio_bitrate, AUDIO_BITRATE_IDS),
            ..AudioConfig::default()
        },
        subtitles: SubtitleConfig {
            handling: combo_id(subtitle, SUBTITLE_IDS),
            ..SubtitleConfig::default()
        },
        output: OutputConfig {
            container: combo_id(container, CONTAINER_IDS),
            ..OutputConfig::default()
        },
    }
}
