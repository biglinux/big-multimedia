// SPDX-License-Identifier: MIT

//! Noise-reduction models and the FFmpeg/mpv filter chain that runs them.
//!
//! Two speech denoisers are offered, the same pair the BigLinux converters
//! and the microphone configurator use: DeepFilterNet3 (light) and DPDFNet-2
//! 48 kHz (higher quality). Both are mono 48 kHz LADSPA plugins with an
//! attenuation cap on control `c0`; FFmpeg runs one instance per channel.
//!
//! [`NoiseModel`] values are stored in user settings as the dropdown index.

use std::path::Path;

/// Filter the live strength command targets inside the lavfi graph.
pub const LADSPA_FILTER_NAME: &str = "ladspa";

/// Samples of the opening the model warms up on before the audio it keeps.
const LEAD_IN_SAMPLES: u32 = 24_000;

/// Either plugin's output delay, in samples at 48 kHz, while its voice gate runs.
pub const VOICE_GATE_DELAY_SAMPLES: u32 = 3168;

/// Models shown in the dropdown, in stored-index order.
pub const MODELS: [NoiseModel; 2] = [NoiseModel::DeepFilterNet3, NoiseModel::DpdfNet2];

/// A speech denoiser, stored in settings as its [`MODELS`] index.
///
/// # Examples
///
/// ```
/// use big_audio_effects::noise_reduction::NoiseModel;
///
/// assert_eq!(NoiseModel::from_i32_or_default(1), NoiseModel::DpdfNet2);
/// assert_eq!(NoiseModel::from_i32_or_default(42), NoiseModel::DeepFilterNet3);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NoiseModel {
    /// DeepFilterNet3 W8A16: light, 40 ms delay.
    #[default]
    DeepFilterNet3 = 0,
    /// Native DPDFNet-2 48 kHz HR: higher quality, 60 ms delay.
    DpdfNet2 = 1,
}

impl NoiseModel {
    /// Resolve a stored setting to a model.
    ///
    /// Older players stored GTCRN (0 and 1), the legacy DeepFilterNet (2)
    /// and the OpenVINO DPDFNet builds (3 to 8). The "natural voice" GTCRN
    /// and the high-resolution DPDFNet builds (1, 3, 4) become DPDFNet-2;
    /// everything else, including corrupt values, becomes DeepFilterNet3.
    #[must_use]
    pub fn from_i32_or_default(value: i32) -> Self {
        match value {
            1 | 3 | 4 => Self::DpdfNet2,
            _ => Self::DeepFilterNet3,
        }
    }

    /// Index of the model in [`MODELS`], which is also its stored value.
    #[must_use]
    pub fn index(self) -> u32 {
        self as u32
    }

    /// The plugin's shared object and the label it advertises.
    #[must_use]
    pub fn plugin_and_label(self) -> (&'static str, &'static str) {
        match self {
            Self::DeepFilterNet3 => (
                "/usr/lib/ladspa/libdfn3_ladspa.so",
                "deep_filter_net3_rs_mono",
            ),
            Self::DpdfNet2 => (
                "/usr/lib/ladspa/libdpdfnet_native.so",
                "dpdfnet_native_48hr",
            ),
        }
    }

    /// Package that installs the plugin.
    #[must_use]
    pub fn package(self) -> &'static str {
        match self {
            Self::DeepFilterNet3 => "deepfilternet3-native",
            Self::DpdfNet2 => "dpdfnet-native",
        }
    }

    /// Output delay of the plugin in samples at 48 kHz, measured.
    #[must_use]
    pub fn delay_samples(self) -> u32 {
        match self {
            Self::DeepFilterNet3 => 1919,
            Self::DpdfNet2 => 2880,
        }
    }

    /// Attenuation cap in dB at full strength: where the model stops removing
    /// more. Measured on speech in pink noise: DeepFilterNet3's residual moves
    /// under 1 dB past 24 dB and it keeps music whatever the cap, while
    /// DPDFNet-2 keeps going and at 48 dB has noise at -74 dBFS and music
    /// 35 dB down. A higher cap would leave half the slider doing nothing.
    #[must_use]
    pub fn max_attenuation_db(self) -> f64 {
        match self {
            Self::DeepFilterNet3 => 24.0,
            Self::DpdfNet2 => 48.0,
        }
    }

    /// Attenuation cap for a unit-scale strength, linear in dB up to
    /// [`Self::max_attenuation_db`], so every slider step is audible.
    #[must_use]
    pub fn attenuation_db(self, strength: f64) -> f64 {
        strength.clamp(0.0, 1.0) * self.max_attenuation_db()
    }

    /// The `c0` value for `strength`, also sent live to [`LADSPA_FILTER_NAME`].
    #[must_use]
    pub fn attenuation_control(self, strength: f64) -> String {
        format!("{:.2}", self.attenuation_db(strength))
    }

    /// The voice gate's depth, detector and sensitivity controls (`cN` indexes).
    #[must_use]
    pub fn voice_gate_controls(self) -> (u8, u8, u8) {
        match self {
            Self::DeepFilterNet3 => (17, 18, 19),
            Self::DpdfNet2 => (1, 2, 3),
        }
    }

    /// Whether the plugin file is installed. Performs a `stat`.
    #[must_use]
    pub fn plugin_available(self) -> bool {
        Path::new(self.plugin_and_label().0).exists()
    }
}

/// Whether any model's plugin is installed.
#[must_use]
pub fn any_model_available() -> bool {
    MODELS.iter().any(|model| model.plugin_available())
}

/// The lavfi chain that denoises with `model` at `strength`; with `voice_only`,
/// a sensitivity from 1 to 5, only what FireRedVAD hears as a voice is kept.
///
/// - The plugin delays its output: the end is padded and the start trimmed
///   by that much, keeping sync and the last words.
/// - A plugin starts from an empty model state and under-restores the first
///   ~150 ms, so it first runs over the opening half second played
///   backwards, which is cut away again. That lead-in ends at timestamp 0,
///   so concat keeps the stream's own timeline; mpv rebuilds the graph on
///   every seek.
/// - DeepFilterNet3 mutes its first second and gates what it takes for no
///   voice by default, both meant for a live microphone; `c6=0` and `c17=0`
///   turn them off unless `voice_only` asks for the gate. The gate holds
///   either plugin's output to [`VOICE_GATE_DELAY_SAMPLES`].
/// - With `mono`, all channels are mixed into one first: one plugin instance
///   instead of one per channel, and every speaker gets the same cleaned
///   voice. `rematrix_maxval=1` keeps the mix from summing louder than its
///   input, which a 5.1 track would otherwise do threefold.
///
/// The pad labels start with `bvcnr_`; a graph holds one such chain.
///
/// # Examples
///
/// ```
/// use big_audio_effects::noise_reduction::{NoiseModel, lavfi_chain};
///
/// let chain = lavfi_chain(NoiseModel::DeepFilterNet3, 0.5, false, None);
/// assert!(chain.contains("controls=c0=12.00|c6=0|c17=0"));
/// assert!(chain.ends_with("atrim=start_sample=25919,asetpts=PTS-1919/SR/TB"));
/// let gated = lavfi_chain(NoiseModel::DpdfNet2, 1.0, false, Some(4));
/// assert!(gated.contains("controls=c0=48.00|c1=60|c2=1|c3=4"));
/// assert!(gated.ends_with("asetpts=PTS-3168/SR/TB"));
/// ```
#[must_use]
pub fn lavfi_chain(model: NoiseModel, strength: f64, mono: bool, voice_only: Option<u8>) -> String {
    let (plugin, label) = model.plugin_and_label();
    let mut delay = model.delay_samples();
    let mut extra = String::from(match model {
        NoiseModel::DeepFilterNet3 => "|c6=0",
        NoiseModel::DpdfNet2 => "",
    });
    let (depth, detector, sensitivity) = model.voice_gate_controls();
    if let Some(level) = voice_only {
        // Muted outright, heard by FireRedVAD as the converters do.
        let level = level.clamp(1, 5);
        extra.push_str(&format!(
            "|c{depth}=60|c{detector}=1|c{sensitivity}={level}"
        ));
        delay = VOICE_GATE_DELAY_SAMPLES;
    } else if model == NoiseModel::DeepFilterNet3 {
        extra.push_str(&format!("|c{depth}=0"));
    }
    format!(
        "aresample=48000{},asplit[bvcnr_main][bvcnr_head];\
         [bvcnr_head]atrim=end_sample={LEAD_IN_SAMPLES},apad=whole_len={LEAD_IN_SAMPLES},areverse,\
         asetpts=PTS-STARTPTS-{LEAD_IN_SAMPLES}/SR/TB[bvcnr_pre];\
         [bvcnr_pre][bvcnr_main]concat=n=2:v=0:a=1,apad=pad_len={delay},\
         {LADSPA_FILTER_NAME}=file={plugin}:plugin={label}:controls=c0={}{extra},\
         atrim=start_sample={},asetpts=PTS-{delay}/SR/TB",
        if mono {
            ":ochl=mono:rematrix_maxval=1"
        } else {
            ""
        },
        model.attenuation_control(strength),
        LEAD_IN_SAMPLES + delay,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_values_keep_their_dropdown_index() {
        for (index, model) in MODELS.iter().enumerate() {
            assert_eq!(model.index() as usize, index);
            assert_eq!(NoiseModel::from_i32_or_default(index as i32), *model);
        }
    }

    #[test]
    fn legacy_settings_map_to_the_closest_current_model() {
        let dpdfnet = [1, 3, 4];
        for value in -1..10 {
            let expected = if dpdfnet.contains(&value) {
                NoiseModel::DpdfNet2
            } else {
                NoiseModel::DeepFilterNet3
            };
            assert_eq!(NoiseModel::from_i32_or_default(value), expected, "{value}");
        }
    }

    #[test]
    fn attenuation_is_linear_up_to_each_models_useful_cap() {
        let dfn3 = NoiseModel::DeepFilterNet3;
        let dpdfnet = NoiseModel::DpdfNet2;
        assert!((dfn3.attenuation_db(0.5) - 12.0).abs() < f64::EPSILON);
        assert!((dpdfnet.attenuation_db(0.5) - 24.0).abs() < f64::EPSILON);
        assert!((dpdfnet.attenuation_db(2.0) - 48.0).abs() < f64::EPSILON);
        assert!(dfn3.attenuation_db(-1.0).abs() < f64::EPSILON);
        assert_eq!(dfn3.attenuation_control(1.0), "24.00");
    }

    #[test]
    fn chain_compensates_each_plugins_delay() {
        let dpdfnet = lavfi_chain(NoiseModel::DpdfNet2, 1.0, false, None);
        assert!(dpdfnet.contains("plugin=dpdfnet_native_48hr:controls=c0=48.00,"));
        assert!(dpdfnet.contains("apad=pad_len=2880,"));
        assert!(dpdfnet.ends_with("atrim=start_sample=26880,asetpts=PTS-2880/SR/TB"));
        assert!(!dpdfnet.contains("c6"));
        assert!(dpdfnet.starts_with("aresample=48000,asplit"));
    }

    #[test]
    fn voice_only_gates_either_plugin() {
        let dfn3 = lavfi_chain(NoiseModel::DeepFilterNet3, 1.0, false, Some(2));
        assert!(dfn3.contains("controls=c0=24.00|c6=0|c17=60|c18=1|c19=2,"));
        assert!(dfn3.contains("apad=pad_len=3168,"));
        assert!(dfn3.ends_with("atrim=start_sample=27168,asetpts=PTS-3168/SR/TB"));
        let dpdfnet = lavfi_chain(NoiseModel::DpdfNet2, 1.0, false, Some(9));
        assert!(dpdfnet.contains("controls=c0=48.00|c1=60|c2=1|c3=5,"));
    }

    #[test]
    fn mono_mixes_before_the_denoiser_without_gain() {
        let chain = lavfi_chain(NoiseModel::DeepFilterNet3, 1.0, true, None);
        assert!(chain.starts_with("aresample=48000:ochl=mono:rematrix_maxval=1,asplit"));
    }
}
