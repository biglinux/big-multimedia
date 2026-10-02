//! Value types derived from the typed key catalog. Unknown legacy keys are
//! retained if they obey the line-oriented format; known numeric values must
//! parse and be finite. Regenerate/check alongside keys.rs in the review gate.
pub(super) fn valid(key: &str, value: &str) -> bool {
    if key.is_empty()
        || key.len() > 128
        || value.len() > 65536
        || !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || value.chars().any(|c| c.is_control())
    {
        return false;
    }
    match key {
        "mute"
        | "deinterlace"
        | "auto-window-resize"
        | "audio-normalize"
        | "noise-mono"
        | "audio-pitch-correction"
        | "sub-bold"
        | "sub-scale-with-window"
        | "sub-ass-scale-with-window"
        | "sub-gray"
        | "sub-filter-sdh"
        | "sub-filter-sdh-harder"
        | "secondary-subtitles"
        | "resume-playback"
        | "keep-open-pause"
        | "hdr-compute-peak"
        | "target-colorspace-hint"
        | "cache"
        | "cache-pause"
        | "cache-on-disk"
        | "screenshot-tag-colorspace"
        | "screenshot-high-bit-depth"
        | "gpu-dumb-mode"
        | "icc-profile-auto"
        | "seekbar-thumbnail"
        | "playlist-folder-recursive"
        | "playlist-sort-desc"
        | "playlist-shuffle"
        | "ui-osd-shadow"
        | "ui-button-custom"
        | "ui-window-radius-custom"
        | "ui-window-btn-custom-size"
        | "ui-show-volume"
        | "ui-show-speed"
        | "ui-show-voice"
        | "ui-show-play"
        | "ui-show-prev"
        | "ui-show-next"
        | "ui-show-subtitle"
        | "ui-show-audio-track"
        | "ui-show-fullscreen"
        | "ui-show-tv"
        | "ui-show-pin"
        | "ui-show-open"
        | "ui-show-playlist"
        | "ui-show-menu"
        | "gapless-audio"
        | "audio-high-contrast"
        | "taskbar-progress"
        | "audio-album-art-bg" => matches!(value, "true" | "false"),
        "appearance-window-width"
        | "appearance-window-height"
        | "audio-appearance-window-width"
        | "audio-appearance-window-height"
        | "volume-max"
        | "subtitle-font-size"
        | "sub-margin-y"
        | "noise-model"
        | "target-peak"
        | "target-contrast"
        | "cache-secs"
        | "screenshot-jpeg-quality"
        | "screenshot-png-compression"
        | "icc-intent"
        | "ui-controls-radius"
        | "ui-controls-margin-bottom"
        | "ui-controls-margin-side"
        | "ui-controls-padding-top"
        | "ui-controls-padding-bottom"
        | "ui-controls-padding-left"
        | "ui-controls-padding-right"
        | "ui-button-size"
        | "ui-button-radius"
        | "ui-window-radius"
        | "audio-volume"
        | "sleep-timer-minutes" => value.parse::<i32>().is_ok(),
        "tv-resolution" => matches!(value, "0" | "720" | "1080" | "1440" | "2160"),
        "volume"
        | "sub-border-size"
        | "sub-shadow-offset"
        | "sub-gauss"
        | "noise-strength"
        | "hdr-contrast-recovery"
        | "shader-fsr-sharpness"
        | "shader-cas-sharpening"
        | "demuxer-readahead-secs"
        | "ui-bg-opacity"
        | "ui-menu-opacity"
        | "replaygain-preamp" => value.parse::<f64>().is_ok_and(f64::is_finite),
        _ => true,
    }
}
