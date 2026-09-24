//! Edit-page render plumbing: crop-overlay Cairo draw handler (rule-of-thirds
//! grid + dim mask outside crop), GLArea render handler bound to FastRender
//! / MpvBackend, mpv lifecycle (load file, react to file-loaded /
//! track-list / time-pos events), and the audio/subtitle/secondary-subtitle
//! track-list pickers driven by `TrackMetadata`.

use adw::prelude::*;
use gtk::glib;
use log::{error, info};
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use std::cell::RefCell;

use crate::i18n::i18n;
use crate::player::mpv_backend::video_surface;
use crate::player::mpv_backend::{MpvBackend, PlayerEvent, TrackMetadata};
use big_relm4_components::layout::widget_container::clear_box_children;

type MpvSlot = Rc<RefCell<Option<Arc<MpvBackend>>>>;

use super::EditPage;
use super::build::EditPageBuild;
use super::gl::{GL_FRAMEBUFFER_BINDING, gl_get_integerv};
use super::time_format::format_time_ms;

pub(super) fn install_draw_handlers(build: &EditPageBuild) {
    install_crop_overlay_renderer(build);
    install_video_area_renderer(build);
}

fn install_crop_overlay_renderer(build: &EditPageBuild) {
    // Weak: the draw func lives ON the crop overlay while the spinners'
    // redraw handlers hold the overlay strong — strong spinners here closed
    // an overlay⇄spinners mutual cycle (leaked the crop UI on close).
    let crop_left = build.page.crop_left.downgrade();
    let crop_right = build.page.crop_right.downgrade();
    let crop_top = build.page.crop_top.downgrade();
    let crop_bottom = build.page.crop_bottom.downgrade();
    let video_width = build.page.video_width.clone();
    let video_height = build.page.video_height.clone();
    let show_grid = build.page.show_grid.clone();

    build
        .page
        .crop_overlay_area
        .set_draw_func(move |_area, cairo, width, height| {
            let width = width as f64;
            let height = height as f64;

            if show_grid.get() && width > 20.0 && height > 20.0 {
                cairo.set_source_rgba(1.0, 1.0, 1.0, 0.3);
                cairo.set_line_width(0.7);
                cairo.set_dash(&[4.0, 4.0], 0.0);
                for idx in 1..3 {
                    let x = width * (idx as f64) / 3.0;
                    let y = height * (idx as f64) / 3.0;
                    cairo.move_to(x, 0.0);
                    cairo.line_to(x, height);
                    let _ = cairo.stroke();
                    cairo.move_to(0.0, y);
                    cairo.line_to(width, y);
                    let _ = cairo.stroke();
                }
                cairo.set_dash(&[], 0.0);
            }

            let video_width = video_width.get() as f64;
            let video_height = video_height.get() as f64;
            if video_width <= 0.0 || video_height <= 0.0 {
                return;
            }
            let (Some(crop_left), Some(crop_right), Some(crop_top), Some(crop_bottom)) = (
                crop_left.upgrade(),
                crop_right.upgrade(),
                crop_top.upgrade(),
                crop_bottom.upgrade(),
            ) else {
                return;
            };
            let left = crop_left.value();
            let right = crop_right.value();
            let top = crop_top.value();
            let bottom = crop_bottom.value();
            if left == 0.0 && right == 0.0 && top == 0.0 && bottom == 0.0 {
                return;
            }

            let x0 = (left / video_width) * width;
            let x1 = width - (right / video_width) * width;
            let y0 = (top / video_height) * height;
            let y1 = height - (bottom / video_height) * height;

            cairo.set_source_rgba(0.0, 0.0, 0.0, 0.45);
            cairo.rectangle(0.0, 0.0, width, y0);
            let _ = cairo.fill();
            cairo.rectangle(0.0, y1, width, height - y1);
            let _ = cairo.fill();
            cairo.rectangle(0.0, y0, x0, y1 - y0);
            let _ = cairo.fill();
            cairo.rectangle(x1, y0, width - x1, y1 - y0);
            let _ = cairo.fill();

            cairo.set_source_rgba(1.0, 1.0, 1.0, 0.8);
            cairo.set_line_width(1.5);
            cairo.rectangle(x0, y0, x1 - x0, y1 - y0);
            let _ = cairo.stroke();

            let crop_width = x1 - x0;
            let crop_height = y1 - y0;
            let handle_length = 12.0_f64.min(crop_width / 4.0).min(crop_height / 4.0);
            if handle_length > 3.0 {
                cairo.set_source_rgba(1.0, 1.0, 1.0, 1.0);
                cairo.set_line_width(2.5);

                cairo.move_to(x0, y0 + handle_length);
                cairo.line_to(x0, y0);
                cairo.line_to(x0 + handle_length, y0);
                let _ = cairo.stroke();

                cairo.move_to(x1 - handle_length, y0);
                cairo.line_to(x1, y0);
                cairo.line_to(x1, y0 + handle_length);
                let _ = cairo.stroke();

                cairo.move_to(x0, y1 - handle_length);
                cairo.line_to(x0, y1);
                cairo.line_to(x0 + handle_length, y1);
                let _ = cairo.stroke();

                cairo.move_to(x1 - handle_length, y1);
                cairo.line_to(x1, y1);
                cairo.line_to(x1, y1 - handle_length);
                let _ = cairo.stroke();
            }
        });
}

fn install_video_area_renderer(build: &EditPageBuild) {
    let render_context = build.page.render_ctx.clone();
    let flip_h = build.page.flip_h.clone();
    let flip_v = build.page.flip_v.clone();

    build.page.video_area.connect_render(move |area, _| {
        if let Some(ref context) = *render_context.borrow() {
            let mut framebuffer: i32 = 0;
            if let Some(get_integerv) = gl_get_integerv() {
                // SAFETY: `get_integerv` is the resolved `glGetIntegerv` (matching
                // ABI `extern "C" fn(u32, *mut i32)`); `GL_FRAMEBUFFER_BINDING`
                // returns one integer, written into the valid `&mut framebuffer`.
                unsafe { get_integerv(GL_FRAMEBUFFER_BINDING, &raw mut framebuffer) };
            }
            let scale = area.scale_factor();
            let width = area.width() * scale;
            let height = area.height() * scale;
            let flip_y = !(flip_v.get() ^ flip_h.get());
            video_surface::render_glarea_frame(context, framebuffer, width, height, flip_y);
        }
        glib::Propagation::Stop
    });
}

impl EditPage {
    /// Load a media file into the editor: spawns an mpv backend, wires
    /// the GLArea render handler, and starts the 50 ms event pump.
    /// Calling on an already-loaded page first unloads the previous
    /// backend. Idempotent w.r.t. transient mpv failures (logs + skips).
    pub fn load_file(&self, path: &Path) {
        self.unload();

        let path_str = path.to_string_lossy().to_string();
        *self.file_path.borrow_mut() = Some(path_str.clone());

        let (sender, receiver) = async_channel::unbounded();
        match crate::player::mpv_backend::MpvBackend::new(sender, "", "") {
            Ok(mpv) => {
                info!(
                    "Editor: preparing to load {}",
                    big_os_kit::url::MediaLogTarget(&path_str)
                );
                *self.mpv.borrow_mut() = Some(Arc::new(mpv));
                self.setup_gl_rendering(Some(path_str));
                self.setup_event_receiver(receiver);
                self.populate_track_menus_delayed();
                self.load_media_metadata(path);
            }
            Err(err) => error!("Editor: failed to init MPV: {err}"),
        }
    }

    fn load_media_metadata(&self, path: &Path) {
        let resolution_label = self.media_metadata_resolution.clone();
        let codec_label = self.media_metadata_codec.clone();
        let file_size_label = self.media_metadata_file_size.clone();
        let duration_label = self.media_metadata_duration.clone();
        let video_width = self.video_width.clone();
        let video_height = self.video_height.clone();
        let mpv = self.mpv.clone();

        // Stat off the main loop: a network/slow filesystem can block on
        // `metadata`. `file_path` already holds the path being loaded (set by
        // `load_file` before this call); compare against it in the result so a
        // rapid re-load to a different file drops the stale size.
        let stat_path = path.to_path_buf();
        let file_path = self.file_path.clone();
        let expected = path.to_string_lossy().to_string();
        crate::shared::spawn::spawn_blocking_result(
            move || std::fs::metadata(&stat_path).ok().map(|meta| meta.len()),
            move |len| {
                if file_path.borrow().as_deref() != Some(expected.as_str()) {
                    return;
                }
                if let Some(len) = len {
                    file_size_label.set_label(&big_media_components::bytes::decimal_detailed(len));
                }
            },
        );

        let attempts = Rc::new(Cell::new(0_u32));
        glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
            let count = attempts.get();
            if count >= 15 {
                return glib::ControlFlow::Break;
            }
            attempts.set(count + 1);

            if let Some(ref mpv) = *mpv.borrow()
                && let Some((width, height)) = mpv.get_video_dimensions()
            {
                resolution_label.set_label(&format!("{width}×{height}"));
                video_width.set(width);
                video_height.set(height);

                let codec = mpv.get_property_string("video-codec");
                if !codec.is_empty() {
                    codec_label.set_label(&codec);
                }
                if let Ok(duration) = mpv.get_property_string("duration").parse::<f64>() {
                    duration_label.set_label(&format_time_ms(duration));
                }
                return glib::ControlFlow::Break;
            }

            glib::ControlFlow::Continue
        });
    }

    fn populate_track_menus_delayed(&self) {
        let mpv = self.mpv.clone();
        // Weak captures: a strong ref to the track popover buttons would keep them
        // alive past the editor's teardown (the source outlives the widget
        // tree), producing "Finalizing GtkButton, but it still has children".
        let audio_box = self.audio_track_box.downgrade();
        let subtitle_box = self.subtitle_track_box.downgrade();
        let audio_track_button = self.audio_track_button.downgrade();
        let subtitle_track_button = self.subtitle_track_button.downgrade();

        let attempts = Rc::new(Cell::new(0_u32));
        glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
            let count = attempts.get();
            if count >= 10 {
                return glib::ControlFlow::Break;
            }
            attempts.set(count + 1);

            let (
                Some(audio_box),
                Some(subtitle_box),
                Some(audio_track_button),
                Some(subtitle_track_button),
            ) = (
                audio_box.upgrade(),
                subtitle_box.upgrade(),
                audio_track_button.upgrade(),
                subtitle_track_button.upgrade(),
            )
            else {
                return glib::ControlFlow::Break;
            };

            let tracks = match mpv.borrow().as_ref() {
                Some(m) => m.get_track_list(),
                None => return glib::ControlFlow::Break,
            };
            if tracks.is_empty() {
                return glib::ControlFlow::Continue;
            }

            clear_box_children(&audio_box);
            clear_box_children(&subtitle_box);

            let (has_audio, has_subtitle) =
                populate_track_checks(&tracks, &audio_box, &subtitle_box, &mpv);
            if has_subtitle {
                add_no_subtitle_option(&subtitle_box, &mpv);
            }

            audio_track_button.set_visible(has_audio);
            subtitle_track_button.set_visible(has_subtitle);
            glib::ControlFlow::Break
        });
    }

    // Tries OffloadRenderer (DMA-BUF / GraphicsOffload / Wayland subsurface)
    // first; falls back to GLArea — same two-tier strategy as the video
    // player. Detection needs a realized widget, so it runs lazily.
    fn setup_gl_rendering(&self, file_to_load: Option<String>) {
        if self.video_backend.borrow().is_none() && self.video_area.is_realized() {
            self.detect_and_install_backend();
        }

        let already_initialized = self.video_backend.borrow().is_some();
        if already_initialized {
            self.finish_render_setup(file_to_load);
            return;
        }

        // GLArea not yet realized → defer detection to the realize handler.
        // caveman: attach the realize handler at most once. Repeated
        // load_file calls would otherwise accumulate handlers and each
        // realize cycle would re-run install_best_backend, creating
        // additional OffloadRenderer instances and stale GL contexts.
        if self.realize_wired.get() {
            return;
        }
        self.realize_wired.set(true);
        // The closure holds a full-page clone on the page's OWN video_area —
        // a page⇄widget cycle. It is one-shot by design: disconnect after
        // the first realize so the clone (and the cycle) is dropped.
        let editor = std::cell::RefCell::new(Some(self.clone_for_realize()));
        let realize_signal_slot: Rc<RefCell<Option<glib::SignalHandlerId>>> =
            Rc::new(RefCell::new(None));
        let realize_signal_slot_for_callback = realize_signal_slot.clone();
        let realize_signal_id = self.video_area.connect_realize(move |area| {
            if let Some(editor) = editor.borrow_mut().take() {
                editor.detect_and_install_backend();
                editor.finish_render_setup(file_to_load.clone());
            }
            if let Some(id) = realize_signal_slot_for_callback.borrow_mut().take() {
                area.disconnect(id);
            }
        });
        *realize_signal_slot.borrow_mut() = Some(realize_signal_id);
    }

    fn detect_and_install_backend(&self) {
        if self.video_backend.borrow().is_some() {
            return;
        }
        let display = self.video_area.display();
        let backend = video_surface::install_best_backend(
            &display,
            &self.overlay,
            &self.video_area,
            "Editor",
        );
        self.video_backend.replace(Some(backend));
    }

    fn finish_render_setup(&self, file_to_load: Option<String>) {
        self.create_render_context();
        if let Some(ref path) = file_to_load
            && let Some(ref mpv) = *self.mpv.borrow()
        {
            info!("Editor: loading {}", big_os_kit::url::MediaLogTarget(path));
            mpv.load_file(path);
        }
    }

    // EditPage fields are all Rc/RefCell/Cell/refcounted GObjects, so each
    // .clone() is O(1) — same "clone the imp" pattern as audio/video windows.
    fn clone_for_realize(&self) -> Self {
        Self {
            toolbar_view: self.toolbar_view.clone(),
            header: self.header.clone(),
            convert_button: self.convert_button.clone(),
            back_button: self.back_button.clone(),
            paned: self.paned.clone(),
            brightness_scale: self.brightness_scale.clone(),
            contrast_scale: self.contrast_scale.clone(),
            saturation_scale: self.saturation_scale.clone(),
            hue_scale: self.hue_scale.clone(),
            rotation: self.rotation.clone(),
            flip_h: self.flip_h.clone(),
            flip_v: self.flip_v.clone(),
            segments_listbox: self.segments_listbox.clone(),
            segments_placeholder: self.segments_placeholder.clone(),
            segments_list_row: self.segments_list_row.clone(),
            output_dropdown: self.output_dropdown.clone(),
            segment_overlay: self.segment_overlay.clone(),
            video_area: self.video_area.clone(),
            overlay: self.overlay.clone(),
            transport_overlay_box: self.transport_overlay_box.clone(),
            timeline: self.timeline.clone(),
            position_label: self.position_label.clone(),
            duration_label: self.duration_label.clone(),
            frame_label: self.frame_label.clone(),
            play_button: self.play_button.clone(),
            mark_button: self.mark_button.clone(),
            speed_button: self.speed_button.clone(),
            volume_button: self.volume_button.clone(),
            volume_scale: self.volume_scale.clone(),
            fullscreen_button: self.fullscreen_button.clone(),
            audio_track_button: self.audio_track_button.clone(),
            subtitle_track_button: self.subtitle_track_button.clone(),
            audio_track_box: self.audio_track_box.clone(),
            subtitle_track_box: self.subtitle_track_box.clone(),
            crop_left: self.crop_left.clone(),
            crop_right: self.crop_right.clone(),
            crop_top: self.crop_top.clone(),
            crop_bottom: self.crop_bottom.clone(),
            crop_overlay_area: self.crop_overlay_area.clone(),
            show_grid: self.show_grid.clone(),
            grid_button: self.grid_button.clone(),
            media_metadata_resolution: self.media_metadata_resolution.clone(),
            media_metadata_codec: self.media_metadata_codec.clone(),
            media_metadata_file_size: self.media_metadata_file_size.clone(),
            media_metadata_duration: self.media_metadata_duration.clone(),
            segments: self.segments.clone(),
            mark_point: self.mark_point.clone(),
            duration: self.duration.clone(),
            position: self.position.clone(),
            seeking: self.seeking.clone(),
            is_fullscreen: self.is_fullscreen.clone(),
            video_width: self.video_width.clone(),
            video_height: self.video_height.clone(),
            file_path: self.file_path.clone(),
            mpv: self.mpv.clone(),
            render_ctx: self.render_ctx.clone(),
            video_backend: self.video_backend.clone(),
            realize_wired: self.realize_wired.clone(),
            af_timer: self.af_timer.clone(),
            hide_timer: self.hide_timer.clone(),
            on_apply_trim: self.on_apply_trim.clone(),
            on_segments_changed: self.on_segments_changed.clone(),
            on_fullscreen: self.on_fullscreen.clone(),
            on_convert: self.on_convert.clone(),
            on_show_audio_settings: self.on_show_audio_settings.clone(),
            on_back: self.on_back.clone(),
        }
    }

    fn create_render_context(&self) {
        if self.render_ctx.borrow().is_some() {
            return;
        }
        let mpv_ref = self.mpv.borrow();
        let Some(mpv) = mpv_ref.as_ref() else {
            return;
        };
        // Weak captures: this callback is STORED in self.render_ctx, so a
        // full-page clone here is a page⇄render_context Rc self-cycle that
        // leaked the whole editor page after any file load.
        let render_context_weak = Rc::downgrade(&self.render_ctx);
        let backend_weak = Rc::downgrade(&self.video_backend);
        let overlay_weak = self.overlay.downgrade();
        match video_surface::create_render_context(
            mpv,
            self.video_backend.borrow().as_ref(),
            &self.video_area,
            "Editor",
            move || {
                let (Some(render_context), Some(backend), Some(overlay)) = (
                    render_context_weak.upgrade(),
                    backend_weak.upgrade(),
                    overlay_weak.upgrade(),
                ) else {
                    return;
                };
                let render_context_ref = render_context.borrow();
                let Some(ref context) = *render_context_ref else {
                    return;
                };
                video_surface::request_offload_render(backend.borrow().as_ref(), context, &overlay);
            },
        ) {
            Ok(ctx) => {
                *self.render_ctx.borrow_mut() = Some(ctx);
                info!("Editor: render context created");
            }
            Err(err) => error!("Editor: render context failed: {err}"),
        }
    }

    fn setup_event_receiver(&self, receiver: async_channel::Receiver<PlayerEvent>) {
        let timeline = self.timeline.clone();
        let position_label = self.position_label.clone();
        let frame_label = self.frame_label.clone();
        let play_button = self.play_button.clone();
        let duration = self.duration.clone();
        let position = self.position.clone();
        let seeking = self.seeking.clone();

        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
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
                    PlayerEvent::PositionChanged(current_position) => {
                        position.set(current_position);
                        if !seeking.get() {
                            timeline.set_value(current_position);
                        }
                        let duration_value = duration.get();
                        position_label.set_label(&format!(
                            "{} / {}",
                            format_time_ms(current_position),
                            format_time_ms(duration_value)
                        ));
                        let frame = (current_position * 30.0) as u64;
                        let total = (duration_value * 30.0) as u64;
                        // Stage 8: prefix is user-visible; route the
                        // template through gettext so the label "Frame:"
                        // can be localised. The numeric pair stays
                        // language-neutral.
                        frame_label
                            .set_label(&format!("{}: {frame}/{total}", crate::i18n::i18n("Frame")));
                    }
                    PlayerEvent::DurationChanged(duration_value) => {
                        duration.set(duration_value);
                        timeline.set_range(0.0, duration_value.max(1.0));
                    }
                    PlayerEvent::PauseChanged(paused) => {
                        play_button.set_icon_name(if paused {
                            "media-playback-start-symbolic"
                        } else {
                            "media-playback-pause-symbolic"
                        });
                    }
                    _ => {}
                }
            }
            if channel_closed {
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }
}

fn format_track_label(track: &TrackMetadata) -> String {
    big_media_components::track_labels::mpv_track_label(
        track.id,
        &track.title,
        &track.lang,
        &track.codec,
        track.external,
        big_media_components::track_labels::TrackLabelText {
            track: &i18n("Track"),
            external: &i18n("external"),
        },
    )
}

fn link_to_group(check: &gtk::CheckButton, container: &gtk::Box) {
    if let Some(first) = container.first_child()
        && let Some(group) = first.downcast_ref::<gtk::CheckButton>()
    {
        check.set_group(Some(group));
    }
}

// Returns (is_audio, is_subtitle) so the caller can flip the popover-visible flags.
fn append_track_row(
    track: &TrackMetadata,
    audio_box: &gtk::Box,
    subtitle_box: &gtk::Box,
    audio_group: &mut Option<gtk::CheckButton>,
    mpv: &MpvSlot,
) -> (bool, bool) {
    let label = format_track_label(track);
    match track.track_type.as_str() {
        "audio" => {
            let check = gtk::CheckButton::builder().label(&label).build();
            if let Some(group) = audio_group.as_ref() {
                check.set_group(Some(group));
            } else {
                *audio_group = Some(check.clone());
            }
            if track.selected {
                check.set_active(true);
            }
            connect_track_select(&check, mpv, track.id, TrackKind::Audio);
            audio_box.append(&check);
            (true, false)
        }
        "sub" => {
            let check = gtk::CheckButton::builder().label(&label).build();
            link_to_group(&check, subtitle_box);
            if track.selected {
                check.set_active(true);
            }
            connect_track_select(&check, mpv, track.id, TrackKind::Subtitle);
            subtitle_box.append(&check);
            (false, true)
        }
        _ => (false, false),
    }
}

#[derive(Clone, Copy)]
enum TrackKind {
    Audio,
    Subtitle,
}

fn connect_track_select(check: &gtk::CheckButton, mpv: &MpvSlot, track_id: i64, kind: TrackKind) {
    let mpv = mpv.clone();
    check.connect_toggled(move |check| {
        if !check.is_active() {
            return;
        }
        let Some(ref mpv) = *mpv.borrow() else { return };
        match kind {
            TrackKind::Audio => mpv.set_audio_track(track_id),
            TrackKind::Subtitle => mpv.set_subtitle_track(track_id),
        }
    });
}

fn populate_track_checks(
    tracks: &[TrackMetadata],
    audio_box: &gtk::Box,
    subtitle_box: &gtk::Box,
    mpv: &MpvSlot,
) -> (bool, bool) {
    let mut has_audio = false;
    let mut has_subtitle = false;
    let mut audio_group: Option<gtk::CheckButton> = None;
    for track in tracks {
        let (a, s) = append_track_row(track, audio_box, subtitle_box, &mut audio_group, mpv);
        has_audio |= a;
        has_subtitle |= s;
    }
    (has_audio, has_subtitle)
}

fn add_no_subtitle_option(subtitle_box: &gtk::Box, mpv: &MpvSlot) {
    let none_check = gtk::CheckButton::builder()
        .label(&i18n("No subtitles"))
        .build();
    link_to_group(&none_check, subtitle_box);
    let mpv = mpv.clone();
    none_check.connect_toggled(move |check| {
        if !check.is_active() {
            return;
        }
        if let Some(ref mpv) = *mpv.borrow() {
            mpv.disable_subtitles();
        }
    });
    subtitle_box.prepend(&none_check);
}
