//! Pure, GTK-free cast helpers: device-row label formatting and the
//! cast-target decision (URL vs direct file vs transcode vs error).
//!
//! Factored out of the Relm4 cast dialog so the branching logic — which
//! mirrors the legacy `device_dialog`'s nested `if` chain — is unit-testable
//! without a running mpv backend or a live Chromecast.

/// Human-readable label for a cast device row: `name` alone, or
/// `name (model)` when a non-empty model string is known.
#[must_use]
pub fn device_label(name: &str, model: &str) -> String {
    if model.is_empty() {
        name.to_string()
    } else {
        format!("{name} ({model})")
    }
}

/// `host:port` subtitle for a cast device row.
#[must_use]
pub fn device_subtitle(host: &str, port: u16) -> String {
    format!("{host}:{port}")
}

/// What the cast dialog should do with the currently loaded media when a
/// device is picked. Computed from the media path + probe + tool availability,
/// keeping the (testable) decision separate from the (untestable) network I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CastTarget {
    /// Remote stream — cast the URL directly with the given MIME type.
    Url { mime: &'static str },
    /// Local file the device can play as-is — serve and cast directly.
    DirectFile,
    /// Local file the device cannot play — transcode through ffmpeg.
    Transcode,
    /// Nothing playable / a missing prerequisite. Carries an error kind for
    /// the caller to map to a translated message.
    Error(CastTargetError),
}

/// Why a cast could not start (paired with [`CastTarget::Error`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastTargetError {
    /// No media is loaded.
    NoMedia,
    /// The file needs transcoding but ffmpeg is not installed.
    FfmpegMissing,
}

/// Decide how to cast `file_path`.
///
/// - `None` path → [`CastTargetError::NoMedia`].
/// - `http(s)://` path → [`CastTarget::Url`] (`video/mp4`).
/// - local file + `direct_compatible` → [`CastTarget::DirectFile`].
/// - local file, not compatible, ffmpeg present → [`CastTarget::Transcode`].
/// - local file, not compatible, ffmpeg absent → [`CastTargetError::FfmpegMissing`].
#[must_use]
pub fn decide_cast_target(
    file_path: Option<&str>,
    direct_compatible: bool,
    ffmpeg_available: bool,
) -> CastTarget {
    let Some(path) = file_path.filter(|p| !p.is_empty()) else {
        return CastTarget::Error(CastTargetError::NoMedia);
    };
    if path.starts_with("http://") || path.starts_with("https://") {
        return CastTarget::Url { mime: "video/mp4" };
    }
    if direct_compatible {
        CastTarget::DirectFile
    } else if ffmpeg_available {
        CastTarget::Transcode
    } else {
        CastTarget::Error(CastTargetError::FfmpegMissing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_label_omits_empty_model() {
        assert_eq!(device_label("Living Room TV", ""), "Living Room TV");
    }

    #[test]
    fn device_label_appends_model() {
        assert_eq!(
            device_label("Living Room TV", "Chromecast Ultra"),
            "Living Room TV (Chromecast Ultra)"
        );
    }

    #[test]
    fn device_subtitle_joins_host_port() {
        assert_eq!(device_subtitle("192.168.0.5", 8009), "192.168.0.5:8009");
    }

    #[test]
    fn no_media_is_error() {
        assert_eq!(
            decide_cast_target(None, false, true),
            CastTarget::Error(CastTargetError::NoMedia)
        );
        assert_eq!(
            decide_cast_target(Some(""), false, true),
            CastTarget::Error(CastTargetError::NoMedia)
        );
    }

    #[test]
    fn http_url_casts_directly() {
        assert_eq!(
            decide_cast_target(Some("http://host/stream.m3u8"), false, false),
            CastTarget::Url { mime: "video/mp4" }
        );
        assert_eq!(
            decide_cast_target(Some("https://host/v.mp4"), false, false),
            CastTarget::Url { mime: "video/mp4" }
        );
    }

    #[test]
    fn compatible_local_file_is_direct() {
        assert_eq!(
            decide_cast_target(Some("/media/clip.mp4"), true, false),
            CastTarget::DirectFile
        );
    }

    #[test]
    fn incompatible_local_file_transcodes_when_ffmpeg_present() {
        assert_eq!(
            decide_cast_target(Some("/media/clip.mkv"), false, true),
            CastTarget::Transcode
        );
    }

    #[test]
    fn incompatible_local_file_errors_without_ffmpeg() {
        assert_eq!(
            decide_cast_target(Some("/media/clip.mkv"), false, false),
            CastTarget::Error(CastTargetError::FfmpegMissing)
        );
    }
}
