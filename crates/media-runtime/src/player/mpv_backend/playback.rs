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
    pub(super) fn queue_command(&self, command: &str, arguments: &[&str]) -> Result<(), super::MpvError> {
        let mut strings = Vec::with_capacity(arguments.len() + 1);
        strings.push(CString::new(command)?);
        for arg in arguments { strings.push(CString::new(*arg)?); }
        let mut args: Vec<*const std::os::raw::c_char> = strings.iter().map(|s| s.as_ptr()).collect();
        args.push(std::ptr::null());
        let status = unsafe { mpv_sys::mpv_command_async(self.raw_mpv_handle, 0, args.as_mut_ptr()) };
        if status < 0 { Err(super::MpvError::Raw(status)) } else { Ok(()) }
    }
    /// Per-file pause is supplied before load, never Play immediately followed by Pause.
    pub fn load_audio_paused(&self, path: &str, profile: AudioCacheProfile, paused: bool) {
        if self.audio_only { self.set_audio_cache_profile(profile); }
        self.loading.store(true, Ordering::Relaxed);
        let options = if paused { "resume-playback=no,pause=yes" } else { "resume-playback=no,pause=no" };
        if let Err(error) = self.queue_command("loadfile", &[path, "replace", "-1", options]) {
            self.loading.store(false, Ordering::Relaxed);
            log::warn!("Could not enqueue media load: {error}");
        }
    }

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
            self.queue_command("loadfile", &[path, "replace", "-1", "resume-playback=no"])
        } else { self.queue_command("loadfile", &[path, "replace"]) };
        if let Err(error) = result {
            self.loading.store(false, Ordering::Relaxed);
            error!("Failed to load {}: {error}", big_os_kit::url::MediaLogTarget(path));
        }
    }

    pub fn load_file_append(&self, path: &str) {
        if let Err(e) = self.queue_command("loadfile", &[path, "append-play"]) {
            error!("Failed to append {}: {e}", big_os_kit::url::MediaLogTarget(path));
        }
    }

    pub fn play(&self) { let _ = self.queue_command("set", &["pause", "no"]); }
    pub fn pause(&self) { let _ = self.queue_command("set", &["pause", "yes"]); }
    pub fn toggle_pause(&self) { let _ = self.queue_command("cycle", &["pause"]); }
    pub fn stop(&self) { let _ = self.queue_command("stop", &[]); }
    pub fn seek(&self, seconds: f64) {
        if seconds.is_finite() { let _ = self.queue_command("seek", &[&seconds.to_string(), "relative"]); }
    }
    pub fn seek_absolute(&self, seconds: f64) { self.seek_absolute_async(seconds); }
    pub fn seek_absolute_async(&self, seconds: f64) {
        if seconds.is_finite() && seconds >= 0.0 { let _ = self.queue_command("seek", &[&seconds.to_string(), "absolute"]); }
    }

    /// Queue a PNG screenshot into private staging. Call off the GTK thread:
    /// staging setup performs filesystem I/O. The backend retains resources
    /// through cancellation until a terminal receipt or native termination.
    pub fn screenshot_to_file_async(
        &self, target: &std::path::Path, alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> std::io::Result<u64> {
        let stage = big_os_kit::storage::StagedFile::new(target)?;
        let command = CString::new("screenshot-to-file").expect("literal");
        let path = CString::new(stage.path().to_str().ok_or_else(|| std::io::Error::other("screenshot path is not UTF-8"))?)?;
        let mode = CString::new("video").expect("literal");
        let mut args = [command.as_ptr(), path.as_ptr(), mode.as_ptr(), std::ptr::null()];
        let id = self.screenshots.register(stage, target.to_path_buf(), alive)?;
        let status = unsafe { mpv_sys::mpv_command_async(self.raw_mpv_handle, id, args.as_mut_ptr()) };
        if status < 0 {
            self.screenshots.rejected(id);
            Err(std::io::Error::other(format!("screenshot command rejected: {status}")))
        } else { Ok(id) }
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
