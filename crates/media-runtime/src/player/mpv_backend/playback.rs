//! Playback control: load, play, pause, seek, playlist, chapters, stream record.

use std::ffi::CString;
use std::sync::atomic::Ordering;

use log::{error, info};

use super::MpvBackend;
use super::types::AudioCacheProfile;

/// Fallback audio policy only. Panels with known intent override this heuristic.
#[must_use]
pub fn profile_for_uri(target: &str) -> Option<AudioCacheProfile> {
    let scheme = big_os_kit::url::uri_scheme(target)?;
    if !["http", "https", "rtsp", "rtmp", "mms"].iter().any(|s| scheme.eq_ignore_ascii_case(s)) { return None; }
    let path = big_os_kit::url::uri_path(target);
    let extension = path.rsplit('/').next().unwrap_or("").rsplit_once('.').map(|(_, e)| e);
    Some(if extension.is_some_and(|e| ["mp3", "m4a", "ogg", "opus", "flac"].iter().any(|v| e.eq_ignore_ascii_case(v))) {
        AudioCacheProfile::Podcast
    } else { AudioCacheProfile::Radio })
}

impl MpvBackend {
    pub fn load_file_with_audio_profile(&self, path: &str, profile: AudioCacheProfile, from_start: bool) {
        if self.audio_only { self.set_audio_cache_profile(profile); }
        self.load_inner(path, from_start);
    }

    pub fn load_file(&self, path: &str) {
        if self.audio_only {
            self.set_audio_cache_profile(profile_for_uri(path).unwrap_or(AudioCacheProfile::Music));
        }
        self.load_inner(path, false);
    }

    /// Bypass watch-later for an explicit new queue occurrence.
    pub fn load_file_from_start(&self, path: &str) {
        if self.audio_only {
            self.set_audio_cache_profile(profile_for_uri(path).unwrap_or(AudioCacheProfile::Music));
        }
        self.load_inner(path, true);
    }

    fn load_inner(&self, path: &str, from_start: bool) {
        self.loading.store(true, Ordering::Relaxed);
        let result = if from_start {
            self.with_mpv(|m| m.command("loadfile", &[path, "replace", "-1", "resume-playback=no"]))
        } else { self.with_mpv(|m| m.command("loadfile", &[path, "replace"])) };
        if let Err(error) = result {
            self.loading.store(false, Ordering::Relaxed);
            error!("Failed to load {}: {error}", big_os_kit::url::MediaLogTarget(path));
        }
    }

    pub fn load_file_append(&self, path: &str) {
        if let Err(e) = self.with_mpv(|m| m.command("loadfile", &[path, "append-play"])) {
            error!("Failed to append {}: {e}", big_os_kit::url::MediaLogTarget(path));
        }
    }

    pub fn play(&self) {
        self.with_mpv(|m| {
            let _ = m.set_property("pause", false);
        });
    }

    pub fn pause(&self) {
        self.with_mpv(|m| {
            let _ = m.set_property("pause", true);
        });
    }

    pub fn toggle_pause(&self) {
        self.with_mpv(|m| match Self::get_bool_property(m, "pause") {
            Ok(paused) => {
                info!("toggle_pause: paused={paused}, setting to {}", !paused);
                if let Err(e) = m.set_property("pause", !paused) {
                    error!("toggle_pause: set_property failed: {e}");
                }
            }
            Err(e) => {
                error!("toggle_pause: get_bool_property failed: {e}");
            }
        });
    }

    pub fn stop(&self) {
        self.with_mpv(|m| {
            let _ = m.command("stop", &[]);
        });
    }

    pub fn seek(&self, seconds: f64) {
        self.with_mpv(|m| {
            let _ = m.command("seek", &[&seconds.to_string(), "relative"]);
        });
    }

    pub fn seek_absolute(&self, seconds: f64) {
        self.with_mpv(|m| {
            let _ = m.command("seek", &[&seconds.to_string(), "absolute"]);
        });
    }

    /// Non-blocking exact seek for live seekbar preview.
    /// `mpv_command_async` on the raw handle — no mutex contention.
    pub fn seek_absolute_async(&self, seconds: f64) {
        let Ok(secs_str) = CString::new(seconds.to_string()) else {
            return;
        };
        let cmd = CString::new("seek").expect("static literal");
        let flags = CString::new("absolute").expect("static literal");
        let mut args: Vec<*const std::os::raw::c_char> = vec![
            cmd.as_ptr(),
            secs_str.as_ptr(),
            flags.as_ptr(),
            std::ptr::null(),
        ];
        // SAFETY: `mpv_command_async` is thread-safe on the same handle;
        // raw_mpv_handle outlives this call (held by MpvBackend); CStrings
        // live until after the call returns.
        unsafe {
            mpv_sys::mpv_command_async(
                self.raw_mpv_handle,
                0, // reply_userdata=0 — reply is discarded.
                args.as_mut_ptr(),
            );
        }
    }

    /// Capture a screenshot WITHOUT blocking the GTK main thread. The
    /// synchronous `screenshot-to-file` command deadlocks under the libmpv
    /// render API: it blocks the caller (and holds the client mutex) until mpv's
    /// playloop completes the command, but the playloop needs the main thread to
    /// run the render callback — so the main thread waits on mpv while mpv waits
    /// on the main thread. `mpv_command_async` returns immediately and mpv writes
    /// the file on its own thread. `mode`: `"subtitles"` | `"video"` | `"window"`.
    pub fn screenshot_to_file_async(&self, path: &str, mode: &str) {
        let (Ok(cmd), Ok(path_c), Ok(mode_c)) = (
            CString::new("screenshot-to-file"),
            CString::new(path),
            CString::new(mode),
        ) else {
            return;
        };
        let mut args: Vec<*const std::os::raw::c_char> = vec![
            cmd.as_ptr(),
            path_c.as_ptr(),
            mode_c.as_ptr(),
            std::ptr::null(),
        ];
        // SAFETY: `mpv_command_async` is thread-safe on the same handle; raw_mpv_handle
        // outlives this call (held by MpvBackend); the CStrings live until the
        // call returns (mpv copies the argv strings).
        unsafe {
            mpv_sys::mpv_command_async(
                self.raw_mpv_handle,
                0, // reply_userdata=0 — reply is discarded.
                args.as_mut_ptr(),
            );
        }
    }

    pub fn playlist_next(&self) {
        self.with_mpv(|m| {
            let _ = m.command("playlist-next", &["force"]);
        });
    }

    pub fn playlist_prev(&self) {
        self.with_mpv(|m| {
            let _ = m.command("playlist-prev", &["force"]);
        });
    }

    pub fn playlist_play_index(&self, index: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("playlist-pos", index);
        });
    }

    pub fn playlist_clear(&self) {
        self.with_mpv(|m| {
            let _ = m.command("playlist-clear", &[]);
        });
    }

    pub fn playlist_remove(&self, index: i64) {
        self.with_mpv(|m| {
            let _ = m.command("playlist-remove", &[&index.to_string()]);
        });
    }

    pub fn get_playlist_pos(&self) -> i64 {
        self.with_mpv(|m| m.get_property::<i64>("playlist-pos").unwrap_or(-1))
    }

    pub fn get_playlist_count(&self) -> i64 {
        self.with_mpv(|m| m.get_property::<i64>("playlist-count").unwrap_or(0))
    }

    /// `(dwidth, dheight)` — accounts for SAR/DAR. `None` when no video.
    pub fn get_video_dimensions(&self) -> Option<(i64, i64)> {
        self.with_mpv(|m| {
            let w = m.get_property::<i64>("dwidth").ok()?;
            let h = m.get_property::<i64>("dheight").ok()?;
            if w > 0 && h > 0 { Some((w, h)) } else { None }
        })
    }

    pub fn frame_step(&self) {
        self.with_mpv(|m| {
            let _ = m.command("frame-step", &[]);
        });
    }

    pub fn frame_back_step(&self) {
        self.with_mpv(|m| {
            let _ = m.command("frame-back-step", &[]);
        });
    }

    pub fn revert_seek(&self) {
        self.with_mpv(|m| {
            let _ = m.command("revert-seek", &[]);
        });
    }

    pub fn set_stream_record(&self, path: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("stream-record", path.to_string());
        });
    }

    pub fn get_stream_record(&self) -> String {
        self.with_mpv(|m| {
            m.get_property::<String>("stream-record")
                .unwrap_or_default()
        })
    }

    pub fn chapter_next(&self) {
        self.with_mpv(|m| {
            let _ = m.command("add", &["chapter", "1"]);
        });
    }

    pub fn chapter_prev(&self) {
        self.with_mpv(|m| {
            let _ = m.command("add", &["chapter", "-1"]);
        });
    }

    pub fn set_chapter(&self, idx: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("chapter", idx);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_path_no_profile_override() {
        assert!(profile_for_uri("/home/me/track.mp3").is_none());
        assert!(profile_for_uri("file:///home/me/track.mp3").is_none());
    }

    #[test]
    fn http_to_mp3_is_podcast() {
        assert_eq!(
            profile_for_uri("https://feed.example.com/ep042.mp3"),
            Some(AudioCacheProfile::Podcast)
        );
    }

    #[test]
    fn http_to_pls_is_radio() {
        assert_eq!(
            profile_for_uri("http://stream.example.com/live.pls"),
            Some(AudioCacheProfile::Radio)
        );
    }

    #[test]
    fn https_stream_without_audio_ext_is_radio() {
        assert_eq!(
            profile_for_uri("https://icecast.example.com/main"),
            Some(AudioCacheProfile::Radio)
        );
    }

    #[test]
    fn case_insensitive_scheme_match() {
        assert_eq!(
            profile_for_uri("HTTPS://Feed.example.COM/EP042.MP3"),
            Some(AudioCacheProfile::Podcast)
        );
    }
}
