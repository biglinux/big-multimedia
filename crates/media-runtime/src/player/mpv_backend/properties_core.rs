//! Core playback properties: volume, speed, position, tracks, loop, cache,
//! screenshots, editions, chapters, generic accessors, IPC.

use super::MpvBackend;

impl MpvBackend {
    // Apply per-stream mixer requests on the existing mpv event thread.
    // Getters read cached/desired state and are safe to use from GTK handlers.
    pub fn set_volume(&self, volume: f64) {
        self.stream_volume.set_volume(volume);
    }

    pub fn get_volume(&self) -> f64 {
        self.stream_volume.snapshot().0
    }

    pub fn set_mute(&self, mute: bool) {
        self.stream_volume.set_mute(mute);
    }

    pub fn get_mute(&self) -> bool {
        self.stream_volume.snapshot().1
    }

    pub fn toggle_mute(&self) {
        self.stream_volume.set_mute(!self.get_mute());
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
        self.with_mpv(|m| Self::get_bool_property(m, "pause").unwrap_or(true))
    }

    pub fn set_speed(&self, speed: f64) {
        if speed.is_finite() && (0.25..=4.0).contains(&speed) {
            let _ = self.queue_command("set", &["speed", &speed.to_string()]);
        }
    }

    pub fn is_seekable(&self) -> bool {
        self.with_mpv(|m| Self::get_bool_property(m, "seekable").unwrap_or(false))
    }

    pub fn get_speed(&self) -> f64 {
        self.with_mpv(|m| m.get_property::<f64>("speed").unwrap_or(1.0))
    }

    pub fn set_audio_track(&self, id: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("aid", id);
        });
    }

    pub fn set_audio_delay(&self, delay: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("audio-delay", delay);
        });
    }

    pub fn set_audio_pitch_correction(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("audio-pitch-correction", enable);
        });
    }

    pub fn set_volume_max(&self, max: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("volume-max", max);
        });
    }

    pub fn set_audio_channels(&self, channels: &str) {
        if !channels.is_empty() {
            self.with_mpv(|m| {
                let _ = m.set_property("audio-channels", channels.to_string());
            });
        }
    }

    pub fn get_property_f64(&self, name: &str) -> f64 {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property(name).unwrap_or(0.0)
    }

    pub fn get_property_string(&self, name: &str) -> String {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property::<String>(name).unwrap_or_default()
    }

    pub fn get_property_bool(&self, name: &str) -> bool {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property::<String>(name)
            .is_ok_and(|v| v == "yes" || v == "true" || v == "1")
    }

    /// Apply the `loop-mode` setting: `"file"`, `"playlist"`, anything else is off.
    pub fn set_loop_mode(&self, mode: &str) {
        let (file, playlist) = match mode {
            "file" => ("inf", "no"),
            "playlist" => ("no", "inf"),
            _ => ("no", "no"),
        };
        self.with_mpv(|m| {
            let _ = m.set_property("loop-file", file.to_string());
            let _ = m.set_property("loop-playlist", playlist.to_string());
        });
    }

    /// Both halves of the preference: saving on quit alone would keep
    /// resuming files saved while it was on.
    pub fn set_save_position(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("save-position-on-quit", enable);
            let _ = m.set_property("resume-playback", enable);
        });
    }

    pub fn set_screenshot_format(&self, fmt: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("screenshot-format", fmt.to_string());
        });
    }

    pub fn set_screenshot_jpeg_quality(&self, quality: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("screenshot-jpeg-quality", quality);
        });
    }

    pub fn set_screenshot_png_compression(&self, level: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("screenshot-png-compression", level);
        });
    }

    pub fn set_screenshot_template(&self, tmpl: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("screenshot-template", tmpl.to_string());
        });
    }

    pub fn set_screenshot_directory(&self, dir: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("screenshot-directory", dir.to_string());
        });
    }

    pub fn set_screenshot_tag_colorspace(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("screenshot-tag-colorspace", enable);
        });
    }

    pub fn set_screenshot_high_bit_depth(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("screenshot-high-bit-depth", enable);
        });
    }

    pub fn set_keep_open(&self, mode: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("keep-open", mode.to_string());
        });
    }

    pub fn set_keep_open_pause(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property(
                "keep-open-pause",
                if enable {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
            );
        });
    }

    pub fn set_idle(&self, mode: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("idle", mode.to_string());
        });
    }

    pub fn set_cache(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property(
                "cache",
                if enable {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
            );
        });
    }

    pub fn set_cache_secs(&self, secs: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("cache-secs", secs);
        });
    }

    /// Queue a cache policy, not a measured memory-saving guarantee. Confirm
    /// behavior and total process/GPU memory on the actual decoder and media.
    pub fn set_audio_cache_profile(&self, profile: super::types::AudioCacheProfile) {
        let _ = self.queue_command("set", &["demuxer-max-bytes", profile.forward_bytes()]);
        let _ = self.queue_command("set", &["demuxer-max-back-bytes", profile.back_bytes()]);
    }

    pub fn set_cache_pause(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("cache-pause", enable);
        });
    }

    pub fn set_cache_on_disk(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property(
                "cache-on-disk",
                if enable {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
            );
        });
    }

    pub fn set_demuxer_readahead_secs(&self, secs: f64) {
        self.with_mpv(|m| {
            let _ = m.set_property("demuxer-readahead-secs", secs);
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
            let _ = m.set_property("alang", lang.to_string());
        });
    }
}
