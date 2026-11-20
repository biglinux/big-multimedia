//! Core playback properties: volume, speed, position, tracks, loop, cache,
//! screenshots, chapters and generic accessors.

use super::MpvBackend;

impl MpvBackend {
    // The event thread applies mixer requests. Getters read the cached or
    // requested state and are safe to use from GTK handlers.
    pub fn set_volume(&self, volume: f64) {
        self.stream_volume.set_volume(volume);
        self.wake_event_loop();
    }

    pub fn get_volume(&self) -> f64 {
        self.stream_volume.snapshot().0
    }

    pub fn set_mute(&self, mute: bool) {
        self.stream_volume.set_mute(mute);
        self.wake_event_loop();
    }

    pub fn get_mute(&self) -> bool {
        self.stream_volume.snapshot().1
    }

    pub fn toggle_mute(&self) {
        self.set_mute(!self.get_mute());
    }

    pub fn get_position(&self) -> f64 {
        self.with_mpv(|m| m.get_property::<f64>("time-pos").unwrap_or(0.0))
    }

    pub fn get_duration(&self) -> f64 {
        self.with_mpv(|m| m.get_property::<f64>("duration").unwrap_or(0.0))
    }

    pub fn get_media_title(&self) -> Option<String> {
        self.with_mpv(|m| m.get_property::<String>("media-title").ok())
    }

    pub fn get_current_file(&self) -> Option<String> {
        self.with_mpv(|m| m.get_property::<String>("path").ok())
    }

    pub fn is_paused(&self) -> bool {
        self.with_mpv(|m| m.get_property::<bool>("pause").unwrap_or(true))
    }

    pub fn set_speed(&self, speed: f64) {
        if speed.is_finite() && (0.25..=4.0).contains(&speed) {
            let _ = self.queue_command("set", &["speed", &speed.to_string()]);
        }
    }

    pub fn is_seekable(&self) -> bool {
        self.with_mpv(|m| m.get_property::<bool>("seekable").unwrap_or(false))
    }

    /// Change the speed by `delta` within `min..=max`, reading and setting it
    /// in one go: a held key adds up, where reading back a queued set would
    /// repeat the same step.
    pub fn step_speed(&self, delta: f64, min: f64, max: f64) {
        self.with_mpv(|m| {
            let current = m.get_property::<f64>("speed").unwrap_or(1.0);
            m.set_property_logged("speed", (current + delta).clamp(min, max));
        });
    }

    pub fn get_speed(&self) -> f64 {
        self.with_mpv(|m| m.get_property::<f64>("speed").unwrap_or(1.0))
    }

    pub fn set_audio_track(&self, id: i64) {
        self.with_mpv(|m| {
            m.set_property_logged("aid", id);
        });
    }

    pub fn audio_delay(&self) -> f64 {
        self.get_property_f64("audio-delay")
    }

    pub fn set_audio_delay(&self, delay: f64) {
        self.with_mpv(|m| {
            m.set_property_logged("audio-delay", delay);
        });
    }

    pub fn set_audio_pitch_correction(&self, enable: bool) {
        self.with_mpv(|m| {
            m.set_property_logged("audio-pitch-correction", enable);
        });
    }

    pub fn set_volume_max(&self, max: i64) {
        self.with_mpv(|m| {
            m.set_property_logged("volume-max", max);
        });
        self.stream_volume.set_volume_max(max as f64);
        self.wake_event_loop();
    }

    /// Empty is Automatic, mpv's own `auto-safe`.
    pub fn set_audio_channels(&self, channels: &str) {
        let channels = if channels.is_empty() {
            "auto-safe"
        } else {
            channels
        };
        self.with_mpv(|m| {
            m.set_property_logged("audio-channels", channels.to_string());
        });
    }

    /// Send compressed surround formats to the receiver untouched, or decode
    /// them here again.
    pub fn toggle_audio_passthrough(&self) {
        const PASSTHROUGH_CODECS: &str = "ac3,eac3,dts,dts-hd,truehd";
        self.with_mpv(|m| {
            let on = m
                .get_property::<String>("audio-spdif")
                .unwrap_or_default()
                .is_empty();
            let codecs = if on { PASSTHROUGH_CODECS } else { "" };
            m.set_property_logged("audio-spdif", codecs.to_string());
        });
    }

    /// Whether playback stopped at the end of the file (`keep-open`).
    pub fn eof_reached(&self) -> bool {
        self.get_property_bool("eof-reached")
    }

    pub(crate) fn get_property_f64(&self, name: &str) -> f64 {
        self.with_mpv(|m| m.get_property(name).unwrap_or(0.0))
    }

    #[cfg(feature = "video")]
    pub(crate) fn get_property_string(&self, name: &str) -> String {
        self.with_mpv(|m| m.get_property::<String>(name).unwrap_or_default())
    }

    pub(crate) fn get_property_bool(&self, name: &str) -> bool {
        self.with_mpv(|m| {
            m.get_property::<String>(name)
                .is_ok_and(|v| v == "yes" || v == "true" || v == "1")
        })
    }

    /// Apply the `loop-mode` setting: `"file"`, `"playlist"`, anything else is off.
    pub fn set_loop_mode(&self, mode: &str) {
        let (file, playlist) = match mode {
            "file" => ("inf", "no"),
            "playlist" => ("no", "inf"),
            _ => ("no", "no"),
        };
        self.with_mpv(|m| {
            m.set_property_logged("loop-file", file.to_string());
            m.set_property_logged("loop-playlist", playlist.to_string());
        });
    }

    /// Both halves of the preference: saving on quit alone would keep
    /// resuming files saved while it was on.
    pub fn set_save_position(&self, enable: bool) {
        self.with_mpv(|m| {
            m.set_property_logged("save-position-on-quit", enable);
            m.set_property_logged("resume-playback", enable);
        });
    }

    pub fn set_screenshot_png_compression(&self, level: i64) {
        self.with_mpv(|m| {
            m.set_property_logged("screenshot-png-compression", level);
        });
    }

    pub fn set_screenshot_tag_colorspace(&self, enable: bool) {
        self.with_mpv(|m| {
            m.set_property_logged("screenshot-tag-colorspace", enable);
        });
    }

    pub fn set_screenshot_high_bit_depth(&self, enable: bool) {
        self.with_mpv(|m| {
            m.set_property_logged("screenshot-high-bit-depth", enable);
        });
    }

    pub fn set_keep_open(&self, mode: &str) {
        self.with_mpv(|m| {
            m.set_property_logged("keep-open", mode.to_string());
        });
    }

    pub fn set_keep_open_pause(&self, enable: bool) {
        self.with_mpv(|m| {
            m.set_property_logged("keep-open-pause", if enable { "yes" } else { "no" });
        });
    }

    pub fn set_idle(&self, mode: &str) {
        self.with_mpv(|m| {
            m.set_property_logged("idle", mode.to_string());
        });
    }

    pub fn set_cache(&self, enable: bool) {
        self.with_mpv(|m| {
            m.set_property_logged("cache", if enable { "yes" } else { "no" });
        });
    }

    pub fn set_cache_secs(&self, secs: i64) {
        self.with_mpv(|m| {
            m.set_property_logged("cache-secs", secs);
        });
    }

    pub fn set_audio_cache_profile(&self, profile: super::types::AudioCacheProfile) {
        let _ = self.queue_command("set", &["demuxer-max-bytes", profile.forward_bytes()]);
        let _ = self.queue_command("set", &["demuxer-max-back-bytes", profile.back_bytes()]);
    }

    pub fn set_cache_pause(&self, enable: bool) {
        self.with_mpv(|m| {
            m.set_property_logged("cache-pause", enable);
        });
    }

    pub fn set_cache_on_disk(&self, enable: bool) {
        self.with_mpv(|m| {
            m.set_property_logged("cache-on-disk", if enable { "yes" } else { "no" });
        });
    }

    pub fn set_demuxer_readahead_secs(&self, secs: f64) {
        self.with_mpv(|m| {
            m.set_property_logged("demuxer-readahead-secs", secs);
        });
    }

    pub fn get_chapter_count(&self) -> i64 {
        self.with_mpv(|m| m.get_property::<i64>("chapters").unwrap_or(0))
    }

    pub fn get_current_chapter(&self) -> i64 {
        self.with_mpv(|m| m.get_property::<i64>("chapter").unwrap_or(-1))
    }

    pub fn get_cache_buffering_state(&self) -> i64 {
        self.with_mpv(|m| m.get_property::<i64>("cache-buffering-state").unwrap_or(-1))
    }

    pub fn get_video_bitrate(&self) -> f64 {
        self.with_mpv(|m| m.get_property::<f64>("video-bitrate").unwrap_or(0.0))
    }

    pub fn get_audio_bitrate(&self) -> f64 {
        self.with_mpv(|m| m.get_property::<f64>("audio-bitrate").unwrap_or(0.0))
    }

    pub fn get_file_format(&self) -> String {
        self.with_mpv(|m| m.get_property::<String>("file-format").unwrap_or_default())
    }

    pub fn get_audio_codec_name(&self) -> String {
        self.with_mpv(|m| {
            m.get_property::<String>("audio-codec-name")
                .unwrap_or_default()
        })
    }

    pub fn get_video_codec(&self) -> String {
        self.with_mpv(|m| m.get_property::<String>("video-codec").unwrap_or_default())
    }

    pub fn set_preferred_audio_lang(&self, lang: &str) {
        self.with_mpv(|m| {
            m.set_property_logged("alang", lang.to_string());
        });
    }
}
