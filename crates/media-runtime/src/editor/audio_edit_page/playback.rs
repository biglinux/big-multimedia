//! AudioEditPage playback: mpv backend (audio-only), 50 ms event pump for
//! position/duration/pause/EndFile, play-selection mode (loop within
//! current segment, advance to next), waveform generation via background
//! ffmpeg → mono f32le with auto-rate (~2M samples target, clamped 200–44.1k).

use adw::prelude::*;
use std::path::{Path, PathBuf};

use big_app_kit::subprocess::BigSubprocessSpec;

use crate::shared::ui_helpers::spawn_blocking_result;

use super::segments::{format_time, refresh_segments_list, seek_next_or_restart_segment};
use super::{AudioEditPage, PlayerEvent, fire};

impl AudioEditPage {
    /// Start audio playback at the current position (no-op if no
    /// file is loaded).
    pub fn play(&self) {
        if let Some(ref player) = *self.mpv.borrow() {
            player.play();
            self.play_button
                .set_icon_name("media-playback-pause-symbolic");
            fire(&self.on_playback_state_changed, true);
        }
    }

    /// Flip between playing and paused; updates the play-button icon
    /// and fires the playback-state callback.
    pub fn toggle_play_pause(&self) {
        if let Some(ref player) = *self.mpv.borrow() {
            player.toggle_pause();
            if player.is_paused() {
                self.play_button
                    .set_icon_name("media-playback-start-symbolic");
                fire(&self.on_playback_state_changed, false);
            } else {
                self.play_button
                    .set_icon_name("media-playback-pause-symbolic");
                fire(&self.on_playback_state_changed, true);
            }
        }
    }

    /// Stop playback, clear hover/drag state, and reset the
    /// play-button icon. Suppresses the next auto-advance so a
    /// programmatic seek after stop does not pull the next track in.
    pub fn stop(&self) {
        self.suppress_auto_advance_after_manual_seek.set(true);
        if let Some(ref player) = *self.mpv.borrow() {
            player.pause();
            player.stop();
        }
        fire(&self.on_playback_state_changed, false);
        self.hover_segment.set(None);
        self.hover_delete_button.set(false);
        *self.drag_mode.borrow_mut() = super::DragMode::None;
        self.play_button
            .set_icon_name("media-playback-start-symbolic");
    }

    /// Load `path` into a fresh audio-only mpv backend, install the
    /// 50 ms event pump, and kick off background waveform generation.
    pub fn load_file(&self, path: &Path) {
        self.unload();

        let (sender, receiver) = async_channel::unbounded();
        if let Ok(player) = super::MpvBackend::new_audio_only(sender) {
            player.pause();
            player.load_file(&path.to_string_lossy());
            *self.mpv.borrow_mut() = Some(player);
            self.install_event_pump(receiver);
            self.spawn_waveform_generation(path.to_path_buf());
        }
    }

    /// Tear down the mpv backend and reset every transient widget
    /// (timeline, labels, hover/drag state, waveform cache).
    pub fn unload(&self) {
        let _ = self.mpv.borrow_mut().take();
        self.duration.set(0.0);
        self.position.set(0.0);
        self.timeline.set_range(0.0, 1.0);
        self.timeline.set_value(0.0);
        self.position_label.set_label("0:00");
        self.duration_label.set_label("0:00");
        self.pending_mark.set(None);
        self.hover_segment.set(None);
        self.hover_delete_button.set(false);
        *self.drag_mode.borrow_mut() = super::DragMode::None;
        self.play_button
            .set_icon_name("media-playback-start-symbolic");
        *self.waveform_data.borrow_mut() = None;
        self.waveform_generation
            .set(self.waveform_generation.get().wrapping_add(1));
        self.segments.borrow_mut().clear();
        refresh_segments_list(
            &self.segments_list,
            &self.segments,
            &self.segments_placeholder,
            &self.segments_row,
            self.mpv.clone(),
            self.waveform.clone(),
        );
        self.waveform.queue_draw();
    }

    fn install_event_pump(&self, receiver: async_channel::Receiver<PlayerEvent>) {
        let timeline = self.timeline.clone();
        let pos_label = self.position_label.clone();
        let dur_label = self.duration_label.clone();
        let duration = self.duration.clone();
        let position = self.position.clone();
        let timeline_syncing = self.timeline_syncing.clone();
        let suppress_auto_advance_after_manual_seek =
            self.suppress_auto_advance_after_manual_seek.clone();
        let play_button = self.play_button.clone();
        let auto_advance_button = self.auto_advance_button.clone();
        let play_selection_button = self.play_selection_button.clone();
        let on_next_track = self.on_next_track.clone();
        let on_playback_state_changed = self.on_playback_state_changed.clone();
        let segments = self.segments.clone();
        let mpv = self.mpv.clone();
        let waveform = self.waveform.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            // Stage 6 LEAK #3: stop the 50 ms pump as soon as the sender
            // side has been dropped (unload). `try_recv` returns `Closed`
            // once the matching `Sender` goes away; in that case return
            // `Break` so the GLib source is removed by the main context.
            let mut channel_closed = false;
            loop {
                let event = match receiver.try_recv() {
                    Ok(event) => event,
                    Err(async_channel::TryRecvError::Empty) => break,
                    Err(async_channel::TryRecvError::Closed) => {
                        channel_closed = true;
                        break;
                    }
                };
                match event {
                    PlayerEvent::PositionChanged(pos) => {
                        position.set(pos);
                        if duration.get() > 0.0 {
                            timeline_syncing.set(true);
                            timeline.set_value(pos);
                            timeline_syncing.set(false);
                        }
                        pos_label.set_label(&format_time(pos));

                        if play_selection_button.is_active() {
                            let segs = segments.borrow();
                            if !segs.is_empty() {
                                let now = pos;
                                let eps = 0.02;
                                if let Some((idx, seg)) = segs
                                    .iter()
                                    .enumerate()
                                    .find(|(_, s)| now >= s.start - eps && now <= s.end + eps)
                                {
                                    if now >= seg.end - eps {
                                        seek_next_or_restart_segment(
                                            &mpv,
                                            segs.get(idx + 1).map(|next| next.start),
                                            segs[0].start,
                                        );
                                    }
                                } else if let Some(next) = segs.iter().find(|s| s.start > now) {
                                    if let Some(ref player) = *mpv.borrow() {
                                        player.seek_absolute(next.start);
                                    }
                                } else if let Some(ref player) = *mpv.borrow() {
                                    player.pause();
                                    player.seek_absolute(segs[0].start);
                                }
                            }
                        }

                        waveform.queue_draw();
                    }
                    PlayerEvent::DurationChanged(dur) => {
                        duration.set(dur);
                        timeline.set_range(0.0, dur.max(1.0));
                        dur_label.set_label(&format_time(dur));
                        pos_label.set_label(&format_time(position.get()));
                        waveform.queue_draw();
                    }
                    PlayerEvent::PauseChanged(paused) => {
                        play_button.set_icon_name(if paused {
                            "media-playback-start-symbolic"
                        } else {
                            "media-playback-pause-symbolic"
                        });
                        fire(&on_playback_state_changed, !paused);
                    }
                    PlayerEvent::EndFile(_reason) => {
                        play_button.set_icon_name("media-playback-start-symbolic");
                        fire(&on_playback_state_changed, false);
                        let skip_once = suppress_auto_advance_after_manual_seek.replace(false);
                        if !skip_once
                            && auto_advance_button.is_active()
                            && !play_selection_button.is_active()
                        {
                            fire(&on_next_track, ());
                        }
                    }
                    _ => {}
                }
            }
            if channel_closed {
                gtk::glib::ControlFlow::Break
            } else {
                gtk::glib::ControlFlow::Continue
            }
        });
    }

    fn spawn_waveform_generation(&self, path: PathBuf) {
        let generation = self.waveform_generation.get().wrapping_add(1);
        self.waveform_generation.set(generation);
        *self.waveform_data.borrow_mut() = None;
        self.waveform.queue_draw();

        let waveform_samples = self.waveform_data.clone();
        let waveform = self.waveform.clone();
        let waveform_generation = self.waveform_generation.clone();
        let duration_hint = self.duration.get();

        spawn_blocking_result(
            move || generate_waveform_samples(&path, duration_hint),
            move |result| {
                if waveform_generation.get() != generation {
                    return;
                }
                *waveform_samples.borrow_mut() = result.ok();
                waveform.queue_draw();
            },
        );
    }
}

fn generate_waveform_samples(path: &Path, duration_hint: f64) -> Result<Vec<f32>, String> {
    let ffprobe = build_waveform_ffprobe_command(path)
        .run()
        .map_err(|e| format!("spawn ffprobe: {e}"))?;

    let duration = if ffprobe.status.success() {
        String::from_utf8_lossy(&ffprobe.stdout)
            .trim()
            .parse::<f64>()
            .unwrap_or(duration_hint)
    } else {
        duration_hint
    };

    let target_rate = if duration > 0.0 {
        ((2_000_000.0 / duration).clamp(200.0, 44_100.0)).round() as u32
    } else {
        2_000
    };

    let output = build_waveform_ffmpeg_command(path, target_rate)
        .run()
        .map_err(|e| format!("spawn ffmpeg: {e}"))?;

    if !output.status.success() {
        return Err("ffmpeg waveform failed".into());
    }

    if output.stdout.is_empty() {
        return Err("empty waveform".into());
    }

    let mut waveform_samples = Vec::with_capacity(output.stdout.len() / 4);
    for chunk in output.stdout.as_chunks::<4>().0 {
        waveform_samples.push(f32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }

    let max = waveform_samples
        .iter()
        .copied()
        .fold(0.0_f32, |acc, v| acc.max(v.abs()));
    if max > 0.0 {
        for sample in &mut waveform_samples {
            *sample /= max;
        }
    }
    Ok(waveform_samples)
}

// `--` terminator scopes path as positional — defends against hostile
// filenames beginning with `-` that ffprobe would parse as flags.
fn build_waveform_ffprobe_command(path: &Path) -> BigSubprocessSpec {
    BigSubprocessSpec::builder()
        .program("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            "--",
        ])
        .arg(path.as_os_str())
        .build()
}

// `-i` scopes the next argv element as input regardless of leading `-`.
fn build_waveform_ffmpeg_command(path: &Path, target_rate: u32) -> BigSubprocessSpec {
    BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args(["-vn", "-sn", "-v", "error", "-i"])
        .arg(path.as_os_str())
        .args(["-af", "pan=mono|c0=c0", "-f", "f32le", "-ar"])
        .arg(target_rate.to_string())
        .arg("-")
        .build()
}

#[cfg(test)]
mod argv_tests {
    use super::*;

    fn argv(cmd: &BigSubprocessSpec) -> Vec<String> {
        cmd.resolved().argv
    }

    // Hostile path beginning with `-` must be preceded by `--` or `-i`.
    #[test]
    fn waveform_ffprobe_separates_hostile_path() {
        let hostile = PathBuf::from("-f lavfi -i /dev/zero");
        let cmd = build_waveform_ffprobe_command(&hostile);
        let a = argv(&cmd);
        let target = hostile.to_string_lossy().into_owned();
        let idx = a.iter().position(|x| x == &target).expect("path argv");
        assert!(idx > 0);
        let prev = &a[idx - 1];
        assert!(prev == "--" || prev == "-i", "argv={a:?}");
    }

    #[test]
    fn waveform_ffmpeg_separates_hostile_path() {
        let hostile = PathBuf::from("-f lavfi -i /dev/zero");
        let cmd = build_waveform_ffmpeg_command(&hostile, 1000);
        let a = argv(&cmd);
        let target = hostile.to_string_lossy().into_owned();
        let idx = a.iter().position(|x| x == &target).expect("path argv");
        assert!(idx > 0);
        let prev = &a[idx - 1];
        assert!(prev == "--" || prev == "-i", "argv={a:?}");
    }
}
