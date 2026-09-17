#![deny(missing_docs)]
#![forbid(unsafe_code)]
//! BigLinux mpv render/offload primitives.
//!
//! This crate is intentionally small until `big-video-player` and the
//! `big-video-converter` editor share a stable render-surface API.
//!
//! It exposes the agreed direct-video-path policy (which render
//! components are mandatory and which copy paths are forbidden for the
//! main video surface) together with a small catalogue of mpv render
//! profiles that BigLinux media apps map onto user-facing quality
//! presets.
//!
//! The items here are pure data and pure functions: no runtime, no
//! GTK/mpv dependency. They are meant to be embedded by the higher-level
//! `big-media-runtime` crate and by the BigLinux media apps to keep the
//! quality contracts and the forbidden copy paths in one place.

/// Names of the render-pipeline components that BigLinux apps **must**
/// keep on the main video path.
///
/// The list is used by audits and by `component-check.mjs` to detect
/// regressions where a BigLinux media app accidentally drops one of the
/// zero-copy primitives.
pub const REQUIRED_MAIN_VIDEO_PATH: &[&str] = &[
    "mpv render API",
    "FastRenderContext",
    "OffloadRenderer",
    "GdkDmabufTexture or GdkGLTexture",
    "GLArea fallback",
];

/// CPU/screenshot paths excluded from the preferred **direct GPU** mode.
///
/// This is not a ban on a usable software fallback. Memory textures and RGB
/// buffers are allowed in [`VideoSurfaceMode::Compatibility`]. A software
/// buffer may be leased without an extra CPU memcpy; it still is not evidence
/// of an end-to-end GPU zero-copy path.
pub const FORBIDDEN_MAIN_VIDEO_COPY_PATHS: &[&str] =
    &["Pixbuf", "MemoryTexture", "RGB buffers", "screenshots"];

/// A render path or surface used by the main video area.
///
/// The enum keeps the BigLinux video-surface policy testable without
/// depending on GTK, mpv, or a display server. Higher-level crates can
/// map their concrete backend choices onto these variants and reject CPU
/// copy paths before they reach the playback UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MainVideoSurfacePath {
    /// mpv's render API drives frame rendering directly.
    MpvRenderApi,
    /// BigLinux fast render context around the mpv render API.
    FastRenderContext,
    /// Runtime backend detection chooses the best supported surface.
    VideoBackendDetection,
    /// GTK graphics offload path used when the compositor supports it.
    OffloadRenderer,
    /// DMA-BUF texture handoff through GTK.
    GdkDmabufTexture,
    /// GL texture handoff through GTK.
    GdkGlTexture,
    /// GLArea fallback used only when offload is unavailable.
    GlAreaFallback,
    /// CPU-side GDK pixbuf copy.
    Pixbuf,
    /// CPU-backed GDK memory texture (possibly leased without a CPU memcpy).
    MemoryTexture,
    /// CPU-side RGB frame buffer.
    RgbBuffer,
    /// Screenshot-based frame extraction path.
    Screenshot,
}

impl MainVideoSurfacePath {
    /// Legacy shorthand for the preferred direct-GPU mode.
    /// Use [`Self::is_allowed_for_mode`] for explicit compatibility fallback.
    #[must_use]
    pub const fn is_allowed_for_main_video_surface(self) -> bool {
        !self.is_cpu_copy_path()
    }

    /// Legacy classification of CPU-backed/extraction paths, not a measurement
    /// of copies performed. A memory texture can retain an existing buffer.
    #[must_use]
    pub const fn is_cpu_copy_path(self) -> bool {
        matches!(
            self,
            Self::Pixbuf | Self::MemoryTexture | Self::RgbBuffer | Self::Screenshot
        )
    }

    /// Return whether this path supports a GPU texture handoff. This historical
    /// name is not proof of zero-copy decode, import, composition or scanout:
    /// actual formats, modifiers, synchronization and driver behavior matter.
    #[must_use]
    pub const fn is_zero_copy_handoff(self) -> bool {
        matches!(self, Self::GdkDmabufTexture | Self::GdkGlTexture)
    }

    /// Apply the explicit rendering mode rather than rejecting compatibility
    /// merely because a machine cannot provide working OpenGL/offload.
    #[must_use]
    pub const fn is_allowed_for_mode(self, mode: VideoSurfaceMode) -> bool {
        self.is_allowed_for_main_video_surface()
            || matches!(
                (mode, self),
                (
                    VideoSurfaceMode::Compatibility,
                    Self::MemoryTexture | Self::RgbBuffer
                )
            )
    }
}

/// User intent/capability outcome for the main video surface. Direct rendering
/// stays preferred; compatibility is a deliberate, observable fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoSurfaceMode {
    /// Prefer the hardware/direct path; avoid CPU-backed frame transfer.
    Direct,
    /// Allow bounded software frame leases when GPU rendering is unavailable.
    Compatibility,
}

/// Typed render-surface steps required by the direct main-video contract.
pub const REQUIRED_DIRECT_VIDEO_SURFACE_PATH: &[MainVideoSurfacePath] = &[
    MainVideoSurfacePath::MpvRenderApi,
    MainVideoSurfacePath::FastRenderContext,
    MainVideoSurfacePath::VideoBackendDetection,
    MainVideoSurfacePath::OffloadRenderer,
    MainVideoSurfacePath::GdkDmabufTexture,
    MainVideoSurfacePath::GdkGlTexture,
    MainVideoSurfacePath::GlAreaFallback,
];

/// Preferred backend order for main video surfaces.
pub const PREFERRED_MAIN_VIDEO_BACKENDS: &[MainVideoSurfacePath] = &[
    MainVideoSurfacePath::OffloadRenderer,
    MainVideoSurfacePath::GlAreaFallback,
];

/// Typed paths that are forbidden for the main video surface.
pub const FORBIDDEN_MAIN_VIDEO_SURFACE_PATHS: &[MainVideoSurfacePath] = &[
    MainVideoSurfacePath::Pixbuf,
    MainVideoSurfacePath::MemoryTexture,
    MainVideoSurfacePath::RgbBuffer,
    MainVideoSurfacePath::Screenshot,
];

/// Value of an mpv render property.
///
/// mpv accepts textual enums (for example `"bilinear"`, `"lanczos"`)
/// and boolean toggles (for example `interpolation=yes|no`).  This enum
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
    /// mpv property name, for example `"scale"` or `"interpolation"`.
    pub name: &'static str,
    /// Property value, see [`MpvPropertyValue`].
    pub value: MpvPropertyValue,
}

/// Default render profile: cheapest scalers, no interpolation, no
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
    text("video-sync", "audio"),
    bool_prop("interpolation", false),
    text("deband", "no"),
    bool_prop("hdr-compute-peak", false),
];

/// Balanced render profile: lanczos scalers with correct downscaling
/// and sigmoid upscaling, still without interpolation or debanding.
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
    text("video-sync", "audio"),
    bool_prop("interpolation", false),
    text("deband", "no"),
    bool_prop("hdr-compute-peak", false),
];

/// High-quality render profile: ewa_lanczossharp/oft scalers,
/// display-resample sync, interpolation, debanding and HDR compute
/// peak.  Targets desktops with capable GPUs.
pub const HIGH_QUALITY_RENDER_PROFILE: &[MpvProperty] = &[
    text("scale", "ewa_lanczossharp"),
    text("cscale", "ewa_lanczossoft"),
    text("dscale", "mitchell"),
    text("correct-downscaling", "yes"),
    text("sigmoid-upscaling", "yes"),
    text("linear-downscaling", "yes"),
    text("dither-depth", "auto"),
    text("video-sync", "display-resample"),
    bool_prop("interpolation", true),
    text("tscale", "oversample"),
    text("deband", "yes"),
    bool_prop("hdr-compute-peak", true),
];

/// Parse a user-supplied list of custom mpv shader paths.
///
/// Accepts both `,` and `:` as separators (mpv itself accepts `:` on
/// Unix; BigLinux GSettings historically used `,`).  Empty segments are
/// dropped and the order is preserved.
///
/// # Examples
///
/// ```
/// use big_mpv_render::parse_custom_shader_paths;
///
/// assert_eq!(
///     parse_custom_shader_paths("/a.glsl,/b.glsl:/c.glsl"),
///     vec!["/a.glsl", "/b.glsl", "/c.glsl"],
/// );
/// assert!(parse_custom_shader_paths("").is_empty());
/// ```
#[must_use]
pub fn parse_custom_shader_paths(custom: &str) -> Vec<String> {
    if custom.is_empty() {
        return Vec::new();
    }
    custom
        .split([',', ':'])
        .filter(|segment| !segment.is_empty())
        .map(String::from)
        .collect()
}

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
    fn policy_names_direct_video_path() {
        assert!(REQUIRED_MAIN_VIDEO_PATH.contains(&"FastRenderContext"));
        assert!(REQUIRED_MAIN_VIDEO_PATH.contains(&"OffloadRenderer"));
        assert!(FORBIDDEN_MAIN_VIDEO_COPY_PATHS.contains(&"MemoryTexture"));
    }

    #[test]
    fn main_video_surface_policy_rejects_cpu_copy_paths() {
        for surface_path in FORBIDDEN_MAIN_VIDEO_SURFACE_PATHS {
            assert!(surface_path.is_cpu_copy_path(), "{surface_path:?}");
            assert!(
                !surface_path.is_allowed_for_main_video_surface(),
                "{surface_path:?}"
            );
        }
    }

    #[test]
    fn direct_video_path_keeps_mpv_fast_context_and_zero_copy_handoff() {
        assert!(REQUIRED_DIRECT_VIDEO_SURFACE_PATH.contains(&MainVideoSurfacePath::MpvRenderApi));
        assert!(
            REQUIRED_DIRECT_VIDEO_SURFACE_PATH.contains(&MainVideoSurfacePath::FastRenderContext)
        );
        assert!(
            REQUIRED_DIRECT_VIDEO_SURFACE_PATH
                .iter()
                .any(|surface_path| surface_path.is_zero_copy_handoff())
        );
        assert!(
            REQUIRED_DIRECT_VIDEO_SURFACE_PATH
                .iter()
                .all(|surface_path| surface_path.is_allowed_for_main_video_surface())
        );
    }

    #[test]
    fn zero_copy_handoff_is_only_the_texture_handoff_step() {
        assert!(MainVideoSurfacePath::GdkDmabufTexture.is_zero_copy_handoff());
        assert!(MainVideoSurfacePath::GdkGlTexture.is_zero_copy_handoff());
        assert!(!MainVideoSurfacePath::OffloadRenderer.is_zero_copy_handoff());
        assert!(!MainVideoSurfacePath::GlAreaFallback.is_zero_copy_handoff());
        assert!(!MainVideoSurfacePath::MemoryTexture.is_zero_copy_handoff());
    }

    #[test]
    fn preferred_backend_order_tries_offload_before_glarea_fallback() {
        assert_eq!(
            PREFERRED_MAIN_VIDEO_BACKENDS,
            &[
                MainVideoSurfacePath::OffloadRenderer,
                MainVideoSurfacePath::GlAreaFallback,
            ]
        );
    }

    #[test]
    fn compatibility_accepts_bounded_cpu_surfaces_but_not_screenshot_playback() {
        for path in [
            MainVideoSurfacePath::MemoryTexture,
            MainVideoSurfacePath::RgbBuffer,
        ] {
            assert!(path.is_allowed_for_mode(VideoSurfaceMode::Compatibility));
            assert!(!path.is_allowed_for_mode(VideoSurfaceMode::Direct));
        }
        assert!(
            !MainVideoSurfacePath::Screenshot.is_allowed_for_mode(VideoSurfaceMode::Compatibility)
        );
        assert!(!MainVideoSurfacePath::Pixbuf.is_allowed_for_mode(VideoSurfaceMode::Compatibility));
    }

    #[test]
    fn render_profiles_keep_expected_quality_tradeoffs() {
        assert!(DEFAULT_RENDER_PROFILE.contains(&text("scale", "bilinear")));
        assert!(BALANCED_RENDER_PROFILE.contains(&text("scale", "lanczos")));
        assert!(HIGH_QUALITY_RENDER_PROFILE.contains(&text("video-sync", "display-resample")));
        assert!(HIGH_QUALITY_RENDER_PROFILE.contains(&bool_prop("interpolation", true)));
    }

    #[test]
    fn unknown_render_profile_falls_back_to_default() {
        assert_eq!(render_profile_properties("unknown"), DEFAULT_RENDER_PROFILE);
    }

    #[test]
    fn custom_shader_path_list_accepts_comma_and_colon_separators() {
        assert_eq!(
            parse_custom_shader_paths("/a.glsl,/b.glsl:/c.glsl"),
            vec!["/a.glsl", "/b.glsl", "/c.glsl"]
        );
        assert_eq!(
            parse_custom_shader_paths(":/a.glsl,,/b.glsl:"),
            vec!["/a.glsl", "/b.glsl"]
        );
        assert!(parse_custom_shader_paths("").is_empty());
    }
}
