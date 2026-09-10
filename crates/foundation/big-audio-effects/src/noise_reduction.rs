// SPDX-License-Identifier: MIT

//! Noise-reduction model and LADSPA control contracts.
//!
//! This module pins down the public contract between BigLinux media
//! tooling and the LADSPA noise-reduction plugins distributed by the
//! distribution:
//!
//! - Where the plugin `.so` files live on disk and what label each
//!   plugin advertises ([`crate::noise_reduction::GTCRN_LADSPA_PATH`]
//!   et al., [`crate::noise_reduction::LABEL_GTCRN_MONO`] et al.).
//! - Which models are exposed to the realtime LAVFI/PipeWire path
//!   ([`crate::noise_reduction::REALTIME_LAVFI_MODELS`]) and how their
//!   slider position is converted to control-port values
//!   ([`crate::noise_reduction::GtcrnControls`],
//!   [`crate::noise_reduction::deepfilter_attenuation_db`]).
//! - How to build the actual `ladspa=...` filter strings for mpv and
//!   FFmpeg hosts
//!   ([`crate::noise_reduction::mpv_ladspa_filter_for_model`],
//!   [`crate::noise_reduction::ffmpeg_ladspa_filter_for_model`] and the
//!   lower-level helpers).
//!
//! The integer encoding of [`crate::noise_reduction::NoiseModel`] is
//! serialized into user settings; renumbering the discriminants is a
//! breaking change.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Filesystem directory that hosts BigLinux LADSPA plugins.
pub const LADSPA_DIR_PATH: &str = "/usr/lib/ladspa";
/// Absolute path of the GTCRN LADSPA plugin shared object.
pub const GTCRN_LADSPA_PATH: &str = "/usr/lib/ladspa/libgtcrn_ladspa.so";
/// Absolute path of the DeepFilterNet LADSPA plugin shared object.
pub const DEEPFILTER_LADSPA_PATH: &str = "/usr/lib/ladspa/libdeep_filter_ladspa.so";
/// Absolute path of the high-resolution 48 kHz DPDFNet v2 plugin.
pub const DPDFNET_V2_HR_LADSPA_PATH: &str =
    "/usr/lib/ladspa/libdpdfnet_dpdfnet2_48khz_hr_ladspa.so";
/// Absolute path of the high-resolution 48 kHz DPDFNet v8 plugin.
pub const DPDFNET_V8_HR_LADSPA_PATH: &str =
    "/usr/lib/ladspa/libdpdfnet_dpdfnet8_48khz_hr_ladspa.so";
/// Absolute path of the DPDFNet baseline (16 kHz) plugin.
pub const DPDFNET_BASELINE_LADSPA_PATH: &str = "/usr/lib/ladspa/libdpdfnet_baseline_ladspa.so";
/// Absolute path of the 16 kHz DPDFNet v2 plugin.
pub const DPDFNET_V2_16K_LADSPA_PATH: &str = "/usr/lib/ladspa/libdpdfnet_dpdfnet2_ladspa.so";
/// Absolute path of the 16 kHz DPDFNet v4 plugin.
pub const DPDFNET_V4_16K_LADSPA_PATH: &str = "/usr/lib/ladspa/libdpdfnet_dpdfnet4_ladspa.so";
/// Absolute path of the 16 kHz DPDFNet v8 plugin.
pub const DPDFNET_V8_16K_LADSPA_PATH: &str = "/usr/lib/ladspa/libdpdfnet_dpdfnet8_ladspa.so";

/// LADSPA plugin label advertised by the GTCRN mono noise reducer.
pub const LABEL_GTCRN_MONO: &str = "gtcrn_mono";
/// LADSPA plugin label advertised by the DeepFilterNet mono reducer.
pub const LABEL_DEEPFILTER_MONO: &str = "deep_filter_mono";
/// LADSPA plugin label for high-resolution 48 kHz DPDFNet v2 mono.
pub const LABEL_DPDFNET_V2_HR_MONO: &str = "dpdfnet2_48khz_hr_mono";
/// LADSPA plugin label for high-resolution 48 kHz DPDFNet v8 mono.
pub const LABEL_DPDFNET_V8_HR_MONO: &str = "dpdfnet8_48khz_hr_mono";
/// LADSPA plugin label for the DPDFNet baseline mono reducer.
pub const LABEL_DPDFNET_BASELINE_MONO: &str = "baseline_mono";
/// LADSPA plugin label for the 16 kHz DPDFNet v2 mono reducer.
pub const LABEL_DPDFNET_V2_16K_MONO: &str = "dpdfnet2_mono";
/// LADSPA plugin label for the 16 kHz DPDFNet v4 mono reducer.
pub const LABEL_DPDFNET_V4_16K_MONO: &str = "dpdfnet4_mono";
/// LADSPA plugin label for the 16 kHz DPDFNet v8 mono reducer.
pub const LABEL_DPDFNET_V8_16K_MONO: &str = "dpdfnet8_mono";

/// Models exposed to the realtime LAVFI/PipeWire playback chain.
///
/// The order here is the order shown to users in the dropdown and stored
/// in their settings file; do not reshuffle without a migration plan.
pub const REALTIME_LAVFI_MODELS: [NoiseModel; 5] = [
    NoiseModel::GtcrnDns3,
    NoiseModel::GtcrnVctk,
    NoiseModel::DeepFilterNet3,
    NoiseModel::DpdfnetV2Hr,
    NoiseModel::DpdfnetV8Hr,
];

/// Human-readable labels matching [`REALTIME_LAVFI_MODELS`].
pub const REALTIME_LAVFI_MODEL_LABELS: [&str; 5] = [
    "GTCRN - DNS3",
    "GTCRN - VCTK",
    "DeepFilterNet3",
    "DPDFNet-2 HR",
    "DPDFNet-8 HR",
];

/// Stable identifier of a noise-reduction model.
///
/// The discriminants are serialized as `u8` via `serde` and stored in
/// user-facing settings files, so existing values must keep their numeric
/// meaning forever. Add new variants by appending; never renumber.
///
/// # Examples
///
/// ```
/// use big_audio_effects::noise_reduction::NoiseModel;
///
/// let model = NoiseModel::GtcrnDns3;
/// assert!(model.is_realtime_lavfi_supported());
/// assert!(!model.is_attenuation_only());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum NoiseModel {
    /// GTCRN model trained on the DNS3 corpus. Default selection.
    #[default]
    GtcrnDns3 = 0,
    /// GTCRN model trained on the VCTK corpus.
    GtcrnVctk = 1,
    /// DeepFilterNet3, attenuation-only realtime path.
    DeepFilterNet3 = 2,
    /// High-resolution 48 kHz DPDFNet v2.
    DpdfnetV2Hr = 3,
    /// High-resolution 48 kHz DPDFNet v8.
    DpdfnetV8Hr = 4,
    /// Baseline 16 kHz DPDFNet model.
    DpdfnetBaseline = 5,
    /// 16 kHz DPDFNet v2 model (offline pipelines only).
    DpdfnetV2 = 6,
    /// 16 kHz DPDFNet v4 model (offline pipelines only).
    DpdfnetV4 = 7,
    /// 16 kHz DPDFNet v8 model (offline pipelines only).
    DpdfnetV8 = 8,
}

impl TryFrom<u8> for NoiseModel {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::GtcrnDns3),
            1 => Ok(Self::GtcrnVctk),
            2 => Ok(Self::DeepFilterNet3),
            3 => Ok(Self::DpdfnetV2Hr),
            4 => Ok(Self::DpdfnetV8Hr),
            5 => Ok(Self::DpdfnetBaseline),
            6 => Ok(Self::DpdfnetV2),
            7 => Ok(Self::DpdfnetV4),
            8 => Ok(Self::DpdfnetV8),
            other => Err(format!("unknown NoiseModel value: {other}")),
        }
    }
}

impl From<NoiseModel> for u8 {
    fn from(model: NoiseModel) -> Self {
        model as Self
    }
}

impl NoiseModel {
    /// Resolve an arbitrary `i32` (e.g. from a settings file) to a model.
    ///
    /// Values that do not match any variant fall back to
    /// [`NoiseModel::default`] so corrupt or out-of-range settings never
    /// crash the UI.
    #[must_use]
    pub fn from_i32_or_default(value: i32) -> Self {
        u8::try_from(value)
            .ok()
            .and_then(|v| Self::try_from(v).ok())
            .unwrap_or_default()
    }

    /// Index of `self` inside [`REALTIME_LAVFI_MODELS`].
    ///
    /// Returns `0` when the model is not in the realtime list, matching
    /// the fallback used by dropdown widgets.
    #[must_use]
    pub fn realtime_lavfi_index(self) -> u32 {
        REALTIME_LAVFI_MODELS
            .iter()
            .position(|model| *model == self)
            .and_then(|idx| u32::try_from(idx).ok())
            .unwrap_or(0)
    }

    /// Indicates the model can be driven from the realtime LAVFI
    /// playback graph (vs offline-only ffmpeg pipelines); the UI uses
    /// this to filter the dropdown shown in the live denoise panel.
    #[must_use]
    pub fn is_realtime_lavfi_supported(self) -> bool {
        REALTIME_LAVFI_MODELS.contains(&self)
    }

    /// LADSPA `model` control-port value to select this variant.
    ///
    /// For dual-model GTCRN plugins this picks DNS3 (`0.0`) vs VCTK
    /// (`1.0`); for single-model plugins the value is irrelevant and is
    /// returned as `0.0`.
    #[must_use]
    pub fn ladspa_control(self) -> f32 {
        match self {
            Self::GtcrnVctk => 1.0,
            Self::GtcrnDns3
            | Self::DeepFilterNet3
            | Self::DpdfnetV2Hr
            | Self::DpdfnetV8Hr
            | Self::DpdfnetBaseline
            | Self::DpdfnetV2
            | Self::DpdfnetV4
            | Self::DpdfnetV8 => 0.0,
        }
    }

    /// Whether this is the DeepFilterNet family.
    #[must_use]
    pub fn is_deepfilter(self) -> bool {
        matches!(self, Self::DeepFilterNet3)
    }

    /// Whether this is any DPDFNet variant.
    #[must_use]
    pub fn is_dpdfnet(self) -> bool {
        matches!(
            self,
            Self::DpdfnetV2Hr
                | Self::DpdfnetV8Hr
                | Self::DpdfnetBaseline
                | Self::DpdfnetV2
                | Self::DpdfnetV4
                | Self::DpdfnetV8
        )
    }

    /// Whether this is one of the 16 kHz DPDFNet variants.
    #[must_use]
    pub fn is_dpdfnet_16khz(self) -> bool {
        matches!(
            self,
            Self::DpdfnetBaseline | Self::DpdfnetV2 | Self::DpdfnetV4 | Self::DpdfnetV8
        )
    }

    /// Whether the plugin uses a single attenuation control port.
    ///
    /// DeepFilterNet and DPDFNet plugins expose a single attenuation
    /// (`c0`) port instead of the GTCRN multi-port contract, so the
    /// filter-string formatters branch on this predicate.
    #[must_use]
    pub fn is_attenuation_only(self) -> bool {
        self.is_deepfilter() || self.is_dpdfnet()
    }

    /// Return the on-disk plugin path and its advertised label.
    #[must_use]
    pub fn plugin_and_label(self) -> (&'static str, &'static str) {
        match self {
            Self::GtcrnDns3 | Self::GtcrnVctk => (GTCRN_LADSPA_PATH, LABEL_GTCRN_MONO),
            Self::DeepFilterNet3 => (DEEPFILTER_LADSPA_PATH, LABEL_DEEPFILTER_MONO),
            Self::DpdfnetV2Hr => (DPDFNET_V2_HR_LADSPA_PATH, LABEL_DPDFNET_V2_HR_MONO),
            Self::DpdfnetV8Hr => (DPDFNET_V8_HR_LADSPA_PATH, LABEL_DPDFNET_V8_HR_MONO),
            Self::DpdfnetBaseline => (DPDFNET_BASELINE_LADSPA_PATH, LABEL_DPDFNET_BASELINE_MONO),
            Self::DpdfnetV2 => (DPDFNET_V2_16K_LADSPA_PATH, LABEL_DPDFNET_V2_16K_MONO),
            Self::DpdfnetV4 => (DPDFNET_V4_16K_LADSPA_PATH, LABEL_DPDFNET_V4_16K_MONO),
            Self::DpdfnetV8 => (DPDFNET_V8_16K_LADSPA_PATH, LABEL_DPDFNET_V8_16K_MONO),
        }
    }

    /// Whether the plugin file is present on disk right now.
    ///
    /// Performs a `stat` of the path returned by
    /// [`NoiseModel::plugin_and_label`]; callers should cache the result
    /// if they query it on a hot path.
    #[must_use]
    pub fn plugin_available(self) -> bool {
        Path::new(self.plugin_and_label().0).exists()
    }
}

/// Control-port values for the GTCRN LADSPA plugin.
///
/// The struct fields are listed in the same order that the plugin
/// expects them, so [`GtcrnControls::control_values`] can render them
/// straight into the LADSPA `controls=` argument.
///
/// # Examples
///
/// ```
/// use big_audio_effects::noise_reduction::GtcrnControls;
///
/// let player = GtcrnControls::player(0.8, 1, true, 0.3);
/// assert_eq!(player.control_values(), [0.8, 1.0, 0.8, 50.0, 1.0, 0.3]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GtcrnControls {
    /// Overall noise-reduction strength on a unit scale.
    pub strength: f64,
    /// Model selector forwarded to the plugin's `model` port.
    pub model: i32,
    /// Speech-preservation strength on a unit scale.
    pub speech_strength: f64,
    /// Lookahead buffer in milliseconds.
    pub lookahead_ms: f64,
    /// Whether to blend two GTCRN models in parallel.
    pub model_blend: bool,
    /// Voice recovery / desibilance amount on a unit scale.
    pub voice_recovery: f64,
}

impl GtcrnControls {
    /// Build controls suitable for the realtime player path.
    ///
    /// Uses a 50 ms lookahead and ties `speech_strength` to `strength`.
    #[must_use]
    pub fn player(strength: f64, model: i32, model_blend: bool, voice_recovery: f64) -> Self {
        Self {
            strength,
            model,
            speech_strength: strength,
            lookahead_ms: 50.0,
            model_blend,
            voice_recovery,
        }
    }

    /// Build controls suitable for the offline converter path.
    ///
    /// Disables lookahead and model blending and lets the caller pick
    /// the speech strength independently.
    #[must_use]
    pub fn converter(strength: f64, model: i32, speech_strength: f64, voice_recovery: f64) -> Self {
        Self {
            strength,
            model,
            speech_strength,
            lookahead_ms: 0.0,
            model_blend: false,
            voice_recovery,
        }
    }

    /// Project the struct to the LADSPA `controls=` `f32` array.
    #[must_use]
    pub fn control_values(self) -> [f32; 6] {
        [
            self.strength as f32,
            self.model as f32,
            self.speech_strength as f32,
            self.lookahead_ms as f32,
            f32::from(self.model_blend),
            self.voice_recovery as f32,
        ]
    }
}

/// Map a unit-scale strength slider to a DeepFilterNet attenuation in dB.
///
/// Uses a squared curve so the lower half of the slider is gentler than
/// the upper half, matching how DeepFilterNet's attenuation control
/// behaves audibly.
#[must_use]
pub fn deepfilter_attenuation_db(strength: f32) -> f64 {
    let s = strength.clamp(0.0, 1.0);
    f64::from(s * s) * 100.0
}

/// Render the GTCRN `controls=...` argument fragment.
///
/// The first port is always `c0=1` (enable). The remaining ports come
/// from `controls` in plugin-defined order.
#[must_use]
pub fn gtcrn_controls_fragment(controls: &GtcrnControls) -> String {
    format!(
        "c0=1|c1={}|c2={}|c3={}|c4={}|c5={}|c6={}",
        controls.strength,
        controls.model,
        controls.speech_strength,
        controls.lookahead_ms,
        i32::from(controls.model_blend),
        controls.voice_recovery,
    )
}

/// Build the GTCRN `ladspa=...` filter string for mpv.
///
/// mpv resolves LADSPA plugins by absolute path, so this variant emits
/// [`GTCRN_LADSPA_PATH`] verbatim.
#[must_use]
pub fn gtcrn_mpv_ladspa_filter(controls: &GtcrnControls) -> String {
    format!(
        "ladspa=file={}:plugin={LABEL_GTCRN_MONO}:controls={}",
        GTCRN_LADSPA_PATH,
        gtcrn_controls_fragment(controls)
    )
}

/// Build the GTCRN `ladspa=...` filter string for FFmpeg.
///
/// FFmpeg's LADSPA loader resolves plugins by short name from
/// `LADSPA_PATH`, so this variant uses `libgtcrn_ladspa` instead of the
/// absolute path used by the mpv variant.
#[must_use]
pub fn gtcrn_ffmpeg_ladspa_filter(controls: &GtcrnControls) -> String {
    format!(
        "ladspa=file=libgtcrn_ladspa:plugin={LABEL_GTCRN_MONO}:controls={}",
        gtcrn_controls_fragment(controls)
    )
}

/// Build a minimal GTCRN `ladspa=...` filter for Chromecast streaming.
///
/// The cast path only carries the overall strength, so this helper
/// emits a two-port `c0=1|c1=<strength>` controls fragment.
#[must_use]
pub fn gtcrn_cast_ladspa_filter(strength: f64) -> String {
    format!("ladspa=file=libgtcrn_ladspa:plugin={LABEL_GTCRN_MONO}:controls=c0=1|c1={strength:.2}")
}

/// Probes the filesystem for the GTCRN LADSPA `.so`; callers gate the
/// realtime denoise menu entry on this so missing plugins fail fast at
/// UI build time instead of producing a confusing runtime ffmpeg error.
#[must_use]
pub fn gtcrn_available() -> bool {
    Path::new(GTCRN_LADSPA_PATH).exists()
}

/// Reports whether any realtime-LAVFI model has its plugin on disk;
/// drives the top-level toggle that decides if the realtime denoise
/// pipeline is offered at all in the UI.
#[must_use]
pub fn any_realtime_lavfi_model_available() -> bool {
    realtime_lavfi_has_available_model(|model| model.plugin_available())
}

fn realtime_lavfi_has_available_model(is_model_available: impl Fn(NoiseModel) -> bool) -> bool {
    REALTIME_LAVFI_MODELS
        .iter()
        .copied()
        .any(is_model_available)
}

/// Render the `controls=c0=<db>` fragment for attenuation-only plugins.
///
/// Used by DeepFilterNet and DPDFNet, which expose a single port whose
/// unit is decibels of attenuation. The caller provides a unit-scale
/// strength which is converted via [`deepfilter_attenuation_db`].
#[must_use]
pub fn attenuation_controls_fragment(strength: f64) -> String {
    format!("c0={:.2}", deepfilter_attenuation_db(strength as f32))
}

/// Pick the correct mpv `ladspa=...` filter string for `model`.
///
/// Returns `None` when the model is not exposed to the realtime LAVFI
/// path or when the plugin's `.so` file is not currently installed.
/// Otherwise dispatches between the GTCRN multi-port path and the
/// attenuation-only path based on [`NoiseModel::is_attenuation_only`].
#[must_use]
pub fn mpv_ladspa_filter_for_model(model: NoiseModel, controls: &GtcrnControls) -> Option<String> {
    mpv_ladspa_filter_for_available_model(model, controls, model.plugin_available())
}

fn mpv_ladspa_filter_for_available_model(
    model: NoiseModel,
    controls: &GtcrnControls,
    is_plugin_available: bool,
) -> Option<String> {
    if !model.is_realtime_lavfi_supported() || !is_plugin_available {
        return None;
    }
    if model.is_attenuation_only() {
        let (plugin, label) = model.plugin_and_label();
        Some(format!(
            "ladspa=file={plugin}:plugin={label}:controls={}",
            attenuation_controls_fragment(controls.strength)
        ))
    } else {
        Some(gtcrn_mpv_ladspa_filter(controls))
    }
}

/// Pick the correct FFmpeg `ladspa=...` filter string for `model`.
///
/// Semantically identical to [`mpv_ladspa_filter_for_model`] but emits
/// the FFmpeg-flavored plugin reference (short library name with no
/// `.so` suffix or absolute path).
#[must_use]
pub fn ffmpeg_ladspa_filter_for_model(
    model: NoiseModel,
    controls: &GtcrnControls,
) -> Option<String> {
    ffmpeg_ladspa_filter_for_available_model(model, controls, model.plugin_available())
}

fn ffmpeg_ladspa_filter_for_available_model(
    model: NoiseModel,
    controls: &GtcrnControls,
    is_plugin_available: bool,
) -> Option<String> {
    if !model.is_realtime_lavfi_supported() || !is_plugin_available {
        return None;
    }
    if model.is_attenuation_only() {
        let (plugin, label) = model.plugin_and_label();
        Some(format!(
            "ladspa=file={plugin}:plugin={label}:controls={}",
            attenuation_controls_fragment(controls.strength)
        ))
    } else {
        Some(gtcrn_ffmpeg_ladspa_filter(controls))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gtcrn_control_values_keep_contract_order() {
        let controls = GtcrnControls::player(0.8, 1, true, 0.3);
        assert_eq!(controls.control_values(), [0.8, 1.0, 0.8, 50.0, 1.0, 0.3]);
    }

    #[test]
    fn noise_model_numeric_encoding_roundtrips() {
        let models = [
            (0, NoiseModel::GtcrnDns3),
            (1, NoiseModel::GtcrnVctk),
            (2, NoiseModel::DeepFilterNet3),
            (3, NoiseModel::DpdfnetV2Hr),
            (4, NoiseModel::DpdfnetV8Hr),
            (5, NoiseModel::DpdfnetBaseline),
            (6, NoiseModel::DpdfnetV2),
            (7, NoiseModel::DpdfnetV4),
            (8, NoiseModel::DpdfnetV8),
        ];

        for (value, model) in models {
            assert_eq!(NoiseModel::try_from(value), Ok(model));
            assert_eq!(u8::from(model), value);
            assert_eq!(NoiseModel::from_i32_or_default(i32::from(value)), model);
        }
        assert!(NoiseModel::try_from(9).is_err());
        assert_eq!(NoiseModel::from_i32_or_default(-1), NoiseModel::default());
        assert_eq!(NoiseModel::from_i32_or_default(99), NoiseModel::default());
    }

    #[test]
    fn noise_model_ladspa_control_values_match_plugin_contract() {
        assert_eq!(NoiseModel::GtcrnVctk.ladspa_control(), 1.0);
        for model in [
            NoiseModel::GtcrnDns3,
            NoiseModel::DeepFilterNet3,
            NoiseModel::DpdfnetV2Hr,
            NoiseModel::DpdfnetV8Hr,
            NoiseModel::DpdfnetBaseline,
            NoiseModel::DpdfnetV2,
            NoiseModel::DpdfnetV4,
            NoiseModel::DpdfnetV8,
        ] {
            assert_eq!(model.ladspa_control(), 0.0);
        }
    }

    #[test]
    fn gtcrn_filters_keep_host_specific_paths() {
        let controls = GtcrnControls::converter(0.7, 2, 0.5, 0.4);
        assert!(gtcrn_ffmpeg_ladspa_filter(&controls).starts_with("ladspa=file=libgtcrn_ladspa"));
        assert!(gtcrn_mpv_ladspa_filter(&controls).starts_with("ladspa=file=/usr/lib/ladspa"));
    }

    #[test]
    fn gtcrn_filter_fragments_match_plugin_contract() {
        let controls = GtcrnControls::player(0.8, 1, true, 0.3);

        assert_eq!(
            gtcrn_controls_fragment(&controls),
            "c0=1|c1=0.8|c2=1|c3=0.8|c4=50|c5=1|c6=0.3"
        );
        assert_eq!(
            gtcrn_cast_ladspa_filter(0.75),
            "ladspa=file=libgtcrn_ladspa:plugin=gtcrn_mono:controls=c0=1|c1=0.75"
        );
    }

    #[test]
    fn noise_models_classify_backend_families() {
        assert!(!NoiseModel::GtcrnDns3.is_attenuation_only());
        assert!(NoiseModel::DeepFilterNet3.is_attenuation_only());
        assert!(!NoiseModel::GtcrnDns3.is_dpdfnet());
        assert!(!NoiseModel::GtcrnVctk.is_dpdfnet());
        assert!(!NoiseModel::DeepFilterNet3.is_dpdfnet());
        assert!(NoiseModel::DpdfnetV2Hr.is_dpdfnet());
        assert!(NoiseModel::DpdfnetV8Hr.is_dpdfnet());
        assert!(NoiseModel::DpdfnetBaseline.is_dpdfnet());
        assert!(NoiseModel::DpdfnetV2.is_dpdfnet());
        assert!(NoiseModel::DpdfnetV4.is_dpdfnet());
        assert!(NoiseModel::DpdfnetV8.is_dpdfnet());
        assert!(NoiseModel::DpdfnetV2.is_dpdfnet_16khz());
        assert!(!NoiseModel::DpdfnetV2Hr.is_dpdfnet_16khz());
        assert!(NoiseModel::DpdfnetV2Hr.is_attenuation_only());
    }

    #[test]
    fn realtime_lavfi_models_are_contiguous_for_dropdown_storage() {
        for (idx, model) in REALTIME_LAVFI_MODELS.iter().enumerate() {
            assert_eq!(*model as usize, idx);
            assert_eq!(model.realtime_lavfi_index(), idx as u32);
            assert!(model.is_realtime_lavfi_supported());
        }
        assert_eq!(
            REALTIME_LAVFI_MODELS.len(),
            REALTIME_LAVFI_MODEL_LABELS.len()
        );
        assert!(!NoiseModel::DpdfnetV2.is_realtime_lavfi_supported());
    }

    #[test]
    fn attenuation_curve_preserves_slider_travel() {
        assert!((deepfilter_attenuation_db(0.5) - 25.0).abs() < f64::EPSILON);
        assert!((deepfilter_attenuation_db(2.0) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn attenuation_filter_uses_single_control_port() {
        let controls = GtcrnControls::player(0.5, 0, false, 0.0);
        let filter =
            mpv_ladspa_filter_for_available_model(NoiseModel::DeepFilterNet3, &controls, true)
                .expect("available realtime model should render");
        assert!(filter.contains("plugin=deep_filter_mono"));
        assert!(filter.contains("controls=c0=25.00"));
    }

    #[test]
    fn realtime_availability_uses_model_predicate() {
        assert!(!realtime_lavfi_has_available_model(|_| false));
        assert!(realtime_lavfi_has_available_model(
            |model| model == NoiseModel::DeepFilterNet3
        ));
    }

    #[test]
    fn host_filter_builders_respect_realtime_and_availability_contracts() {
        let controls = GtcrnControls::player(0.5, 1, false, 0.2);

        assert!(
            mpv_ladspa_filter_for_available_model(NoiseModel::DeepFilterNet3, &controls, false)
                .is_none()
        );
        assert!(
            ffmpeg_ladspa_filter_for_available_model(NoiseModel::DpdfnetV2, &controls, true)
                .is_none()
        );

        let mpv_gtcrn =
            mpv_ladspa_filter_for_available_model(NoiseModel::GtcrnDns3, &controls, true)
                .expect("available GTCRN model should render for mpv");
        assert!(mpv_gtcrn.starts_with("ladspa=file=/usr/lib/ladspa/libgtcrn_ladspa.so"));
        assert!(mpv_gtcrn.contains("plugin=gtcrn_mono"));
        assert!(mpv_gtcrn.contains("controls=c0=1|c1=0.5|c2=1|c3=0.5|c4=50|c5=0|c6=0.2"));

        let ffmpeg_gtcrn =
            ffmpeg_ladspa_filter_for_available_model(NoiseModel::GtcrnDns3, &controls, true)
                .expect("available GTCRN model should render for FFmpeg");
        assert!(ffmpeg_gtcrn.starts_with("ladspa=file=libgtcrn_ladspa"));
        assert!(ffmpeg_gtcrn.contains("plugin=gtcrn_mono"));

        let ffmpeg_deepfilter =
            ffmpeg_ladspa_filter_for_available_model(NoiseModel::DeepFilterNet3, &controls, true)
                .expect("available attenuation model should render for FFmpeg");
        assert!(ffmpeg_deepfilter.contains("file=/usr/lib/ladspa/libdeep_filter_ladspa.so"));
        assert!(ffmpeg_deepfilter.contains("controls=c0=25.00"));
    }
}
