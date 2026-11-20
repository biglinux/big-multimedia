use super::*;

/// Collapse the OSD when the top-level window loses focus. The overlay's root is
/// only known once mounted, so the `is-active` handler is wired on `root-notify`.
pub(super) fn wire_osd_focus_out(main_overlay: &gtk::Overlay, osd: &Rc<OsdState>) {
    let osd = Rc::downgrade(osd);
    main_overlay.connect_root_notify(move |overlay| {
        let Some(window) = overlay.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let osd = osd.clone();
        window.connect_is_active_notify(move |window| {
            if !window.is_active()
                && let Some(osd) = osd.upgrade()
            {
                osd.hide_on_focus_out();
            }
        });
    });
}

/// Movement (px) below which a `motion` event is treated as "no movement".
/// A playing `GLArea` makes GTK re-emit `motion` at the SAME coordinates every
/// rendered frame; each re-emit would re-arm the idle timer, so the OSD would
/// never fade while the pointer rests over the video. The small threshold also
/// absorbs a resting hand's jitter.
const OSD_MOTION_EPSILON_PX: f64 = 2.0;

/// Pointer motion shows the OSD and restarts its fade; the pointer resting
/// anywhere, the OSD included, lets it fade.
pub(super) fn wire_osd_autohide(main_overlay: &gtk::Overlay, osd: &Rc<OsdState>) {
    let motion = gtk::EventControllerMotion::new();
    let last_pos = Rc::new(Cell::new((f64::NAN, f64::NAN)));
    {
        let osd = Rc::downgrade(osd);
        let last_pos = last_pos.clone();
        motion.connect_motion(move |_, x, y| {
            if !pointer_moved(last_pos.get(), x, y) {
                return;
            }
            last_pos.set((x, y));
            if let Some(osd) = osd.upgrade() {
                osd.show_and_schedule_hide();
            }
        });
    }
    {
        let osd = Rc::downgrade(osd);
        // Crossings come and go under a resting pointer as hover popovers open
        // over it, so only a real move counts. GDK forgets the buttons on
        // enter: a release the compositor kept while it moved the window
        // never reaches the overlay.
        motion.connect_enter(move |_, x, y| {
            let Some(osd) = osd.upgrade() else {
                return;
            };
            let was_held = osd.button_held.replace(false);
            if was_held || pointer_moved(last_pos.get(), x, y) {
                last_pos.set((x, y));
                osd.show_and_schedule_hide();
            }
        });
    }
    main_overlay.add_controller(motion);
}

/// The OSD in use: a button held over the overlay keeps it up, and keys
/// pressed inside the OSD (`osd_areas`) restart its fade.
pub(super) fn wire_osd_in_use(
    main_overlay: &gtk::Overlay,
    osd_areas: &[gtk::Widget],
    osd: &Rc<OsdState>,
) {
    // Watches presses and releases on everything under the overlay, the OSD's
    // popovers included, without taking them from their targets.
    let buttons = gtk::EventControllerLegacy::new();
    buttons.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let osd = Rc::downgrade(osd);
        buttons.connect_event(move |_, event| {
            if let Some(osd) = osd.upgrade() {
                let was_held = osd.button_held.get();
                let held = button_held_after(was_held, event.event_type(), event.modifier_state());
                osd.button_held.set(held);
                if held != was_held {
                    osd.show_and_schedule_hide();
                }
            }
            glib::Propagation::Proceed
        });
    }
    main_overlay.add_controller(buttons);

    for area in osd_areas {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let osd = Rc::downgrade(osd);
        keys.connect_key_pressed(move |_, _, _, _| {
            if let Some(osd) = osd.upgrade() {
                osd.show_and_schedule_hide();
            }
            glib::Propagation::Proceed
        });
        area.add_controller(keys);
    }
}

/// Whether the pointer at (`x`, `y`) left the [`OSD_MOTION_EPSILON_PX`] square
/// around `last`; a `NaN` `last` (no motion yet) never holds it.
fn pointer_moved(last: (f64, f64), x: f64, y: f64) -> bool {
    let (last_x, last_y) = last;
    let resting =
        (x - last_x).abs() < OSD_MOTION_EPSILON_PX && (y - last_y).abs() < OSD_MOTION_EPSILON_PX;
    !resting
}

/// Whether a pointer button or touch is down after `event`. A motion carries
/// the buttons down at that moment, which also clears a release that never
/// arrived.
fn button_held_after(
    held: bool,
    event: gtk::gdk::EventType,
    state: gtk::gdk::ModifierType,
) -> bool {
    use gtk::gdk::{EventType, ModifierType};
    match event {
        EventType::ButtonPress | EventType::TouchBegin => true,
        EventType::ButtonRelease | EventType::TouchEnd | EventType::TouchCancel => false,
        EventType::MotionNotify => state.intersects(
            ModifierType::BUTTON1_MASK
                | ModifierType::BUTTON2_MASK
                | ModifierType::BUTTON3_MASK
                | ModifierType::BUTTON4_MASK
                | ModifierType::BUTTON5_MASK,
        ),
        _ => held,
    }
}

/// Wire the pointer gestures over the video surface: button-1 double-click toggles fullscreen, right-click pops
/// the context menu, and a button-1 drag past [`DRAG_THRESHOLD`] moves the
/// window (unless fullscreen). All attach to `main_overlay`, which receives the
/// bubbled events from both render backends; the overlay's interactive children
/// (header / controls) handle their own clicks first, so a drag/click on a
/// button never reaches these.
pub(super) fn wire_video_input(
    // The mountable content; the window (for the window-drag gesture)
    // is resolved lazily from it.
    content: &gtk::Box,
    main_overlay: &gtk::Overlay,
    sender: &ComponentSender<VideoShell>,
) {
    // Double-click → fullscreen.
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_PRIMARY);
    {
        let sender = sender.clone();
        click.connect_released(move |gesture, n_press, x, y| {
            // A click on the video, outside the settings panel, puts it away.
            let on_panel = gesture
                .widget()
                .and_then(|overlay| overlay.pick(x, y, gtk::PickFlags::DEFAULT))
                .is_some_and(|picked| {
                    std::iter::successors(Some(picked), gtk::Widget::parent)
                        .any(|widget| widget.has_css_class("settings-panel"))
                });
            if n_press == 1 && !on_panel {
                sender.input(VideoShellInput::ClosePanel);
                sender.input(VideoShellInput::HidePlaylistPanel);
            }
            if n_press == 2 {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                sender.input(VideoShellInput::ToggleFullscreen);
            }
        });
    }
    main_overlay.add_controller(click);

    // Right-click → context menu (uses the `win.*` actions on the window, which
    // resolve through the action group inherited by this descendant).
    let context_menu = gio::Menu::new();
    big_gtk_kit::menu::append_shared_section(&context_menu, None, "video-context-playback", || {
        let playback = gio::Menu::new();
        playback.append(Some(&i18n("Play / Pause")), Some("win.play-pause"));
        playback.append(Some(&i18n("Fullscreen")), Some("win.fullscreen"));
        playback.append(Some(&i18n("Screenshot")), Some("win.screenshot"));
        playback
    });
    big_gtk_kit::menu::append_shared_section(&context_menu, None, "video-context-av", || {
        let av = gio::Menu::new();
        av.append(Some(&i18n("HDR")), Some("win.show-hdr"));
        av.append(Some(&i18n("Picture")), Some("win.show-image-adjust"));
        av.append(Some(&i18n("Zoom and position")), Some("win.show-framing"));
        av.append(Some(&i18n("Effects and filters")), Some("win.filters"));
        av.append(Some(&i18n("Equalizer")), Some("win.show-sound"));
        av
    });
    big_gtk_kit::menu::append_shared_section(&context_menu, None, "video-context-nav", || {
        let nav = gio::Menu::new();
        nav.append(Some(&i18n("Open File…")), Some("win.open-file"));
        nav.append(Some(&i18n("Open URL…")), Some("win.open-url"));
        nav.append(Some(&i18n("About this video")), Some("win.media-info"));
        nav
    });

    let popover = big_relm4_components::feedback::popover_menu::from_model(&context_menu);
    popover.set_parent(main_overlay);
    popover.set_has_arrow(false);
    big_relm4_components::feedback::popover_lifecycle::unparent_popover_on_parent_teardown(
        main_overlay.upcast_ref(),
        &popover,
    );
    let right_click = gtk::GestureClick::new();
    right_click.set_button(gtk::gdk::BUTTON_SECONDARY);
    {
        let popover = popover.clone();
        right_click.connect_released(move |gesture, _n_press, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
    }
    main_overlay.add_controller(right_click);

    // Drag → move the window (the header is not the only draggable zone).
    // Claims only past the threshold and never while fullscreen.
    let drag = gtk::GestureDrag::new();
    drag.set_button(gtk::gdk::BUTTON_PRIMARY);
    let drag_start = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
    let move_started = Rc::new(Cell::new(false));
    {
        let drag_start = drag_start.clone();
        let move_started = move_started.clone();
        drag.connect_drag_begin(move |gesture, start_x, start_y| {
            drag_start.set((start_x, start_y));
            // A drag that starts on a floating panel is the panel's (a row
            // being reordered, a slider): it does not move the window.
            let on_panel = gesture
                .widget()
                .and_then(|overlay| overlay.pick(start_x, start_y, gtk::PickFlags::DEFAULT))
                .is_some_and(|picked| {
                    std::iter::successors(Some(picked), gtk::Widget::parent)
                        .any(|widget| widget.has_css_class("settings-panel"))
                });
            move_started.set(on_panel);
        });
    }
    {
        // Weak: this gesture lives on the window's own drag zone; a strong
        // window capture is a window⇄gesture cycle that pins the whole
        // window after close.
        let content = content.downgrade();
        let drag_start = drag_start.clone();
        let move_started = move_started.clone();
        drag.connect_drag_update(move |gesture, dx, dy| {
            let Some(window) = content
                .upgrade()
                .and_then(|content| content.root())
                .and_downcast::<adw::ApplicationWindow>()
            else {
                return;
            };
            if move_started.get() || window.is_fullscreen() {
                return;
            }
            if dx.abs() <= DRAG_THRESHOLD && dy.abs() <= DRAG_THRESHOLD {
                return;
            }
            move_started.set(true);
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let Some(surface) = window.surface() else {
                return;
            };
            let Ok(toplevel) = surface.downcast::<gtk::gdk::Toplevel>() else {
                return;
            };
            let Some(device) = gesture.device() else {
                return;
            };
            let (start_x, start_y) = drag_start.get();
            let (win_x, win_y) = window.surface_transform();
            toplevel.begin_move(
                &device,
                gesture.current_button() as i32,
                start_x + win_x,
                start_y + win_y,
                gtk::gdk::CURRENT_TIME,
            );
        });
    }
    main_overlay.add_controller(drag);
}

#[cfg(test)]
mod tests {
    use super::{button_held_after, pointer_moved};
    use gtk::gdk::{EventType, ModifierType};

    #[test]
    fn resting_pointer_stays_idle() {
        // A playing GLArea re-emits motion at the same position every frame.
        assert!(!pointer_moved((400.0, 300.0), 400.0, 300.0));
        assert!(!pointer_moved((400.0, 300.0), 401.5, 298.5), "hand jitter");
        assert!(pointer_moved((400.0, 300.0), 402.0, 300.0));
        assert!(pointer_moved((400.0, 300.0), 400.0, 290.0));
        assert!(
            pointer_moved((f64::NAN, f64::NAN), 0.0, 0.0),
            "first motion"
        );
    }

    #[test]
    fn seek_drag_holds_button() {
        let pressed = button_held_after(false, EventType::ButtonPress, ModifierType::empty());
        assert!(pressed);
        let dragging =
            button_held_after(pressed, EventType::MotionNotify, ModifierType::BUTTON1_MASK);
        assert!(dragging);
        assert!(button_held_after(
            dragging,
            EventType::KeyPress,
            ModifierType::empty()
        ));
        assert!(!button_held_after(
            dragging,
            EventType::ButtonRelease,
            ModifierType::BUTTON1_MASK
        ));
    }

    #[test]
    fn touch_drag_holds_button() {
        let touching = button_held_after(false, EventType::TouchBegin, ModifierType::empty());
        assert!(touching);
        assert!(button_held_after(
            touching,
            EventType::TouchUpdate,
            ModifierType::empty()
        ));
        assert!(!button_held_after(
            touching,
            EventType::TouchEnd,
            ModifierType::empty()
        ));
        assert!(!button_held_after(
            touching,
            EventType::TouchCancel,
            ModifierType::empty()
        ));
    }

    #[test]
    fn lost_release_clears_on_motion() {
        assert!(!button_held_after(
            true,
            EventType::MotionNotify,
            ModifierType::SHIFT_MASK
        ));
    }
}
