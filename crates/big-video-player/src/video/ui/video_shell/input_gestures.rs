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

pub(super) fn wire_osd_autohide(main_overlay: &gtk::Overlay, osd: &Rc<OsdState>) {
    let motion = gtk::EventControllerMotion::new();
    let last_pos = Rc::new(Cell::new((f64::NAN, f64::NAN)));
    {
        let osd = Rc::downgrade(osd);
        let last_pos = last_pos.clone();
        motion.connect_motion(move |_, x, y| {
            let (lx, ly) = last_pos.get();
            if (x - lx).abs() < OSD_MOTION_EPSILON_PX && (y - ly).abs() < OSD_MOTION_EPSILON_PX {
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
        // Arm the idle timer instead of hiding at once: leaving the video (or the
        // pointer moving onto a just-opened popover, which reads as a leave) must
        // fade the OSD after the same delay, and the delayed hide
        // re-checks the guards, so an open header menu keeps the OSD (and itself).
        motion.connect_leave(move |_| {
            if let Some(osd) = osd.upgrade() {
                osd.show_and_schedule_hide();
            }
        });
    }
    main_overlay.add_controller(motion);
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

    let popover = gtk::PopoverMenu::from_model(Some(&context_menu));
    popover.set_parent(main_overlay);
    popover.set_has_arrow(false);
    {
        let popover = popover.downgrade();
        main_overlay.connect_destroy(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
                if popover.parent().is_some() {
                    popover.unparent();
                }
            }
        });
    }
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
