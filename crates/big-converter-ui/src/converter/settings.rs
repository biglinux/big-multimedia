//! Converter settings: flat-key Settings store + save/restore for sidebar
//! audio-effects + audio-converter widgets.

use crate::settings::Settings;
use crate::shared::audio_effects::noise_model_index;
use adw::prelude::*;
use big_audio_effects::equalizer::{EQ_PRESET_IDS, eq_preset_index};

/// Open the [`Settings`] store for the given converter binding, picking
/// between the audio and video config namespace.
pub fn converter_settings(mode: crate::converter::app::ConverterMode) -> Settings {
    let namespace = if mode.is_audio() {
        crate::config::audio_converter::CONFIG_DIR
    } else {
        crate::config::video_converter::CONFIG_DIR
    };
    Settings::new(namespace)
}

/// Typed [`Settings`] key constants for every flat-key the converter
/// reads or writes.
///
/// Each constant carries a `BoolKey` / `IntKey` / `DoubleKey` /
/// `StringKey` wrapper around the on-disk key name. Use them through
/// [`Settings::get_bool`](crate::settings::Settings::get_bool) /
/// `put_*` methods so the data type is enforced at compile time.
pub mod keys {
    #![allow(dead_code)]

    use crate::settings::keys::{BoolKey, DoubleKey, IntKey, StringKey};

    /// Last persisted window width in pixels.
    pub const WINDOW_WIDTH: IntKey = IntKey("window-width");
    /// Last persisted window height in pixels.
    pub const WINDOW_HEIGHT: IntKey = IntKey("window-height");
    /// Whether the window was last closed maximized.
    pub const WINDOW_MAXIMIZED: BoolKey = BoolKey("window-maximized");

    /// Id of the last profile selected in the sidebar profile combo.
    pub const LAST_PROFILE: StringKey = StringKey("last-profile");
    /// Last chosen output directory (absolute path).
    pub const OUTPUT_DIR: StringKey = StringKey("output-dir");

    /// Selected GPU device id for hardware-accelerated encoding.
    pub const GPU_DEVICE: StringKey = StringKey("gpu-device");
    /// Set to `true` once the one-shot GPU auto-migration ran.
    pub const GPU_AUTO_MIGRATED: BoolKey = BoolKey("gpu-auto-migrated");

    /// Overwrite existing files at the output path instead of skipping.
    pub const OVERWRITE_EXISTING: BoolKey = BoolKey("overwrite-existing");
    /// Maximum parallel FFmpeg jobs.
    pub const PARALLEL_JOBS: IntKey = IntKey("parallel-jobs");
    /// Whether to emit a desktop notification on queue completion.
    pub const NOTIFY_DONE: BoolKey = BoolKey("notify-done");
    /// Delete source files after a successful conversion.
    pub const DELETE_SOURCE: BoolKey = BoolKey("delete-source");
    /// Hold a power-management inhibitor while jobs are running.
    pub const PREVENT_SLEEP: BoolKey = BoolKey("prevent-sleep");
    /// Shut the system down after the queue drains.
    pub const SHUTDOWN_AFTER: BoolKey = BoolKey("shutdown-after");

    /// Deprecated: multiline TOML breaks the flat settings format.
    pub const SAVED_PROFILE: StringKey = StringKey("saved-profile");

    /// Last directory used by the converter file picker.
    pub const LAST_DIR: StringKey = StringKey("conv-last-dir");
    /// Segment mode for cut output: `"join"` or `"split"`.
    pub const SEGMENT_MODE: StringKey = StringKey("conv-segment-mode");

    /// Noise reduction master enable.
    pub const NR_ENABLED: BoolKey = BoolKey("conv-nr-enabled");
    /// Noise reduction lookahead in milliseconds.
    pub const NR_LOOKAHEAD: IntKey = IntKey("conv-nr-lookahead");
    /// Noise reduction strength (0.0–1.0).
    pub const NR_STRENGTH: DoubleKey = DoubleKey("conv-nr-strength");
    /// Selected noise-model index (mapped via `noise_model_index`).
    pub const NR_MODEL: IntKey = IntKey("conv-nr-model");
    /// Voice-recovery amount for the noise reducer (0.0–1.0).
    pub const NR_VOICE_RECOVERY: DoubleKey = DoubleKey("conv-nr-voice-recovery");
    /// Noise gate master enable.
    pub const GATE_ENABLED: BoolKey = BoolKey("conv-gate-enabled");
    /// Noise gate intensity (0.0–1.0).
    pub const GATE_INTENSITY: DoubleKey = DoubleKey("conv-gate-intensity");
    /// High-pass filter master enable.
    pub const HPF_ENABLED: BoolKey = BoolKey("conv-hpf-enabled");
    /// High-pass filter cutoff frequency in Hz.
    pub const HPF_FREQ: IntKey = IntKey("conv-hpf-freq");
    /// Loudness normalization enable.
    pub const NORMALIZE: BoolKey = BoolKey("conv-normalize");
    /// Dynamic-range compressor master enable.
    pub const COMPRESSOR_ENABLED: BoolKey = BoolKey("conv-compressor-enabled");
    /// Dynamic-range compressor intensity (0.0–1.0).
    pub const COMPRESSOR_INTENSITY: DoubleKey = DoubleKey("conv-compressor-intensity");
    /// Equalizer master enable.
    pub const EQ_ENABLED: BoolKey = BoolKey("conv-eq-enabled");
    /// Currently selected equalizer preset id.
    pub const EQ_PRESET: StringKey = StringKey("conv-eq-preset");
    /// Comma-separated equalizer band gains, one per band.
    pub const EQ_BANDS: StringKey = StringKey("conv-eq-bands");

    /// Audio output format id (e.g. `"mp3"`, `"opus"`).
    pub const AUDIO_CONV_FORMAT: StringKey = StringKey("audio-conv-format");
    /// Audio output bitrate id (e.g. `"128k"`).
    pub const AUDIO_CONV_BITRATE: StringKey = StringKey("audio-conv-bitrate");
    /// Channel layout selector: 0 = original, 1 = mono, 2 = stereo.
    pub const AUDIO_CONV_CHANNELS: IntKey = IntKey("audio-conv-channels");
    /// Cut/segment numbering mode: 0 = off, 1 = chronological,
    /// 2 = segment number.
    pub const AUDIO_CONV_CUT_MODE: IntKey = IntKey("audio-conv-cut-mode");
    /// Cut output layout: 0 = separate files, 1 = merge into one.
    pub const AUDIO_CONV_CUT_OUTPUT: IntKey = IntKey("audio-conv-cut-output");
    /// Audio volume gain factor.
    pub const AUDIO_CONV_VOLUME: DoubleKey = DoubleKey("audio-conv-volume");
    /// Audio playback-speed multiplier (atempo filter).
    pub const AUDIO_CONV_SPEED: DoubleKey = DoubleKey("audio-conv-speed");
}

use crate::converter::ui::sidebar::Sidebar;

/// Persist every audio-effect and audio-converter widget value from
/// `sidebar` into `settings`.
///
/// Writes all noise-reduction, gate, HPF, normalization, compressor,
/// equalizer, format, bitrate, channel, cut, volume, and speed
/// values, then calls [`Settings::flush`](crate::settings::Settings::flush)
/// to commit. Master enable switches are stored alongside continuous
/// values, but [`restore_audio_settings`] deliberately ignores them
/// on the read path.
pub fn save_audio_settings(settings: &Settings, sidebar: &Sidebar) {
    settings.put_bool(keys::NR_ENABLED, sidebar.nr_enabled_switch.is_active());
    settings.put_double(keys::NR_STRENGTH, sidebar.nr_speech_adj.value());
    settings.put_int(keys::NR_MODEL, sidebar.nr_model_combo.selected() as i32);
    settings.put_double(keys::NR_VOICE_RECOVERY, sidebar.nr_recovery_adj.value());
    settings.put_bool(keys::GATE_ENABLED, sidebar.gate_enabled_switch.is_active());
    settings.put_double(keys::GATE_INTENSITY, sidebar.gate_intensity_adj.value());
    settings.put_bool(keys::HPF_ENABLED, sidebar.hpf_enabled_switch.is_active());
    settings.put_int(keys::HPF_FREQ, sidebar.hpf_freq_adj.value() as i32);
    settings.put_bool(keys::NORMALIZE, sidebar.normalize_switch.is_active());
    settings.put_bool(
        keys::COMPRESSOR_ENABLED,
        sidebar.compressor_enabled_switch.is_active(),
    );
    settings.put_double(
        keys::COMPRESSOR_INTENSITY,
        sidebar.compressor_intensity_adj.value(),
    );
    settings.put_bool(keys::EQ_ENABLED, sidebar.eq_enabled_switch.is_active());

    let preset = EQ_PRESET_IDS
        .get(sidebar.eq_preset_combo.selected() as usize)
        .unwrap_or(&"flat");
    settings.put_string(keys::EQ_PRESET, preset);

    let bands: String = sidebar
        .eq_band_adjs
        .iter()
        .map(|a| format!("{:.1}", a.value()))
        .collect::<Vec<_>>()
        .join(",");
    settings.put_string(keys::EQ_BANDS, &bands);

    let fmt = crate::converter::ui::sidebar::AUDIO_FORMAT_IDS
        .get(sidebar.audio_fmt_combo.selected() as usize)
        .copied()
        .unwrap_or("mp3");
    settings.put_string(keys::AUDIO_CONV_FORMAT, fmt);

    let br = crate::converter::ui::sidebar::AUDIO_BITRATE_IDS_A
        .get(sidebar.audio_br_combo.selected() as usize)
        .copied()
        .unwrap_or("128k");
    settings.put_string(keys::AUDIO_CONV_BITRATE, br);

    settings.put_int(
        keys::AUDIO_CONV_CHANNELS,
        sidebar.audio_ch_combo.selected() as i32,
    );
    settings.put_int(
        keys::AUDIO_CONV_CUT_MODE,
        sidebar.audio_cut_combo.selected() as i32,
    );
    settings.put_int(
        keys::AUDIO_CONV_CUT_OUTPUT,
        sidebar.audio_cut_out_combo.selected() as i32,
    );
    settings.put_double(keys::AUDIO_CONV_VOLUME, sidebar.audio_vol_adj.value());
    settings.put_double(keys::AUDIO_CONV_SPEED, sidebar.audio_speed_adj.value());

    settings.flush();
}

/// Restore non-toggle audio-filter values (NR strength/model, gate
/// intensity, HPF freq, compressor intensity, EQ preset/bands, volume,
/// speed) from GSettings into the sidebar adjustments and combos.
///
/// Enable switches are deliberately NOT restored — every audio filter
/// starts disabled on launch so the user opts in explicitly.
pub fn restore_audio_settings(settings: &Settings, sidebar: &Sidebar) {
    let strength = settings.get_double(keys::NR_STRENGTH);
    if strength > 0.0 {
        sidebar.nr_speech_adj.set_value(strength);
    }
    let model = settings.get_int(keys::NR_MODEL);
    sidebar
        .nr_model_combo
        .set_selected(noise_model_index(model));
    let recovery = settings.get_double(keys::NR_VOICE_RECOVERY);
    if recovery > 0.0 {
        sidebar.nr_recovery_adj.set_value(recovery);
    }

    let gate_int = settings.get_double(keys::GATE_INTENSITY);
    if gate_int > 0.0 {
        sidebar.gate_intensity_adj.set_value(gate_int);
    }

    let freq = settings.get_int(keys::HPF_FREQ);
    if freq > 0 {
        sidebar.hpf_freq_adj.set_value(f64::from(freq));
    }

    let comp_int = settings.get_double(keys::COMPRESSOR_INTENSITY);
    if comp_int > 0.0 {
        sidebar.compressor_intensity_adj.set_value(comp_int);
    }

    let preset_str = settings.get_string(keys::EQ_PRESET);
    let eq_idx = eq_preset_index(&preset_str);
    sidebar.eq_preset_combo.set_selected(eq_idx as u32);

    let bands_str = settings.get_string(keys::EQ_BANDS);
    if !bands_str.is_empty() {
        for (adj, val_str) in sidebar.eq_band_adjs.iter().zip(bands_str.split(',')) {
            if let Ok(val) = val_str.trim().parse::<f64>() {
                adj.set_value(val);
            }
        }
    }

    let fmt = settings.get_string(keys::AUDIO_CONV_FORMAT);
    if !fmt.is_empty() {
        crate::converter::ui::sidebar::set_combo_by_id(
            &sidebar.audio_fmt_combo,
            crate::converter::ui::sidebar::AUDIO_FORMAT_IDS,
            &fmt,
        );
    }

    let br = settings.get_string(keys::AUDIO_CONV_BITRATE);
    if !br.is_empty() {
        crate::converter::ui::sidebar::set_combo_by_id(
            &sidebar.audio_br_combo,
            crate::converter::ui::sidebar::AUDIO_BITRATE_IDS_A,
            &br,
        );
    }

    let channels = settings.get_int(keys::AUDIO_CONV_CHANNELS);
    if channels >= 0 {
        sidebar.audio_ch_combo.set_selected(channels as u32);
    }

    let cut_mode = settings.get_int(keys::AUDIO_CONV_CUT_MODE);
    if cut_mode >= 0 {
        sidebar.audio_cut_combo.set_selected(cut_mode as u32);
    }

    let cut_output = settings.get_int(keys::AUDIO_CONV_CUT_OUTPUT);
    if cut_output >= 0 {
        sidebar.audio_cut_out_combo.set_selected(cut_output as u32);
    }
    // Visibility derives from cut_mode even when AUDIO_CONV_CUT_OUTPUT is absent.
    sidebar
        .audio_cut_out_combo
        .set_visible(sidebar.audio_cut_combo.selected() != 0);

    let volume = settings.get_double(keys::AUDIO_CONV_VOLUME);
    if volume > 0.0 {
        sidebar.audio_vol_adj.set_value(volume);
    }

    let speed = settings.get_double(keys::AUDIO_CONV_SPEED);
    if speed > 0.0 {
        sidebar.audio_speed_adj.set_value(speed);
    }
}
