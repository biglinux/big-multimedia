//! Edit-page action wiring: transport autohide (3s motion / 1s leave),
//! sidebar sliders + flip/rotate, transport buttons, speed-radio popover,
//! mark-point + bare-key shortcuts (space, ←/→, ,/., M, F).

use adw::prelude::*;
use gtk::{gdk, glib};
use std::cell::Cell;
use std::rc::Rc;

use super::build::EditPageBuild;
use super::rotation::effective_rotation;
use super::segments::{clear_segments_state, handle_mark_action};
use super::{Callback, SPEED_VALUES, TrimRange, fire};

pub(super) fn install_edit_page_actions(build: &EditPageBuild) {
    install_transport_autohide(build);
    install_sidebar_actions(build);
    install_transport_actions(build);
    install_keyboard_shortcuts(build);
}

pub(super) fn toggle_fullscreen_state(
    is_fullscreen: &Rc<Cell<bool>>,
    fullscreen_button: &gtk::Button,
    on_fullscreen: &Callback<bool>,
) {
    let new_state = !is_fullscreen.get();
    is_fullscreen.set(new_state);
    fullscreen_button.set_icon_name(if new_state {
        "view-restore-symbolic"
    } else {
        "view-fullscreen-symbolic"
    });
    fire(on_fullscreen, new_state);
}

fn install_transport_autohide(build: &EditPageBuild) {
    let transport_overlay_box = build.page.transport_overlay_box.clone();
    let hide_timer = build.page.hide_timer.clone();
    let overlay = build.page.overlay.clone();

    let motion = gtk::EventControllerMotion::new();
    {
        let transport_overlay_box = transport_overlay_box.clone();
        let hide_timer = hide_timer.clone();
        motion.connect_motion(move |_, _x, _y| {
            transport_overlay_box.set_visible(true);
            if let Some(old) = hide_timer.take() {
                old.remove();
            }
            let overlay_box = transport_overlay_box.clone();
            let timer = hide_timer.clone();
            let id = glib::timeout_add_seconds_local_once(3, move || {
                overlay_box.set_visible(false);
                timer.set(None);
            });
            hide_timer.set(Some(id));
        });
    }
    {
        let transport_overlay_box = transport_overlay_box.clone();
        let hide_timer = hide_timer.clone();
        motion.connect_leave(move |_| {
            if let Some(old) = hide_timer.take() {
                old.remove();
            }
            let overlay_box = transport_overlay_box.clone();
            let timer = hide_timer.clone();
            let id = glib::timeout_add_seconds_local_once(1, move || {
                overlay_box.set_visible(false);
                timer.set(None);
            });
            hide_timer.set(Some(id));
        });
    }
    overlay.add_controller(motion);
}

#[allow(clippy::too_many_lines)] // GTK action wiring — long by nature
fn install_sidebar_actions(build: &EditPageBuild) {
    {
        let mpv = build.page.mpv.clone();
        build
            .page
            .brightness_scale
            .connect_value_changed(move |scale| {
                if let Some(ref mpv) = *mpv.borrow() {
                    mpv.set_brightness((scale.value() * 100.0) as i64);
                }
            });
        let brightness_scale = build.page.brightness_scale.clone();
        build
            .brightness_reset
            .connect_clicked(move |_| brightness_scale.set_value(0.0));
    }

    {
        let mpv = build.page.mpv.clone();
        build
            .page
            .contrast_scale
            .connect_value_changed(move |scale| {
                if let Some(ref mpv) = *mpv.borrow() {
                    mpv.set_contrast(((scale.value() - 1.0) * 100.0) as i64);
                }
            });
        let contrast_scale = build.page.contrast_scale.clone();
        build
            .contrast_reset
            .connect_clicked(move |_| contrast_scale.set_value(1.0));
    }

    {
        let mpv = build.page.mpv.clone();
        build
            .page
            .saturation_scale
            .connect_value_changed(move |scale| {
                if let Some(ref mpv) = *mpv.borrow() {
                    mpv.set_saturation(((scale.value() - 1.0) * 100.0) as i64);
                }
            });
        let saturation_scale = build.page.saturation_scale.clone();
        build
            .saturation_reset
            .connect_clicked(move |_| saturation_scale.set_value(1.0));
    }

    {
        let mpv = build.page.mpv.clone();
        build.page.hue_scale.connect_value_changed(move |scale| {
            if let Some(ref mpv) = *mpv.borrow() {
                mpv.set_hue((scale.value() * 180.0) as i64);
            }
        });
        let hue_scale = build.page.hue_scale.clone();
        build
            .hue_reset
            .connect_clicked(move |_| hue_scale.set_value(0.0));
    }

    {
        let mpv = build.page.mpv.clone();
        let rotation = build.page.rotation.clone();
        let flip_h = build.page.flip_h.clone();
        build.rot_left_button.connect_clicked(move |_| {
            let new_rotation = (rotation.get() - 90).rem_euclid(360);
            rotation.set(new_rotation);
            if let Some(ref mpv) = *mpv.borrow() {
                mpv.set_video_rotate(effective_rotation(new_rotation, flip_h.get()) as i64);
            }
        });
    }
    {
        let mpv = build.page.mpv.clone();
        let rotation = build.page.rotation.clone();
        let flip_h = build.page.flip_h.clone();
        build.rot_right_button.connect_clicked(move |_| {
            let new_rotation = (rotation.get() + 90).rem_euclid(360);
            rotation.set(new_rotation);
            if let Some(ref mpv) = *mpv.borrow() {
                mpv.set_video_rotate(effective_rotation(new_rotation, flip_h.get()) as i64);
            }
        });
    }

    {
        let mpv = build.page.mpv.clone();
        let flip_h = build.page.flip_h.clone();
        let rotation = build.page.rotation.clone();
        let video_area = build.page.video_area.clone();
        // Signal arg, not a clone: self-capture = button⇄handler leak cycle.
        build.flip_h_button.connect_clicked(move |flip_h_button| {
            let new_value = !flip_h.get();
            flip_h.set(new_value);
            if new_value {
                flip_h_button.add_css_class("accent");
            } else {
                flip_h_button.remove_css_class("accent");
            }
            if let Some(ref mpv) = *mpv.borrow() {
                mpv.set_video_rotate(effective_rotation(rotation.get(), new_value) as i64);
            }
            video_area.queue_render();
        });
    }
    {
        let flip_v = build.page.flip_v.clone();
        let video_area = build.page.video_area.clone();
        // Signal arg, not a clone: self-capture = button⇄handler leak cycle.
        build.flip_v_button.connect_clicked(move |flip_v_button| {
            let new_value = !flip_v.get();
            flip_v.set(new_value);
            if new_value {
                flip_v_button.add_css_class("accent");
            } else {
                flip_v_button.remove_css_class("accent");
            }
            video_area.queue_render();
        });
    }

    {
        let mpv = build.page.mpv.clone();
        let rotation = build.page.rotation.clone();
        let flip_h = build.page.flip_h.clone();
        let flip_v = build.page.flip_v.clone();
        let flip_h_button = build.flip_h_button.clone();
        let flip_v_button = build.flip_v_button.clone();
        let video_area = build.page.video_area.clone();
        build.reset_transform_button.connect_clicked(move |_| {
            rotation.set(0);
            flip_h.set(false);
            flip_v.set(false);
            flip_h_button.remove_css_class("accent");
            flip_v_button.remove_css_class("accent");
            if let Some(ref mpv) = *mpv.borrow() {
                mpv.set_video_rotate(0);
            }
            video_area.queue_render();
        });
    }

    {
        let on_show_audio_settings = build.page.on_show_audio_settings.clone();
        build.audio_row.connect_activated(move |_| {
            fire(&on_show_audio_settings, ());
        });
    }

    {
        let on_apply_trim = build.page.on_apply_trim.clone();
        let on_convert = build.page.on_convert.clone();
        let segments = build.page.segments.clone();
        build.page.convert_button.connect_clicked(move |_| {
            let ranges: Vec<TrimRange> = {
                let segments = segments.borrow();
                segments
                    .iter()
                    .map(|segment| TrimRange {
                        start: Some(segment.start),
                        end: Some(segment.end),
                    })
                    .collect()
            };
            fire(&on_apply_trim, ranges);
            fire(&on_convert, ());
        });
    }

    {
        let context = build.page.seg_list_actions();
        let mark_point = build.page.mark_point.clone();
        let mark_button = build.page.mark_button.clone();
        let mpv = build.page.mpv.clone();
        build.add_segs_button.connect_clicked(move |_| {
            let _ = handle_mark_action(&mpv, &mark_point, &mark_button, &context);
        });
    }

    {
        let context = build.page.seg_list_actions();
        let mark_point = build.page.mark_point.clone();
        let mark_button = build.page.mark_button.clone();
        // Signal arg, not a clone: self-capture = button⇄handler leak cycle.
        build.clear_segs_button.connect_clicked(move |button| {
            // Stage 10: clearing segments discards mark points + ranges the
            // user has been building up; confirm before destruction.
            let context = context.clone();
            let mark_point = mark_point.clone();
            let mark_button = mark_button.clone();
            let dialog = crate::shared::dialogs::new_confirm_dialog(
                &crate::i18n::i18n("Clear all segments?"),
                &crate::i18n::i18n("Mark points and segment ranges will be removed."),
                "clear",
                &crate::i18n::i18n("Clear"),
                true,
            );
            dialog.connect_response(None, move |_, response| {
                if response == "clear" {
                    clear_segments_state(&context, &mark_point, &mark_button);
                }
            });
            let parent: Option<gtk::Window> = button.root().and_downcast();
            dialog.present(parent.as_ref());
        });
    }
}

fn install_transport_actions(build: &EditPageBuild) {
    {
        let mpv = build.page.mpv.clone();
        build.page.play_button.connect_clicked(move |_| {
            if let Some(ref mpv) = *mpv.borrow() {
                mpv.toggle_pause();
            }
        });
    }
    {
        let mpv = build.page.mpv.clone();
        build.seek_back_button.connect_clicked(move |_| {
            if let Some(ref mpv) = *mpv.borrow() {
                mpv.seek(-5.0);
            }
        });
    }
    {
        let mpv = build.page.mpv.clone();
        build.seek_fwd_button.connect_clicked(move |_| {
            if let Some(ref mpv) = *mpv.borrow() {
                mpv.seek(5.0);
            }
        });
    }
    {
        let mpv = build.page.mpv.clone();
        build.frame_back_button.connect_clicked(move |_| {
            if let Some(ref mpv) = *mpv.borrow() {
                mpv.frame_back_step();
            }
        });
    }
    {
        let mpv = build.page.mpv.clone();
        build.frame_fwd_button.connect_clicked(move |_| {
            if let Some(ref mpv) = *mpv.borrow() {
                mpv.frame_step();
            }
        });
    }

    {
        let mpv = build.page.mpv.clone();
        let mark_point = build.page.mark_point.clone();
        let context = build.page.seg_list_actions();
        // Signal arg, not a clone: self-capture = button⇄handler leak cycle.
        build.page.mark_button.connect_clicked(move |mark_button| {
            let _ = handle_mark_action(&mpv, &mark_point, mark_button, &context);
        });
    }

    {
        let mpv = build.page.mpv.clone();
        let speed_button = build.page.speed_button.clone();
        let mut child = build.speed_box.first_child();
        let mut idx = 0_usize;
        while let Some(widget) = child {
            if let Some(check) = widget.downcast_ref::<gtk::CheckButton>() {
                let mpv = mpv.clone();
                // Weak: a strong speed_button here forms a cycle (button → popover
                // → check → this closure → button), keeping it alive past
                // teardown ("Finalizing GtkButton, but it still has children").
                let speed_button = speed_button.downgrade();
                let speed_idx = idx;
                check.connect_toggled(move |check| {
                    if check.is_active()
                        && let Some(ref mpv) = *mpv.borrow()
                    {
                        let speed = SPEED_VALUES[speed_idx];
                        mpv.set_speed(speed);
                        let labels = crate::ui_labels::speed_labels();
                        if let (Some(label), Some(speed_button)) =
                            (labels.get(speed_idx), speed_button.upgrade())
                        {
                            speed_button.set_label(label);
                        }
                    }
                });
            }
            child = widget.next_sibling();
            idx += 1;
        }
    }

    {
        let mpv = build.page.mpv.clone();
        build.page.volume_scale.connect_value_changed(move |scale| {
            if let Some(ref mpv) = *mpv.borrow() {
                mpv.set_volume(scale.value());
            }
        });
    }

    {
        let is_fullscreen = build.page.is_fullscreen.clone();
        let on_fullscreen = build.page.on_fullscreen.clone();
        // Signal arg, not a clone: self-capture = button⇄handler leak cycle.
        build
            .page
            .fullscreen_button
            .connect_clicked(move |fullscreen_button| {
                toggle_fullscreen_state(&is_fullscreen, fullscreen_button, &on_fullscreen);
            });
    }
}

fn install_keyboard_shortcuts(build: &EditPageBuild) {
    let key_controller = gtk::EventControllerKey::new();
    {
        let mpv = build.page.mpv.clone();
        let mark_point = build.page.mark_point.clone();
        let context = build.page.seg_list_actions();
        let is_fullscreen = build.page.is_fullscreen.clone();
        let mark_button = build.page.mark_button.clone();
        let fullscreen_button = build.page.fullscreen_button.clone();
        let on_fullscreen = build.page.on_fullscreen.clone();
        key_controller.connect_key_pressed(move |_, key, _, _| match key {
            gdk::Key::space => {
                if let Some(ref mpv) = *mpv.borrow() {
                    mpv.toggle_pause();
                }
                glib::Propagation::Stop
            }
            gdk::Key::Left => {
                if let Some(ref mpv) = *mpv.borrow() {
                    mpv.seek(-1.0);
                }
                glib::Propagation::Stop
            }
            gdk::Key::Right => {
                if let Some(ref mpv) = *mpv.borrow() {
                    mpv.seek(1.0);
                }
                glib::Propagation::Stop
            }
            gdk::Key::comma => {
                if let Some(ref mpv) = *mpv.borrow() {
                    mpv.frame_back_step();
                }
                glib::Propagation::Stop
            }
            gdk::Key::period => {
                if let Some(ref mpv) = *mpv.borrow() {
                    mpv.frame_step();
                }
                glib::Propagation::Stop
            }
            gdk::Key::m | gdk::Key::M => {
                let _ = handle_mark_action(&mpv, &mark_point, &mark_button, &context);
                glib::Propagation::Stop
            }
            gdk::Key::f | gdk::Key::F => {
                toggle_fullscreen_state(&is_fullscreen, &fullscreen_button, &on_fullscreen);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        });
    }
    build.page.toolbar_view.add_controller(key_controller);
}
