//! Playback control: load, play, pause, seek, playlist, chapters, stream record.

use std::ffi::CString;
use std::sync::atomic::Ordering;

use log::error;

use super::MpvBackend;
use super::types::AudioCacheProfile;

/// Fallback audio policy only. Panels with known intent override this heuristic.
#[must_use]
pub fn profile_for_uri(target: &str) -> Option<AudioCacheProfile> {
    let scheme = big_os_kit::url::uri_scheme(target)?;
    if !["http", "https", "rtsp", "rtmp", "mms"]
        .iter()
        .any(|s| scheme.eq_ignore_ascii_case(s))
    {
        return None;
    }
    let path = big_os_kit::url::uri_path(target);
    let extension = path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .rsplit_once('.')
        .map(|(_, e)| e);
    Some(
        if extension.is_some_and(|e| {
            ["mp3", "m4a", "ogg", "opus", "flac"]
                .iter()
                .any(|v| e.eq_ignore_ascii_case(v))
        }) {
            AudioCacheProfile::Podcast
        } else {
            AudioCacheProfile::Radio
        },
    )
}

impl MpvBackend {
    pub(super) fn queue_command(
        &self,
        command: &str,
        arguments: &[&str],
    ) -> Result<(), super::MpvError> {
        let mut strings = Vec::with_capacity(arguments.len() + 1);
        strings.push(CString::new(command)?);
        for arg in arguments {
            strings.push(CString::new(*arg)?);
        }
        let mut args: Vec<*const std::os::raw::c_char> =
            strings.iter().map(|s| s.as_ptr()).collect();
        args.push(std::ptr::null());
        // SAFETY: `args` is null-terminated and its strings outlive the call,
        // which copies them; the handle is live while `self` exists.
        let status =
            unsafe { mpv_sys::mpv_command_async(self.raw_mpv_handle, 0, args.as_mut_ptr()) };
        if status < 0 {
            Err(super::MpvError::Raw(status))
        } else {
            Ok(())
        }
    }
    /// Per-file pause is supplied before load, never Play immediately followed by Pause.
    pub fn load_audio_paused(&self, path: &str, profile: AudioCacheProfile, paused: bool) {
        if self.audio_only {
            self.set_audio_cache_profile(profile);
        }
        self.loading.store(true, Ordering::Relaxed);
        // Resume stays with the global resume-playback preference: mpv reads
        // the watch-later file before per-file options, so none can veto it.
        let options = if paused { "pause=yes" } else { "pause=no" };
        if let Err(error) = self.queue_command("loadfile", &[path, "replace", "-1", options]) {
            self.loading.store(false, Ordering::Relaxed);
            log::warn!("Could not enqueue media load: {error}");
        }
    }

    pub fn load_file_with_audio_profile(&self, path: &str, profile: AudioCacheProfile) {
        if self.audio_only {
            self.set_audio_cache_profile(profile);
        }
        self.load_inner(path);
    }

    pub fn load_file(&self, path: &str) {
        if self.audio_only {
            self.set_audio_cache_profile(profile_for_uri(path).unwrap_or(AudioCacheProfile::Music));
        }
        self.load_inner(path);
    }

    fn load_inner(&self, path: &str) {
        self.loading.store(true, Ordering::Relaxed);
        if let Err(error) = self.queue_command("loadfile", &[path, "replace"]) {
            self.loading.store(false, Ordering::Relaxed);
            error!(
                "Failed to load {}: {error}",
                big_os_kit::url::MediaLogTarget(path)
            );
        }
    }

    pub fn load_file_append(&self, path: &str) {
        if let Err(e) = self.queue_command("loadfile", &[path, "append-play"]) {
            error!(
                "Failed to append {}: {e}",
                big_os_kit::url::MediaLogTarget(path)
            );
        }
    }

    /// Append `paths` in order with as few commands as mpv's plaintext
    /// playlist allows. Every async command holds a reply slot in mpv's
    /// bounded client event queue until the event thread drains it, so one
    /// `loadfile` per entry overflows it on a large folder, and a synchronous
    /// one blocks the GTK loop while mpv opens the first file. The parser
    /// splits lines and strips leading blanks and a line-final CR, so such
    /// paths still go one by one.
    pub fn load_files_append(&self, paths: &[String]) {
        let mut list = String::new();
        for path in paths {
            if path.contains('\n') || path.ends_with('\r') || path.starts_with(char::is_whitespace)
            {
                self.append_list(&mut list);
                self.load_file_append(path);
            } else {
                list.push_str(path);
                list.push('\n');
            }
        }
        self.append_list(&mut list);
    }

    fn append_list(&self, list: &mut String) {
        if list.is_empty() {
            return;
        }
        let url = format!("memory://{list}");
        list.clear();
        if let Err(e) = self.queue_command("loadlist", &[&url, "append-play"]) {
            error!("Failed to append a playlist: {e}");
        }
    }

    pub fn play(&self) {
        let _ = self.queue_command("set", &["pause", "no"]);
    }
    pub fn pause(&self) {
        let _ = self.queue_command("set", &["pause", "yes"]);
    }
    pub fn toggle_pause(&self) {
        let _ = self.queue_command("cycle", &["pause"]);
    }
    pub fn stop(&self) {
        let _ = self.queue_command("stop", &[]);
    }
    pub fn seek(&self, seconds: f64) {
        if seconds.is_finite() {
            let _ = self.queue_command("seek", &[&seconds.to_string(), "relative"]);
        }
    }
    pub fn seek_absolute(&self, seconds: f64) {
        self.seek_absolute_async(seconds);
    }
    pub fn seek_absolute_async(&self, seconds: f64) {
        if seconds.is_finite() && seconds >= 0.0 {
            let _ = self.queue_command("seek", &[&seconds.to_string(), "absolute"]);
        }
    }

    /// Queue a PNG screenshot into private staging. Call off the GTK thread:
    /// staging setup performs filesystem I/O. The backend retains resources
    /// through cancellation until a terminal receipt or native termination.
    pub fn screenshot_to_file_async(
        &self,
        target: &std::path::Path,
        alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> std::io::Result<u64> {
        let stage = big_os_kit::storage::StagedFile::new(target)?;
        let command = CString::new("screenshot-to-file").expect("literal");
        let path = CString::new(
            stage
                .path()
                .to_str()
                .ok_or_else(|| std::io::Error::other("screenshot path is not UTF-8"))?,
        )?;
        let mode = CString::new("video").expect("literal");
        let mut args = [
            command.as_ptr(),
            path.as_ptr(),
            mode.as_ptr(),
            std::ptr::null(),
        ];
        let id = self
            .screenshots
            .register(stage, target.to_path_buf(), alive)?;
        // SAFETY: as in `queue_command`; the registry owns the staging file
        // until mpv's reply for `id`.
        let status =
            unsafe { mpv_sys::mpv_command_async(self.raw_mpv_handle, id, args.as_mut_ptr()) };
        if status < 0 {
            self.screenshots.rejected(id);
            Err(std::io::Error::other(format!(
                "screenshot command rejected: {status}"
            )))
        } else {
            Ok(id)
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

    /// Queued like the loads: set synchronously, it could run before mpv has
    /// taken the appended entries and land on the wrong one.
    pub fn playlist_play_index(&self, index: i64) {
        if let Err(error) = self.queue_command("playlist-play-index", &[&index.to_string()]) {
            error!("Failed to play playlist entry {index}: {error}");
        }
    }

    /// Move the entry at `from` to just before `to` (mpv's own meaning: `to`
    /// past the end appends). Queued like the loads, so a move right after an
    /// append sees the appended entry.
    pub fn playlist_move(&self, from: i64, to: i64) {
        if let Err(error) =
            self.queue_command("playlist-move", &[&from.to_string(), &to.to_string()])
        {
            error!("Failed to move playlist entry {from} to {to}: {error}");
        }
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

    /// Shuffle the playlist, or restore the order it was loaded in. The playing
    /// entry keeps playing and moves to the front: mpv shuffles it too, and
    /// Next only reaches what sits after it.
    pub fn playlist_shuffle(&self, shuffle: bool) {
        self.with_mpv(|m| {
            if !shuffle {
                let _ = m.command("playlist-unshuffle", &[]);
                return;
            }
            let _ = m.command("playlist-shuffle", &[]);
            if let Ok(current) = m.get_property::<i64>("playlist-pos")
                && current > 0
            {
                let _ = m.command("playlist-move", &[&current.to_string(), "0"]);
            }
        });
    }

    /// Playlist entries in mpv's current (possibly shuffled) order.
    pub fn get_playlist_paths(&self) -> Vec<String> {
        self.with_mpv(|m| {
            let count = m.get_property::<i64>("playlist-count").unwrap_or(0);
            (0..count)
                .filter_map(|index| {
                    m.get_property::<String>(&format!("playlist/{index}/filename"))
                        .ok()
                })
                .collect()
        })
    }

    pub fn get_playlist_pos(&self) -> i64 {
        self.with_mpv(|m| m.get_property::<i64>("playlist-pos").unwrap_or(-1))
    }

    pub fn get_playlist_count(&self) -> i64 {
        self.with_mpv(|m| m.get_property::<i64>("playlist-count").unwrap_or(0))
    }

    /// `(dwidth, dheight)`, which accounts for SAR/DAR. `None` when no video.
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

    pub fn set_stream_record(&self, path: &str) {
        self.with_mpv(|m| {
            let _ = m.set_property("stream-record", path.to_string());
        });
    }

    pub fn set_chapter(&self, idx: i64) {
        self.with_mpv(|m| {
            let _ = m.set_property("chapter", idx);
        });
    }

    /// Where `dvd://` (or `bd://` when `bluray`) reads from: a drive, an
    /// ISO or a folder.
    pub fn set_disc_device(&self, bluray: bool, device: &str) {
        let property = if bluray {
            "bluray-device"
        } else {
            "dvd-device"
        };
        self.with_mpv(|m| {
            let _ = m.set_property(property, device.to_string());
        });
    }

    /// Go back to where the last seek started.
    pub fn revert_seek(&self) {
        let _ = self.queue_command("revert-seek", &[]);
    }

    /// Seek to the start of the next (`1`) or previous (`-1`) subtitle line.
    pub fn sub_seek(&self, lines: i64) {
        let _ = self.queue_command("sub-seek", &[&lines.to_string()]);
    }

    /// Step `property` to its next value, wrapping (`sub`, `edition`, …).
    pub fn cycle(&self, property: &str) {
        let _ = self.queue_command("cycle", &[property]);
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
