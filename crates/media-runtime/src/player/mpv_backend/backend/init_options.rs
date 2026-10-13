//! Per-mode mpv `MpvInitializer` option helpers (video, audio-only,
//! playback, watch-later, IPC, custom). Split out of `backend.rs`;
//! behavior is identical. Kept as free functions for testability and
//! to keep `MpvBackend::new` short.

use log::error;

use super::client::{MpvError, MpvInitializer};

/// PulseAudio/PipeWire client name reported to the sound server. mpv's default
/// ("mpv") is what audio mixers show and what `module-stream-restore` keys the
/// remembered per-application volume on. Use the stable program name so the
/// stream is attributed to this player and its volume memory survives restarts.
pub(super) fn audio_client_name() -> String {
    gtk::glib::prgname()
        .map(|name| name.to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "big-media-player".to_string())
}

/// Parse `"key=value,key2=value2"` into owned pairs. `""` values accept bare
/// keys (`"cache"` → `("cache", "yes")`). Empty segments are dropped.
pub(super) fn parse_custom_options(custom: &str) -> Vec<(String, String)> {
    custom
        .split(',')
        .filter_map(|opt| {
            let opt = opt.trim();
            if opt.is_empty() {
                return None;
            }
            let mut parts = opt.splitn(2, '=');
            let key = parts.next()?.trim().to_string();
            let value = parts.next().unwrap_or("yes").trim().to_string();
            Some((key, value))
        })
        .collect()
}

/// mpv starts a Lua VM and thread for each of its built-in scripts. The players
/// drive mpv through their own UI (no OSD, no default bindings), so the stats
/// overlay, console, select menu, commands, positioning, context menu and
/// auto-profiles are never reachable. Measured: 6 MiB less anonymous memory in
/// the video player. `ytdl` stays on: it plays web video URLs. A mpv without one
/// of these options just keeps it.
pub(super) fn disable_unused_builtin_scripts(init: &MpvInitializer) {
    for option in [
        "load-stats-overlay",
        "load-console",
        "load-select",
        "load-commands",
        "load-positioning",
        "load-context-menu",
        "load-auto-profiles",
    ] {
        if let Err(error) = init.set_option(option, "no") {
            log::debug!("mpv has no {option}: {error}");
        }
    }
}

pub(super) fn apply_video_options(init: &MpvInitializer) -> Result<(), MpvError> {
    init.set_option("vo", "libmpv")?;
    init.set_option("hwdec", "auto-safe")?;
    init.set_option("gpu-hwdec-interop", "auto")?;
    init.set_option("dither-depth", "no")?;
    init.set_option("correct-downscaling", "no")?;
    init.set_option("sigmoid-upscaling", "no")?;
    init.set_option("linear-downscaling", "no")?;
    init.set_option("hdr-compute-peak", "no")?;
    init.set_option("scale", "bilinear")?;
    init.set_option("dscale", "bilinear")?;
    init.set_option("fbo-format", "rgba8")?;
    init.set_option("video-sync", "audio")?;
    // The render API is called with BLOCK_FOR_TARGET_TIME=0, so mpv must not
    // render ahead of the target display time: without this the frames it hands
    // us are early and A/V sync is slightly off (render.h, MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME).
    init.set_option("video-timing-offset", "0")?;
    init.set_option("interpolation", "no")?;
    init.set_option("vd-lavc-dr", "auto")?;
    init.set_option("hwdec-extra-frames", "2")?;
    Ok(())
}

/// Empty `audio_output` leaves mpv's auto-detection in place.
pub(super) fn apply_audio_output(init: &MpvInitializer, audio_output: &str) {
    if audio_output.is_empty() {
        return;
    }
    if let Err(error) = init.set_option("ao", audio_output.to_string()) {
        error!("Failed to set audio output '{audio_output}': {error}");
    }
}

pub(super) fn apply_playback_options(init: &MpvInitializer) -> Result<(), MpvError> {
    disable_unused_builtin_scripts(init);
    init.set_option("keep-open", "yes")?;
    init.set_option("keep-open-pause", "no")?;
    init.set_option("idle", "yes")?;
    init.set_option("input-default-bindings", "no")?;
    init.set_option("input-vo-keyboard", "no")?;
    init.set_option("osc", "no")?;
    init.set_option("osd-level", "0")?;
    init.set_option("sub-auto", "fuzzy")?;
    init.set_option("sub-file-paths", "Subs:subs:Subtitles:subtitles:Sub:sub")?;
    init.set_option("demuxer-max-bytes", "10MiB")?;
    init.set_option("demuxer-max-back-bytes", "4MiB")?;
    init.set_option("framedrop", "vo")?;
    init.set_option("vd-lavc-skiploopfilter", "nonref")?;
    init.set_option("volume-max", "150")?;
    init.set_option("audio-client-name", audio_client_name())?;
    init.set_option("screenshot-format", "png")?;
    init.set_option("screenshot-template", "%F-%p")?;
    Ok(())
}

/// Audio-only variant: no video output. Defaults to [`AudioCacheProfile::Music`]
/// (gapless transitions covered with a 30 MiB lookahead). Callers can switch
/// profile post-init via [`super::MpvBackend::set_audio_cache_profile`]:
/// podcast/audiobook playback drops the demuxer cache to ~10 MiB total, and
/// network radio bumps the forward window for jitter resilience.
pub(super) fn apply_audio_only_options(init: &MpvInitializer) -> Result<(), MpvError> {
    disable_unused_builtin_scripts(init);
    init.set_option("vo", "null")?;
    init.set_option("video", "no")?;
    init.set_option("hwdec", "no")?;
    init.set_option("keep-open", "yes")?;
    init.set_option("keep-open-pause", "no")?;
    init.set_option("idle", "yes")?;
    init.set_option("input-default-bindings", "no")?;
    init.set_option("input-vo-keyboard", "no")?;
    init.set_option("osc", "no")?;
    init.set_option("osd-level", "0")?;
    init.set_option("volume-max", "150")?;
    init.set_option("audio-client-name", audio_client_name())?;
    init.set_option("gapless-audio", "yes")?;
    let p = crate::player::mpv_backend::types::AudioCacheProfile::Music;
    init.set_option("demuxer-max-bytes", p.forward_bytes())?;
    init.set_option("demuxer-max-back-bytes", p.back_bytes())?;
    Ok(())
}

/// What mpv restores per file. Filters, speed, volume and picture settings
/// belong to the apps' own settings: restored behind their back, the UI
/// would disagree with what mpv plays.
pub(super) const VIDEO_WATCH_LATER_OPTIONS: &str = "start,aid,sid,sub-delay,audio-delay";
pub(super) const AUDIO_WATCH_LATER_OPTIONS: &str = "start";

/// Persist mpv's per-file resume state under XDG data. `subdir` is the
/// path below `$XDG_DATA_HOME` (e.g. `"big-video-player/watch_later"`).
pub(super) fn apply_watch_later(
    init: &MpvInitializer,
    subdir: &str,
    options: &str,
) -> Result<(), MpvError> {
    if let Some(data_dir) = gtk::glib::user_data_dir().to_str() {
        let watch_dir = format!("{data_dir}/{subdir}");
        let _ = std::fs::create_dir_all(&watch_dir);
        init.set_option("watch-later-directory", watch_dir)?;
    }
    init.set_option("watch-later-options", options)
}

/// Best-effort IPC socket under `$XDG_RUNTIME_DIR`. Failures are non-fatal.
pub(super) fn apply_input_ipc(init: &MpvInitializer, sock_name: &str) {
    if let Some(runtime_dir) = gtk::glib::user_runtime_dir().to_str() {
        let sock = format!("{runtime_dir}/{sock_name}");
        let _ = init.set_option("input-ipc-server", sock);
    }
}

/// Apply user-supplied `key=value` pairs; individual failures are logged
/// but don't abort initialisation.
pub(super) fn apply_custom_options(init: &MpvInitializer, opts: &[(String, String)]) {
    for (key, value) in opts {
        if let Err(error) = init.set_option(key, value.clone()) {
            error!("Custom mpv option '{key}={value}' failed: {error}");
        }
    }
}
