//! Core playback properties: volume, speed, position, tracks, loop, cache,
//! screenshots, editions, chapters, generic accessors, IPC.

use crate::i18n::i18n;

use super::MpvBackend;

impl MpvBackend {
    // Volume/mute drive our PipeWire/PulseAudio sink-input directly (via `pactl`),
    // NOT an mpv property. mpv's software `volume` filter never reaches the sound
    // server (mixer applets stay at 100%), and `ao-volume` is capped at 100 and
    // uses a different scale than the mixer. Controlling the stream means the
    // in-app value equals the value the system mixer shows (with >100% boost),
    // and the server remembers it per client name. No-ops while nothing plays.
    pub fn set_volume(&self, volume: f64) {
        super::pactl_volume::set_volume(&self.pa_sink_input, &self.audio_client_name, volume);
    }

    pub fn get_volume(&self) -> f64 {
        super::pactl_volume::get_volume(&self.pa_sink_input, &self.audio_client_name)
            .unwrap_or(100.0)
    }

    pub fn set_mute(&self, mute: bool) {
        super::pactl_volume::set_mute(&self.pa_sink_input, &self.audio_client_name, mute);
    }

    pub fn get_mute(&self) -> bool {
        super::pactl_volume::get_mute(&self.audio_client_name).unwrap_or(false)
    }

    pub fn toggle_mute(&self) {
        let muted = super::pactl_volume::get_mute(&self.audio_client_name).unwrap_or(false);
        super::pactl_volume::set_mute(&self.pa_sink_input, &self.audio_client_name, !muted);
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
        self.with_mpv(|m| {
            let _ = m.set_property("speed", speed);
        });
    }

    pub fn get_speed(&self) -> f64 {
        self.with_mpv(|m| m.get_property::<f64>("speed").unwrap_or(1.0))
    }

    pub fn set_audio_track(&self, id: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("aid", id);
        });
    }

    pub fn add_audio_file(&self, path: &str) {
        self.with_mpv(|m| {
            let _ = m.command("audio-add", &[path, "auto"]);
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

    pub fn set_audio_spdif(&self, codecs: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("audio-spdif", codecs.to_string());
        });
    }

    pub fn get_audio_spdif(&self) -> String {
        self.with_mpv(|m| m.get_property::<String>("audio-spdif").unwrap_or_default())
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

    pub fn get_property_i64(&self, name: &str) -> i64 {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property(name).unwrap_or(0)
    }

    pub fn get_property_bool(&self, name: &str) -> bool {
        let mpv = self
            .mpv
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        mpv.get_property::<String>(name)
            .is_ok_and(|v| v == "yes" || v == "true" || v == "1")
    }

    pub fn set_loop_file(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property(
                "loop-file",
                if enable {
                    "inf".to_string()
                } else {
                    "no".to_string()
                },
            );
        });
    }

    pub fn set_loop_playlist(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property(
                "loop-playlist",
                if enable {
                    "inf".to_string()
                } else {
                    "no".to_string()
                },
            );
        });
    }

    pub fn set_save_position(&self, enable: bool) {
        self.with_mpv(|m| {
            let _ = m.set_property("save-position-on-quit", enable);
        });
    }

    /// Call before close so resume-playback survives the next launch.
    pub fn write_watch_later_config(&self) {
        self.with_mpv(|m| {
            let _ = m.command("write-watch-later-config", &[]);
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

    /// Switch the demuxer cache profile at runtime. Cheap (mpv truncates
    /// in-place); safe to call between tracks. Podcast/audiobook mode
    /// cuts ~120 MB RSS off long-form playback vs the `Music` default.
    pub fn set_audio_cache_profile(&self, profile: super::types::AudioCacheProfile) {
        self.with_mpv(|m| {
            let _ = m.set_property("demuxer-max-bytes", profile.forward_bytes().to_string());
            let _ = m.set_property("demuxer-max-back-bytes", profile.back_bytes().to_string());
            log::info!(
                "audio cache profile -> {:?} ({}+{}, cap {} MiB)",
                profile,
                profile.forward_bytes(),
                profile.back_bytes(),
                profile.total_mib()
            );
        });
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

    pub fn get_video_params_string(&self) -> String {
        self.with_mpv(|m| {
            let mut video_details = Vec::new();
            if let Ok(v) = m.get_property::<String>("video-codec") {
                video_details.push(format!("{}: {v}", i18n("Video Codec")));
            }
            if let Ok(w) = m.get_property::<i64>("width")
                && let Ok(h) = m.get_property::<i64>("height")
            {
                video_details.push(format!("{}: {w}x{h}", i18n("Resolution")));
            }
            if let Ok(fps) = m.get_property::<f64>("estimated-vf-fps") {
                video_details.push(format!("{}: {fps:.2}", i18n("FPS")));
            }
            if let Ok(br) = m.get_property::<f64>("video-bitrate") {
                video_details.push(format!(
                    "{}: {:.0} kbps",
                    i18n("Video Bitrate"),
                    br / 1000.0
                ));
            }
            if let Ok(ac) = m.get_property::<String>("audio-codec-name") {
                video_details.push(format!("{}: {ac}", i18n("Audio Codec")));
            }
            if let Ok(abr) = m.get_property::<f64>("audio-bitrate") {
                video_details.push(format!(
                    "{}: {:.0} kbps",
                    i18n("Audio Bitrate"),
                    abr / 1000.0
                ));
            }
            if let Ok(hwdec) = m.get_property::<String>("hwdec-current") {
                let value = if hwdec.is_empty() {
                    i18n("none (sw)")
                } else {
                    hwdec
                };
                video_details.push(format!("{}: {value}", i18n("HW Decoder")));
            }
            if let Ok(drops) = m.get_property::<i64>("frame-drop-count") {
                video_details.push(format!("{}: {drops}", i18n("Dropped Frames")));
            }
            if let Ok(colorspace) = m.get_property::<String>("video-params/colormatrix") {
                video_details.push(format!("{}: {colorspace}", i18n("Colorspace")));
            }
            if let Ok(primaries) = m.get_property::<String>("video-params/primaries") {
                video_details.push(format!("{}: {primaries}", i18n("Primaries")));
            }
            if let Ok(gamma) = m.get_property::<String>("video-params/gamma") {
                if gamma.contains("pq") || gamma.contains("hlg") {
                    video_details.push(format!("{}: {gamma} ", i18n("HDR")));
                } else {
                    video_details.push(format!("{}: {gamma}", i18n("Transfer")));
                }
            }
            video_details.join("\n")
        })
    }

    pub fn get_edition_count(&self) -> i64 {
        self.with_mpv(|m| m.get_property::<i64>("editions").unwrap_or(0))
    }

    pub fn get_current_edition(&self) -> i64 {
        self.with_mpv(|m| m.get_property::<i64>("current-edition").unwrap_or(0))
    }

    pub fn set_edition(&self, id: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("edition", id);
        });
    }

    pub fn set_chapters_file(&self, path: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("chapters-file", path.to_string());
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

    pub fn set_ipc_server(&self, path: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("input-ipc-server", path.to_string());
        });
    }
}
