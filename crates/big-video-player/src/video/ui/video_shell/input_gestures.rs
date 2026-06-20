use super::*;

pub(super) fn wire_osd_autohide(main_overlay: &gtk::Overlay, osd: &Rc<OsdState>) {
    let motion = gtk::EventControllerMotion::new();
    {
        let osd = Rc::downgrade(osd);
        motion.connect_motion(move |_, _, _| {
            if let Some(osd) = osd.upgrade() {
                osd.show_and_schedule_hide();
            }
        });
    }
    {
        let osd = Rc::downgrade(osd);
        motion.connect_leave(move |_| {
            if let Some(osd) = osd.upgrade() {
                osd.hide();
            }
        });
    }
    main_overlay.add_controller(motion);
}

/// Wire the desktop-background context menu. This intentionally exposes only
/// wallpaper-level actions; the full player menus stay in [`wire_video_input`].
pub(super) fn wire_background_video_input(main_overlay: &gtk::Overlay) {
    let ctx_menu = gio::Menu::new();
    ctx_menu.append(Some(&i18n("Choose Video…")), Some("win.open-file"));
    ctx_menu.append(Some(&i18n("Pause / Resume")), Some("win.play-pause"));

    let popover = gtk::PopoverMenu::from_model(Some(&ctx_menu));
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
}

/// Wire the pointer gestures over the video surface (mirrors the old
/// `input/mod.rs`): button-1 double-click toggles fullscreen, right-click pops
/// the context menu, and a button-1 drag past [`DRAG_THRESHOLD`] moves the
/// window (unless fullscreen). All attach to `main_overlay`, which receives the
/// bubbled events from both render backends; the overlay's interactive children
/// (header / controls) handle their own clicks first, so a drag/click on a
/// button never reaches these.
pub(super) fn wire_video_input(
    // The mountable content (ADR D14); the window (for the window-drag gesture)
    // is resolved lazily from it.
    content: &adw::ToastOverlay,
    main_overlay: &gtk::Overlay,
    sender: &ComponentSender<VideoShell>,
) {
    // Double-click → fullscreen.
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_PRIMARY);
    {
        let sender = sender.clone();
        click.connect_released(move |gesture, n_press, _x, _y| {
            if n_press == 2 {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                sender.input(VideoShellInput::ToggleFullscreen);
            }
        });
    }
    main_overlay.add_controller(click);

    // Right-click → context menu (uses the `win.*` actions on the window, which
    // resolve through the action group inherited by this descendant).
    let ctx_menu = gio::Menu::new();
    let playback = gio::Menu::new();
    playback.append(Some(&i18n("Play / Pause")), Some("win.play-pause"));
    playback.append(Some(&i18n("Fullscreen")), Some("win.fullscreen"));
    playback.append(Some(&i18n("Screenshot")), Some("win.screenshot"));
    ctx_menu.append_section(None, &playback);
    let av = gio::Menu::new();
    av.append(Some(&i18n("Equalizer")), Some("win.show-equalizer"));
    av.append(
        Some(&i18n("Audio & Subtitle Delay")),
        Some("win.show-delay"),
    );
    av.append(Some(&i18n("Image & Video")), Some("win.show-image-adjust"));
    ctx_menu.append_section(None, &av);
    let nav = gio::Menu::new();
    nav.append(Some(&i18n("Open File…")), Some("win.open-file"));
    nav.append(Some(&i18n("Open URL…")), Some("win.open-url"));
    nav.append(Some(&i18n("Media Info")), Some("win.media-info"));
    ctx_menu.append_section(None, &nav);

    let popover = gtk::PopoverMenu::from_model(Some(&ctx_menu));
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

    // Drag → move the window (the old whole-window drag; the header is no longer
    // the only draggable zone). Claims only past the threshold and never while
    // fullscreen.
    let drag = gtk::GestureDrag::new();
    drag.set_button(gtk::gdk::BUTTON_PRIMARY);
    let drag_start = Rc::new(Cell::new((0.0_f64, 0.0_f64)));
    let move_started = Rc::new(Cell::new(false));
    {
        let drag_start = drag_start.clone();
        let move_started = move_started.clone();
        drag.connect_drag_begin(move |_, start_x, start_y| {
            drag_start.set((start_x, start_y));
            move_started.set(false);
        });
    }
    {
        // Weak: this gesture lives on the window's own drag zone — a strong
        // window capture is a window⇄gesture cycle that pinned the whole
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
