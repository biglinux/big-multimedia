#![deny(missing_docs)]
#![forbid(unsafe_code)]
//! mpv render profiles.
//!
//! Pure data (no GTK/mpv dependency) consumed by `big-media-runtime`: the mpv
//! property tables behind the user-facing render quality presets.

/// Value of an mpv render property.
///
/// mpv accepts textual enums (for example `"bilinear"`, `"lanczos"`)
/// and boolean toggles (for example `deband=yes|no`).  This enum
/// keeps both shapes in a single, `Copy` type so the render profiles can
/// live in `const` slices.
///
/// # Examples
///
/// ```
/// use big_mpv_render::MpvPropertyValue;
///
/// let scale = MpvPropertyValue::Text("lanczos");
/// let interp = MpvPropertyValue::Bool(true);
/// assert_ne!(scale, interp);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpvPropertyValue {
    /// Textual mpv enum value, for example `"lanczos"` or `"auto"`.
    Text(&'static str),
    /// Boolean mpv toggle. `true` maps to `yes`, `false` to `no`.
    Bool(bool),
}

/// A single mpv render property: name plus value.
///
/// Properties are stored as `'static` slices so that whole render
/// profiles can be built at compile time and compared with `==`.
///
/// # Examples
///
/// ```
/// use big_mpv_render::{DEFAULT_RENDER_PROFILE, MpvPropertyValue};
///
/// let scale = DEFAULT_RENDER_PROFILE
///     .iter()
///     .find(|p| p.name == "scale")
///     .expect("default profile sets scale");
/// assert_eq!(scale.value, MpvPropertyValue::Text("bilinear"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MpvProperty {
    /// mpv property name, for example `"scale"` or `"deband"`.
    pub name: &'static str,
    /// Property value, see [`MpvPropertyValue`].
    pub value: MpvPropertyValue,
}

/// Default render profile: cheapest scalers, no
/// debanding.  Targets low-end GPUs and battery-powered playback.
///
/// # Examples
///
/// ```
/// use big_mpv_render::DEFAULT_RENDER_PROFILE;
///
/// assert!(DEFAULT_RENDER_PROFILE.iter().any(|p| p.name == "scale"));
/// ```
pub const DEFAULT_RENDER_PROFILE: &[MpvProperty] = &[
    text("scale", "bilinear"),
    text("dscale", "bilinear"),
    text("cscale", "bilinear"),
    text("correct-downscaling", "no"),
    text("sigmoid-upscaling", "no"),
    text("linear-downscaling", "no"),
    text("dither-depth", "no"),
    text("deband", "no"),
    bool_prop("hdr-compute-peak", false),
];

/// Balanced render profile: lanczos scalers with correct downscaling
/// and sigmoid upscaling, still without debanding.
///
/// # Examples
///
/// ```
/// use big_mpv_render::{BALANCED_RENDER_PROFILE, MpvPropertyValue};
///
/// let scale = BALANCED_RENDER_PROFILE
///     .iter()
///     .find(|p| p.name == "scale")
///     .unwrap();
/// assert_eq!(scale.value, MpvPropertyValue::Text("lanczos"));
/// ```
pub const BALANCED_RENDER_PROFILE: &[MpvProperty] = &[
    text("scale", "lanczos"),
    text("dscale", "lanczos"),
    text("cscale", "lanczos"),
    text("correct-downscaling", "yes"),
    text("sigmoid-upscaling", "yes"),
    text("linear-downscaling", "no"),
    text("dither-depth", "auto"),
    text("deband", "no"),
    bool_prop("hdr-compute-peak", false),
];

/// High-quality render profile: ewa_lanczossharp/oft scalers,
/// debanding and HDR compute
/// peak.  Targets desktops with capable GPUs.
pub const HIGH_QUALITY_RENDER_PROFILE: &[MpvProperty] = &[
    text("scale", "ewa_lanczossharp"),
    text("cscale", "ewa_lanczossoft"),
    text("dscale", "mitchell"),
    text("correct-downscaling", "yes"),
    text("sigmoid-upscaling", "yes"),
    text("linear-downscaling", "yes"),
    text("dither-depth", "auto"),
    text("deband", "yes"),
    bool_prop("hdr-compute-peak", true),
];

/// Resolve a render-profile name into the matching property slice.
///
/// Recognised names are `"balanced"` and `"high-quality"`.  Any other
/// value (including the empty string and `"default"`) falls back to
/// [`DEFAULT_RENDER_PROFILE`], so callers can pass user input straight
/// through.
///
/// # Examples
///
/// ```
/// use big_mpv_render::{
///     BALANCED_RENDER_PROFILE, DEFAULT_RENDER_PROFILE,
///     HIGH_QUALITY_RENDER_PROFILE, render_profile_properties,
/// };
///
/// assert_eq!(render_profile_properties("balanced"), BALANCED_RENDER_PROFILE);
/// assert_eq!(render_profile_properties("high-quality"), HIGH_QUALITY_RENDER_PROFILE);
/// assert_eq!(render_profile_properties("unknown"), DEFAULT_RENDER_PROFILE);
/// ```
#[must_use]
pub const fn render_profile_properties(profile: &str) -> &'static [MpvProperty] {
    match profile.as_bytes() {
        b"balanced" => BALANCED_RENDER_PROFILE,
        b"high-quality" => HIGH_QUALITY_RENDER_PROFILE,
        _ => DEFAULT_RENDER_PROFILE,
    }
}

const fn text(name: &'static str, value: &'static str) -> MpvProperty {
    MpvProperty {
        name,
        value: MpvPropertyValue::Text(value),
    }
}

const fn bool_prop(name: &'static str, value: bool) -> MpvProperty {
    MpvProperty {
        name,
        value: MpvPropertyValue::Bool(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_profiles_keep_expected_quality_tradeoffs() {
        assert!(DEFAULT_RENDER_PROFILE.contains(&text("scale", "bilinear")));
        assert!(BALANCED_RENDER_PROFILE.contains(&text("scale", "lanczos")));
        assert!(HIGH_QUALITY_RENDER_PROFILE.contains(&text("deband", "yes")));
    }

    #[test]
    fn unknown_render_profile_falls_back_to_default() {
        assert_eq!(render_profile_properties("unknown"), DEFAULT_RENDER_PROFILE);
    }
}
