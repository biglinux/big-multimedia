use std::collections::HashMap;

pub(super) fn defaults() -> HashMap<String, String> {
    let mut values = HashMap::new();

    values.insert("advanced-mode".into(), "false".into());

    values.insert("prefs-window-width".into(), "960".into());
    values.insert("prefs-window-height".into(), "700".into());
    values.insert("appearance-window-width".into(), "600".into());
    values.insert("appearance-window-height".into(), "700".into());
    values.insert("audio-prefs-window-width".into(), "600".into());
    values.insert("audio-prefs-window-height".into(), "500".into());
    values.insert("audio-appearance-window-width".into(), "500".into());
    values.insert("audio-appearance-window-height".into(), "400".into());

    values.insert("window-width".into(), "960".into());
    values.insert("window-height".into(), "540".into());
    values.insert("window-maximized".into(), "false".into());
    values.insert("volume".into(), "1.0".into());
    values.insert("mute".into(), "false".into());
    values.insert("open-new-windows".into(), "false".into());

    values.insert("last-profile".into(), String::new());
    values.insert("output-dir".into(), String::new());
    values.insert("gpu-device".into(), String::new());
    values.insert("gpu-auto-migrated".into(), "false".into());
    values.insert("overwrite-existing".into(), "false".into());
    values.insert("parallel-jobs".into(), "1".into());
    values.insert("notify-done".into(), "true".into());
    values.insert("delete-source".into(), "false".into());
    values.insert("prevent-sleep".into(), "true".into());
    values.insert("shutdown-after".into(), "false".into());
    values.insert("saved-profile".into(), String::new());
    values.insert("conv-last-dir".into(), String::new());
    values.insert("conv-segment-mode".into(), "split".into());
    values.insert("conv-nr-enabled".into(), "false".into());
    values.insert("conv-nr-lookahead".into(), "0".into());
    values.insert("conv-nr-strength".into(), "0.0".into());
    values.insert("conv-nr-model".into(), "0".into());
    values.insert("conv-nr-voice-recovery".into(), "0.0".into());
    values.insert("conv-gate-enabled".into(), "false".into());
    values.insert("conv-gate-intensity".into(), "0.0".into());
    values.insert("conv-hpf-enabled".into(), "false".into());
    values.insert("conv-hpf-freq".into(), "0".into());
    values.insert("conv-normalize".into(), "false".into());
    values.insert("conv-compressor-enabled".into(), "false".into());
    values.insert("conv-compressor-intensity".into(), "0.0".into());
    values.insert("conv-eq-enabled".into(), "false".into());
    values.insert("conv-eq-preset".into(), String::new());
    values.insert("conv-eq-bands".into(), String::new());
    values.insert("audio-conv-format".into(), String::new());
    values.insert("audio-conv-bitrate".into(), String::new());
    values.insert("audio-conv-channels".into(), "0".into());
    values.insert("audio-conv-cut-mode".into(), "0".into());
    values.insert("audio-conv-cut-output".into(), "0".into());
    values.insert("audio-conv-volume".into(), "0.0".into());
    values.insert("audio-conv-speed".into(), "0.0".into());

    values.insert("hwdec".into(), "auto-safe".into());
    values.insert("deinterlace".into(), "false".into());

    values.insert("audio-output".into(), String::new());
    values.insert("audio-normalize".into(), "false".into());
    values.insert("audio-pitch-correction".into(), "true".into());
    values.insert("volume-max".into(), "150".into());
    values.insert("preferred-audio-lang".into(), String::new());
    values.insert("audio-channels".into(), String::new());

    values.insert("sub-font".into(), String::new());
    values.insert("subtitle-font-size".into(), "36".into());
    values.insert("sub-bold".into(), "false".into());
    values.insert("sub-color".into(), "#FFFFFFFF".into());
    values.insert("sub-border-size".into(), "2.5".into());
    values.insert("sub-border-color".into(), "#FF000000".into());
    values.insert("sub-shadow-color".into(), "#80000000".into());
    values.insert("sub-shadow-offset".into(), "0.0".into());
    values.insert("sub-margin-y".into(), "22".into());
    values.insert("preferred-sub-lang".into(), String::new());
    values.insert("sub-auto".into(), "fuzzy".into());

    values.insert("resume-playback".into(), "true".into());
    values.insert("loop-mode".into(), "no".into());

    values.insert("custom-mpv-options".into(), String::new());
    values.insert("gpu-dumb-mode".into(), "false".into());
    values.insert("noise-strength".into(), "0.0".into());
    values.insert("noise-gate-threshold".into(), "0".into());
    values.insert("noise-model".into(), "0".into());
    values.insert("noise-voice-recovery".into(), "0.75".into());
    values.insert("noise-model-blend".into(), "false".into());
    values.insert("eq-bands".into(), String::new());
    values.insert("eq-preset".into(), "flat".into());

    values.insert("screenshot-format".into(), "png".into());
    values.insert("screenshot-jpeg-quality".into(), "90".into());
    values.insert("screenshot-png-compression".into(), "7".into());
    values.insert("screenshot-template".into(), "%F-%p".into());
    values.insert("screenshot-directory".into(), String::new());
    values.insert("screenshot-tag-colorspace".into(), "true".into());
    values.insert("screenshot-high-bit-depth".into(), "true".into());

    values.insert("keep-open".into(), "yes".into());
    values.insert("keep-open-pause".into(), "false".into());
    values.insert("idle-mode".into(), "yes".into());
    values.insert("render-profile".into(), "default".into());

    values.insert("video-sync".into(), "audio".into());
    values.insert("interpolation".into(), "false".into());
    values.insert("tscale".into(), "mitchell".into());

    values.insert("tone-mapping".into(), "auto".into());
    values.insert("target-peak".into(), "0".into());
    values.insert("target-contrast".into(), "0".into());
    values.insert("hdr-contrast-recovery".into(), "0.0".into());
    values.insert("hdr-compute-peak".into(), "false".into());
    values.insert("target-colorspace-hint".into(), "false".into());
    values.insert("gamut-mapping-mode".into(), "auto".into());

    values.insert("glsl-shaders".into(), String::new());
    values.insert("builtin-upscaler".into(), String::new());
    values.insert("builtin-sharpener".into(), String::new());
    values.insert("neural-upscaler".into(), String::new());
    values.insert("anime4k-preset".into(), String::new());

    values.insert("cache".into(), "false".into());
    values.insert("cache-secs".into(), "30".into());
    values.insert("cache-pause".into(), "true".into());
    values.insert("cache-on-disk".into(), "false".into());
    values.insert("demuxer-readahead-secs".into(), "1.0".into());

    values.insert("sub-border-style".into(), "outline-and-shadow".into());
    values.insert("sub-scale-with-window".into(), "true".into());
    values.insert("sub-ass-scale-with-window".into(), "false".into());

    values.insert("gpu-api".into(), String::new());
    values.insert("gpu-context".into(), String::new());

    values.insert("icc-profile".into(), String::new());
    values.insert("icc-profile-auto".into(), "false".into());
    values.insert("icc-intent".into(), "0".into());

    values.insert("sub-filter-sdh".into(), "false".into());
    values.insert("sub-filter-sdh-harder".into(), "false".into());
    values.insert("stop-screensaver".into(), "yes".into());
    values.insert("sub-gauss".into(), "0.0".into());
    values.insert("sub-gray".into(), "false".into());
    values.insert("secondary-subtitles".into(), "false".into());
    values.insert("auto-window-resize".into(), "true".into());
    values.insert("seekbar-thumbnail".into(), "true".into());
    values.insert("custom-keybindings".into(), String::new());

    values.insert("playlist-view".into(), "list".into());
    values.insert("playlist-folder-recursive".into(), "false".into());
    values.insert("playlist-sort".into(), "name".into());
    values.insert("playlist-sort-desc".into(), "false".into());
    values.insert("playlist-last-folder".into(), String::new());
    values.insert("playlist-shuffle".into(), "false".into());

    values.insert("ui-color-source".into(), "gtk-default".into());
    values.insert("ui-bg-color".into(), "#2A2A30".into());
    values.insert("ui-bg-opacity".into(), "0.90".into());
    values.insert("ui-menu-opacity".into(), "0.95".into());
    values.insert("ui-osd-shadow".into(), "false".into());
    values.insert("ui-font-color".into(), "#FFFFFF".into());
    values.insert("ui-icon-color".into(), "#FFFFFF".into());
    values.insert("ui-dialog-color".into(), "#2A2A30".into());

    values.insert("audio-ui-color-source".into(), "gtk-default".into());
    values.insert("audio-ui-bg-color".into(), "#2A2A30".into());
    values.insert("audio-ui-font-color".into(), "#FFFFFF".into());
    values.insert("audio-ui-icon-color".into(), "#FFFFFF".into());
    values.insert("audio-ui-dialog-color".into(), "#2A2A30".into());
    values.insert("ui-controls-radius".into(), "0".into());
    values.insert("ui-controls-margin-bottom".into(), "0".into());
    values.insert("ui-controls-margin-side".into(), "0".into());
    values.insert("ui-controls-padding-top".into(), "0".into());
    values.insert("ui-controls-padding-bottom".into(), "0".into());
    values.insert("ui-controls-padding-left".into(), "0".into());
    values.insert("ui-controls-padding-right".into(), "0".into());
    values.insert("ui-button-custom".into(), "false".into());
    values.insert("ui-button-size".into(), "36".into());
    values.insert("ui-button-radius".into(), "18".into());
    values.insert("ui-window-radius-custom".into(), "false".into());
    values.insert("ui-window-radius".into(), "12".into());
    values.insert("ui-window-btn-custom-size".into(), "false".into());
    values.insert("ui-show-volume".into(), "true".into());
    values.insert("ui-show-speed".into(), "true".into());
    values.insert("ui-show-voice".into(), "true".into());
    values.insert("ui-show-play".into(), "true".into());
    values.insert("ui-show-prev".into(), "true".into());
    values.insert("ui-show-next".into(), "true".into());
    values.insert("ui-show-subtitle".into(), "true".into());
    values.insert("ui-show-audio-track".into(), "true".into());
    values.insert("ui-show-fullscreen".into(), "true".into());
    values.insert("ui-show-pin".into(), "true".into());
    values.insert("ui-show-open".into(), "true".into());
    values.insert("ui-show-playlist".into(), "true".into());
    values.insert("ui-show-menu".into(), "true".into());
    values.insert("ui-zone-top-left".into(), "pin,open".into());
    values.insert("ui-zone-top-right".into(), "menu,playlist".into());
    values.insert("ui-zone-bottom-left".into(), "volume,speed,voice".into());
    values.insert("ui-zone-bottom-center".into(), "prev,play,next".into());
    values.insert(
        "ui-zone-bottom-right".into(),
        "subtitle,audio-track,fullscreen".into(),
    );

    values.insert("replaygain".into(), "track".into());
    values.insert("replaygain-preamp".into(), "0.0".into());
    values.insert("gapless-audio".into(), "false".into());
    values.insert("crossfade-secs".into(), "0".into());
    values.insert("audio-volume".into(), "100".into());
    values.insert("taskbar-progress".into(), "true".into());

    values.insert("browser-pane-width".into(), "400".into());
    values.insert("browser-mode".into(), "0".into());
    values.insert("browser-visible".into(), "true".into());
    values.insert("audio-bg-blur".into(), "false".into());
    values.insert("audio-high-contrast".into(), "false".into());

    values.insert("sleep-timer-action".into(), "pause".into());
    values.insert("sleep-timer-minutes".into(), "30".into());

    values.insert("radio-api-servers".into(), "de2.api.radio-browser.info,nl1.api.radio-browser.info,at1.api.radio-browser.info,de1.api.radio-browser.info".into());

    values.insert("queue-current-index".into(), "0".into());
    values.insert("podcast-active-feed".into(), String::new());
    values.insert("files-last-dir".into(), String::new());

    values
}
