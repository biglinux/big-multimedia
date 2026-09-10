//! AudioEditPage public surface: compact-mode toggle, callbacks for trim/
//! convert/dialog/track-nav, sidebar adjustment binders for volume + speed
//! popovers, and the live audio-preview push.

use adw::prelude::*;

use crate::converter::ffmpeg::TrimRange;
use crate::shared::popovers::{
    build_speed_popover, build_volume_popover, setup_hover_controller_exclusive,
};

use super::segments::{refresh_segments_list, segments_to_trim_ranges};
use super::{AudioEditPage, AudioEffects};

impl AudioEditPage {
    /// Root [`adw::ToolbarView`]; embed this in a parent container.
    #[must_use]
    pub fn widget(&self) -> &adw::ToolbarView {
        &self.toolbar
    }

    /// Switch between full and compact layout. Compact hides the
    /// header and trim group and reduces waveform height — used when
    /// the page sits inline inside the queue's audio preview slot.
    pub fn set_compact_mode(&self, compact: bool) {
        self.header.set_visible(!compact);
        self.waveform
            .set_content_height(if compact { 122 } else { 220 });
        self.waveform.set_vexpand(!compact);
        self.trim_group.set_visible(!compact);
        self.root.set_spacing(if compact { 6 } else { 12 });
        self.root.set_margin_top(if compact { 6 } else { 12 });
        self.root.set_margin_bottom(if compact { 6 } else { 12 });
        self.controls.set_margin_bottom(0);
    }

    /// Set the callback fired when the user applies trim/cut ranges.
    pub fn connect_apply_trim<F: Fn(Vec<TrimRange>) + 'static>(&self, f: F) {
        *self.on_apply_trim.borrow_mut() = Some(std::rc::Rc::new(f));
    }

    /// Set the callback fired when the user clicks "Convert".
    pub fn connect_convert<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_convert.borrow_mut() = Some(std::rc::Rc::new(f));
    }

    /// Set the callback fired when the user requests the audio
    /// settings dialog.
    pub fn connect_show_audio_settings<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_show_audio_settings.borrow_mut() = Some(std::rc::Rc::new(f));
    }

    /// Set the callback fired when the user requests the equalizer
    /// dialog.
    pub fn connect_show_equalizer<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_show_equalizer.borrow_mut() = Some(std::rc::Rc::new(f));
    }

    /// Set the callback fired whenever playback toggles between
    /// playing and paused (the parameter is `true` while playing).
    pub fn connect_playback_state_changed<F: Fn(bool) + 'static>(&self, f: F) {
        *self.on_playback_state_changed.borrow_mut() = Some(std::rc::Rc::new(f));
    }

    /// Set the callback fired when the user activates the
    /// "previous track" transport button.
    pub fn connect_previous_track<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_previous_track.borrow_mut() = Some(std::rc::Rc::new(f));
    }

    /// Set the callback fired when the user activates the
    /// "next track" transport button.
    pub fn connect_next_track<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_next_track.borrow_mut() = Some(std::rc::Rc::new(f));
    }

    /// Bind the volume popover scale to `adj` (range 0.0–2.0, where
    /// 1.0 is 100 %). Updates the inline label and speaker icon in
    /// both directions.
    pub fn bind_volume_adjustment(&self, adjustment: &gtk::Adjustment) {
        let volume_hover_box = self
            .volume_button
            .parent()
            .and_then(|widget| widget.downcast::<gtk::Box>().ok());
        if let Some(ref hover_box) = volume_hover_box {
            let (popover, scale) =
                build_volume_popover(hover_box, 200.0, adjustment.value() * 100.0);
            let popover_adjustment = adjustment.clone();
            let popover_volume_label = self.volume_label.clone();
            let button = self.volume_button.clone();
            scale.connect_value_changed(move |volume_scale| {
                let pct = volume_scale.value();
                popover_adjustment.set_value(pct / 100.0);
                popover_volume_label.set_label(&format!("{pct:.0}"));
                button.set_icon_name(if pct < 1.0 {
                    "audio-volume-muted-symbolic"
                } else if pct < 75.0 {
                    "audio-volume-low-symbolic"
                } else if pct < 150.0 {
                    "audio-volume-medium-symbolic"
                } else {
                    "audio-volume-high-symbolic"
                });
            });
            setup_hover_controller_exclusive(hover_box, &popover, &[]);

            let adjustment_volume_label = self.volume_label.clone();
            let adjustment_volume_button = self.volume_button.clone();
            adjustment.connect_value_changed(move |volume_adjustment| {
                let pct = volume_adjustment.value() * 100.0;
                adjustment_volume_label.set_label(&format!("{pct:.0}"));
                adjustment_volume_button.set_icon_name(if pct < 1.0 {
                    "audio-volume-muted-symbolic"
                } else if pct < 75.0 {
                    "audio-volume-low-symbolic"
                } else if pct < 150.0 {
                    "audio-volume-medium-symbolic"
                } else {
                    "audio-volume-high-symbolic"
                });
            });
        }
    }

    /// Bind the speed popover scale to `adjustment` (range 0.1-5.0). Updates
    /// the inline `1.00×`-style label in both directions.
    pub fn bind_speed_adjustment(&self, adjustment: &gtk::Adjustment) {
        let speed_hover_box = self
            .speed_button
            .parent()
            .and_then(|widget| widget.downcast::<gtk::Box>().ok());
        if let Some(ref hover_box) = speed_hover_box {
            let popover_speed_label = self.speed_label.clone();
            let popover_adjustment = adjustment.clone();
            let popover = build_speed_popover(
                hover_box,
                &self.speed_button,
                &self.speed_label,
                move |speed| {
                    popover_adjustment.set_value(speed);
                    // Locale-neutral × matches ui_labels::speed_labels.
                    popover_speed_label.set_label(&format!("{speed:.2}×"));
                },
            );
            setup_hover_controller_exclusive(hover_box, &popover, &[]);

            let adjustment_speed_label = self.speed_label.clone();
            adjustment.connect_value_changed(move |speed_adjustment| {
                adjustment_speed_label.set_label(&format!("{:.2}×", speed_adjustment.value()));
            });
        }
    }

    /// Reset the waveform horizontal zoom to 1× and redraw.
    pub fn reset_zoom(&self) {
        self.zoom_level.set(1.0);
        // Locale-neutral × for zoom multiplier.
        self.zoom_label.set_label("1.0×");
        self.waveform.queue_draw();
    }

    /// Toggle the "cut points" editing mode. When `active`, the cut
    /// reveal area expands, the waveform takes the extra height, and
    /// the pending mark is reset.
    pub fn set_cut_mode_active(&self, active: bool) {
        self.cut_mode_active.set(active);
        self.cut_revealer.set_reveal_child(active);
        self.waveform.set_vexpand(active);
        if !active {
            self.pending_mark.set(None);
            self.waveform.queue_draw();
        }
    }

    /// Returns `true` once an mpv backend has been allocated for the
    /// current file.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.mpv.borrow().is_some()
    }

    /// Current trim ranges as understood by FFmpeg, derived from the
    /// in-memory segment list.
    #[must_use]
    pub fn trim_ranges(&self) -> Vec<TrimRange> {
        segments_to_trim_ranges(&self.segments.borrow())
    }

    /// Push fresh audio-effect parameters into the live preview mpv
    /// instance. `volume_factor` is clamped to `0.0..=2.0` and `speed`
    /// to `0.1..=5.0`; pitch correction is enabled.
    pub fn apply_audio_preview(&self, fx: &AudioEffects<'_>, volume_factor: f64, speed: f64) {
        if let Some(ref player) = *self.mpv.borrow() {
            player.apply_audio_effects(fx);
            player.set_volume((volume_factor.clamp(0.0, 2.0)) * 100.0);
            player.set_speed(speed.clamp(0.1, 5.0));
            player.set_audio_pitch_correction(true);
        }
    }

    /// Replace the current trim segments with `segments`, refresh
    /// the segment list widget, and redraw the waveform overlay.
    pub fn set_segments(&self, segments: &[TrimRange]) {
        let mapped = segments
            .iter()
            .filter_map(|segment| {
                Some(super::AudioSegment {
                    start: segment.start?,
                    end: segment.end?,
                })
            })
            .collect();
        *self.segments.borrow_mut() = mapped;
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
}
