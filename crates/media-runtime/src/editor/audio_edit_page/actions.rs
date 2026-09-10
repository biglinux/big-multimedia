//! Audio-edit basic actions: transport buttons, clear-segments confirm,
//! timeline seek, apply/convert/dialog buttons, and waveform zoom (popover
//! scale + scroll-to-zoom). Manual seek near the end suppresses the auto-
//! advance hop because users dragging to file-end expect playback to stop,
//! not jump tracks.

use adw::prelude::*;
use big_relm4_components::input::scale_popover::{
    BigVerticalScalePopoverSpec, vertical_scale_popover,
};
use gtk::glib;

use crate::converter::ffmpeg::TrimRange;
use crate::shared::popovers::setup_hover_controller_exclusive;

use super::build::AudioEditPageBuild;
use super::fire;
use super::segments::{refresh_segments_list, segments_to_trim_ranges};

pub(super) fn install_basic_actions(build: &AudioEditPageBuild) {
    install_transport_buttons(build);
    install_clear_segments_controller(build);
    install_timeline_seek_controller(build);
    install_apply_and_convert_handlers(build);
    install_dialog_buttons(build);
    install_zoom_controls(build);
}

fn install_transport_buttons(build: &AudioEditPageBuild) {
    {
        let mpv = build.page.mpv.clone();
        // Signal arg, not a clone: a strong self-capture in the button's own
        // handler is a button⇄handler cycle that leaks the page on close.
        build.page.play_button.connect_clicked(move |play_button| {
            if let Some(ref player) = *mpv.borrow() {
                player.toggle_pause();
                play_button.set_icon_name(if player.is_paused() {
                    "media-playback-start-symbolic"
                } else {
                    "media-playback-pause-symbolic"
                });
            }
        });
    }

    {
        let on_previous_track = build.page.on_previous_track.clone();
        build
            .prev_button
            .connect_clicked(move |_| fire(&on_previous_track, ()));
    }

    {
        let on_next_track = build.page.on_next_track.clone();
        build
            .next_button
            .connect_clicked(move |_| fire(&on_next_track, ()));
    }
}

fn install_clear_segments_controller(build: &AudioEditPageBuild) {
    let segments = build.page.segments.clone();
    let on_apply_trim = build.page.on_apply_trim.clone();
    let list = build.page.segments_list.clone();
    let placeholder = build.page.segments_placeholder.clone();
    let row = build.page.segments_row.clone();
    let pending_mark = build.page.pending_mark.clone();
    let waveform_ref = build.page.waveform.clone();
    let mpv_ref = build.page.mpv.clone();

    build.clear_button.connect_clicked(move |button| {
        if segments.borrow().is_empty() {
            return;
        }

        let segments2 = segments.clone();
        let on_apply_trim2 = on_apply_trim.clone();
        let list2 = list.clone();
        let placeholder2 = placeholder.clone();
        let row2 = row.clone();
        let pending_mark2 = pending_mark.clone();
        let waveform2 = waveform_ref.clone();
        let mpv2 = mpv_ref.clone();
        let dialog = crate::shared::ui_helpers::new_confirm_dialog(
            &crate::i18n::i18n("Clear All Segments?"),
            &crate::i18n::i18n("All cut marks will be removed. This cannot be undone."),
            "clear",
            &crate::i18n::i18n("Clear All"),
            true,
        );
        dialog.connect_response(None, move |_, response| {
            if response == "clear" {
                segments2.borrow_mut().clear();
                pending_mark2.set(None);
                refresh_segments_list(
                    &list2,
                    &segments2,
                    &placeholder2,
                    &row2,
                    mpv2.clone(),
                    waveform2.clone(),
                );
                fire(&on_apply_trim2, Vec::<TrimRange>::new());
                waveform2.queue_draw();
            }
        });
        dialog.present(Some(button));
    });
}

fn install_timeline_seek_controller(build: &AudioEditPageBuild) {
    let mpv = build.page.mpv.clone();
    let timeline_syncing = build.page.timeline_syncing.clone();
    let duration = build.page.duration.clone();
    let suppress_auto_advance_after_manual_seek =
        build.page.suppress_auto_advance_after_manual_seek.clone();

    build.page.timeline.connect_value_changed(move |scale| {
        if timeline_syncing.get() {
            return;
        }
        let target = scale.value();
        let dur = duration.get();
        let near_end = dur > 0.0 && target >= (dur - 0.05_f64).max(0.0_f64);
        suppress_auto_advance_after_manual_seek.set(near_end);
        if let Some(ref player) = *mpv.borrow() {
            player.seek_absolute(target);
        }
    });
}

fn install_apply_and_convert_handlers(build: &AudioEditPageBuild) {
    {
        let segments = build.page.segments.clone();
        let on_apply_trim = build.page.on_apply_trim.clone();
        build.apply_button.connect_clicked(move |_| {
            fire(&on_apply_trim, segments_to_trim_ranges(&segments.borrow()));
        });
    }

    {
        let on_convert = build.page.on_convert.clone();
        build.convert_button.connect_clicked(move |_| {
            fire(&on_convert, ());
        });
    }
}

fn install_dialog_buttons(build: &AudioEditPageBuild) {
    {
        let show_audio_settings = build.page.on_show_audio_settings.clone();
        build
            .settings_button
            .connect_clicked(move |_| fire(&show_audio_settings, ()));
    }

    {
        let show_equalizer = build.page.on_show_equalizer.clone();
        build
            .eq_toggle_button
            .connect_clicked(move |_| fire(&show_equalizer, ()));
    }
}

fn install_zoom_controls(build: &AudioEditPageBuild) {
    let (popover, zoom_scale) = vertical_scale_popover(
        &build.zoom_button,
        BigVerticalScalePopoverSpec::new(
            crate::i18n::i18n("Zoom"),
            crate::i18n::i18n("Waveform zoom"),
        )
        .range(1.0, 10.0, 0.2)
        .initial(1.0)
        .height_request(120),
    );

    {
        let zoom_level = build.page.zoom_level.clone();
        let zoom_label = build.page.zoom_label.clone();
        let waveform = build.page.waveform.clone();
        zoom_scale.connect_value_changed(move |scale| {
            let zoom = scale.value();
            zoom_level.set(zoom);
            zoom_label.set_label(&format!("{zoom:.1}×"));
            waveform.queue_draw();
        });
    }

    if let Some(hover_box) = build
        .zoom_button
        .parent()
        .and_then(|widget| widget.downcast::<gtk::Box>().ok())
    {
        setup_hover_controller_exclusive(&hover_box, &popover, &[]);
    }

    let zoom_level_scroll = build.page.zoom_level.clone();
    let zoom_label_scroll = build.page.zoom_label.clone();
    let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
    // Controller lives ON the waveform: redraw through the controller's own
    // widget — a strong waveform capture here is a widget⇄controller cycle.
    scroll.connect_scroll(move |ctrl, _dx, dy| {
        let current = zoom_level_scroll.get();
        let delta = if dy < 0.0 {
            0.2
        } else if dy > 0.0 {
            -0.2
        } else {
            0.0
        };
        let new_zoom = (current + delta).clamp(1.0, 10.0);
        zoom_level_scroll.set(new_zoom);
        zoom_label_scroll.set_label(&format!("{new_zoom:.1}×"));
        if let Some(widget) = ctrl.widget() {
            widget.queue_draw();
        }
        glib::Propagation::Stop
    });
    build.page.waveform.add_controller(scroll);
}
