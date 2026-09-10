//! EditPage public surface: thin setters that forward to mpv (filters,
//! rotation/flip, color, crop, seek), trim/audio-effect callbacks, and
//! crop-rect / video-overrides extraction for the conversion job.

use adw::prelude::*;
use big_relm4_components::list::bounded_list_box::clear_list_box_rows;
use gtk::glib;

use crate::player::mpv_backend::AudioEffects;

use super::EditPage;
use super::rotation::effective_rotation;

impl EditPage {
    /// Root [`adw::ToolbarView`]; embed in a parent container.
    #[must_use]
    pub fn widget(&self) -> &adw::ToolbarView {
        &self.toolbar_view
    }

    /// Set the mpv `vf` filter chain on the live preview.
    pub fn set_video_filter(&self, filter: &str) {
        if let Some(ref mpv) = *self.mpv.borrow() {
            mpv.set_vf(filter);
        }
    }

    /// Rotate the preview video by `degrees` (multiples of 90).
    pub fn set_rotation(&self, degrees: i32) {
        if let Some(ref mpv) = *self.mpv.borrow() {
            mpv.set_video_rotate(degrees as i64);
        }
    }

    /// Apply horizontal and/or vertical flip to the preview;
    /// rotation is recomputed because mpv composes the two
    /// orientations.
    pub fn set_flip(&self, horizontal: bool, vertical: bool) {
        self.flip_h.set(horizontal);
        self.flip_v.set(vertical);
        if let Some(ref mpv) = *self.mpv.borrow() {
            mpv.set_video_rotate(effective_rotation(self.rotation.get(), horizontal) as i64);
        }
        self.video_area.queue_render();
    }

    /// Push audio effects to the preview with a 150 ms debounce so
    /// slider drags do not thrash mpv. The filter chain is rebuilt
    /// each call and applied with a seek to ensure latency-free
    /// reload.
    pub fn apply_audio_effects(&self, fx: &AudioEffects) {
        if let Some(id) = self.af_timer.take() {
            id.remove();
        }

        use crate::player::mpv_backend::filters::build_audio_filter_chain;

        let filters = build_audio_filter_chain(fx);
        let mpv = self.mpv.clone();
        let af_timer = self.af_timer.clone();
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
            af_timer.set(None);
            if let Some(ref mpv) = *mpv.borrow() {
                if filters.is_empty() {
                    log::debug!("edit_page: clearing af");
                    mpv.set_audio_filter_with_seek("");
                } else {
                    let chain = format!("lavfi=[{}]", filters.join(","));
                    log::debug!("edit_page: applying af={chain}");
                    mpv.set_audio_filter_with_seek(&chain);
                }
            }
        });
        self.af_timer.set(Some(id));
    }

    /// Set mpv `brightness` property (range −100..=100).
    pub fn set_brightness(&self, val: i64) {
        if let Some(ref mpv) = *self.mpv.borrow() {
            mpv.set_brightness(val);
        }
    }

    /// Set mpv `saturation` property (range −100..=100).
    pub fn set_saturation(&self, val: i64) {
        if let Some(ref mpv) = *self.mpv.borrow() {
            mpv.set_saturation(val);
        }
    }

    /// Set mpv `hue` property (range −100..=100, degrees).
    pub fn set_hue(&self, val: i64) {
        if let Some(ref mpv) = *self.mpv.borrow() {
            mpv.set_hue(val);
        }
    }

    /// Set mpv `contrast` property (range −100..=100).
    pub fn set_contrast(&self, val: i64) {
        if let Some(ref mpv) = *self.mpv.borrow() {
            mpv.set_contrast(val);
        }
    }

    /// Apply a `video-crop` rectangle in mpv string form
    /// (`"WxH+X+Y"`).
    pub fn set_crop(&self, crop: &str) {
        if let Some(ref mpv) = *self.mpv.borrow() {
            mpv.set_video_crop(crop);
        }
    }

    /// Remove any active mpv `video-crop`.
    pub fn clear_crop(&self) {
        if let Some(ref mpv) = *self.mpv.borrow() {
            mpv.set_video_crop("");
        }
    }

    /// Seek the preview to `pos` seconds (absolute).
    pub fn seek_to(&self, pos: f64) {
        if let Some(ref mpv) = *self.mpv.borrow() {
            mpv.seek_absolute(pos);
        }
    }

    /// Cached playback position in seconds.
    #[must_use]
    pub fn current_position(&self) -> f64 {
        self.position.get()
    }

    /// Cached media duration in seconds.
    #[must_use]
    pub fn get_duration(&self) -> f64 {
        self.duration.get()
    }

    /// Reset every editor widget to defaults without dropping the
    /// mpv backend (use [`EditPage::unload`] for that).
    pub fn reset(&self) {
        self.segments.borrow_mut().clear();
        self.mark_point.set(None);
        self.mark_button.remove_css_class("accent");
        self.duration.set(0.0);
        self.position.set(0.0);
        self.position_label.set_label("0:00:00.000 / 0:00:00.000");
        self.frame_label.set_label("");
        self.timeline.set_range(0.0, 1.0);
        self.timeline.set_value(0.0);
        self.play_button
            .set_icon_name("media-playback-start-symbolic");
        self.volume_scale.set_value(100.0);
        self.is_fullscreen.set(false);
        self.fullscreen_button
            .set_icon_name("view-fullscreen-symbolic");
        self.brightness_scale.set_value(0.0);
        self.contrast_scale.set_value(1.0);
        self.saturation_scale.set_value(1.0);
        self.hue_scale.set_value(0.0);
        self.rotation.set(0);
        self.flip_h.set(false);
        self.flip_v.set(false);
        self.crop_left.set_value(0.0);
        self.crop_right.set_value(0.0);
        self.crop_top.set_value(0.0);
        self.crop_bottom.set_value(0.0);
        self.video_width.set(0);
        self.video_height.set(0);
        self.media_metadata_resolution.set_label("...");
        self.media_metadata_codec.set_label("...");
        self.media_metadata_file_size.set_label("...");
        self.media_metadata_duration.set_label("...");
        clear_list_box_rows(&self.segments_listbox);
        self.segments_placeholder.set_visible(true);
        self.segments_list_row.set_visible(false);
        self.segment_overlay.queue_draw();
        *self.file_path.borrow_mut() = None;
    }

    /// Drop the mpv backend and render context, then reset every
    /// transient widget via [`EditPage::reset`].
    pub fn unload(&self) {
        let _ = self.render_ctx.borrow_mut().take();
        let _ = self.mpv.borrow_mut().take();
        self.reset();
    }

    /// Set the callback fired when the user applies trim ranges.
    pub fn connect_apply_trim<F: Fn(Vec<crate::converter::ffmpeg::TrimRange>) + 'static>(
        &self,
        f: F,
    ) {
        *self.on_apply_trim.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired whenever the in-memory segment list
    /// changes (add/remove/edit).
    pub fn connect_segments_changed<F: Fn(Vec<super::TrimSegment>) + 'static>(&self, f: F) {
        *self.on_segments_changed.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired when the user toggles fullscreen.
    pub fn connect_fullscreen<F: Fn(bool) + 'static>(&self, f: F) {
        *self.on_fullscreen.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired when the user clicks "Convert".
    pub fn connect_convert<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_convert.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired when the user requests the audio
    /// settings dialog.
    pub fn connect_show_audio_settings<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_show_audio_settings.borrow_mut() = Some(Box::new(f));
    }

    /// Set the callback fired when the user navigates back to the
    /// queue page.
    pub fn connect_back<F: Fn(()) + 'static>(&self, f: F) {
        *self.on_back.borrow_mut() = Some(Box::new(f));
    }

    /// `true` if the user picked "split each segment into a separate
    /// file"; `false` if they picked "join all segments".
    #[must_use]
    pub fn segment_mode_split(&self) -> bool {
        self.output_dropdown.selected() == 1
    }

    /// Programmatically set the split/join segment-output mode.
    pub fn set_segment_mode_split(&self, split: bool) {
        self.output_dropdown.set_selected(u32::from(split));
    }

    /// Snapshot the brightness/contrast/saturation/hue/rotation/flip widget
    /// values into a [`VideoOverrides`](crate::converter::queue::VideoOverrides)
    /// struct ready to hand to the conversion job.
    #[must_use]
    pub fn video_overrides(&self) -> crate::converter::queue::VideoOverrides {
        crate::converter::queue::VideoOverrides {
            brightness: self.brightness_scale.value(),
            contrast: self.contrast_scale.value(),
            saturation: self.saturation_scale.value(),
            hue: self.hue_scale.value(),
            rotation: self.rotation.get(),
            flip_h: self.flip_h.get(),
            flip_v: self.flip_v.get(),
        }
    }

    /// Build the FFmpeg-ready [`CropRect`](crate::converter::profiles::CropRect)
    /// from the four crop spin buttons, or `None` when no cropping is
    /// configured (or the cropped area collapses to zero).
    #[must_use]
    pub fn crop_rect(&self) -> Option<crate::converter::profiles::CropRect> {
        let left = self.crop_left.value() as u32;
        let right = self.crop_right.value() as u32;
        let top = self.crop_top.value() as u32;
        let bottom = self.crop_bottom.value() as u32;
        if left == 0 && right == 0 && top == 0 && bottom == 0 {
            return None;
        }
        let video_width = self.video_width.get().max(0) as u32;
        let video_height = self.video_height.get().max(0) as u32;
        let width = video_width.saturating_sub(left + right);
        let height = video_height.saturating_sub(top + bottom);
        if width == 0 || height == 0 {
            return None;
        }
        Some(crate::converter::profiles::CropRect {
            w: width,
            h: height,
            x: left,
            y: top,
        })
    }
}
