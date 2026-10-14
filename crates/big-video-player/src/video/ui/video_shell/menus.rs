//! Closing the header and control-bar menus.
//!
//! The control-bar menus are non-autohide popovers (an autohide popover takes
//! a Wayland grab that anchors it at the window origin), so GTK closes none of
//! them on its own: not on a click elsewhere, not on Escape, not when another
//! one opens. Left alone they piled up over the video. Escape is handled with
//! the keyboard shortcuts; this module closes a menu when another opens and on
//! a click outside it.

use super::*;

/// Make the `popovers` close one another and close on a click in `root`
/// outside them.
pub(super) fn wire_menu_dismissal(root: &gtk::Box, popovers: &[gtk::Popover]) {
    let weak: Rc<[glib::WeakRef<gtk::Popover>]> =
        popovers.iter().map(ObjectExt::downgrade).collect();

    // One open at a time: opening a menu closes the others.
    for popover in popovers {
        let weak = weak.clone();
        popover.connect_show(move |shown| {
            for other in weak.iter().filter_map(glib::WeakRef::upgrade) {
                if &other != shown && other.is_visible() {
                    other.popdown();
                }
            }
        });
    }

    // A press anywhere in the window outside an open menu closes it. The
    // press still reaches what it landed on. A press on a menu's own button
    // is left to the button, which closes the menu itself.
    let click = gtk::GestureClick::new();
    click.set_button(0);
    click.set_propagation_phase(gtk::PropagationPhase::Capture);
    click.connect_pressed(move |gesture, _n_press, x, y| {
        // Watch only: the press belongs to whatever it landed on.
        gesture.set_state(gtk::EventSequenceState::Denied);
        let Some(root) = gesture.widget() else {
            return;
        };
        // Presses inside a menu arrive on the menu's own surface.
        let on_window_surface = gesture
            .current_event()
            .and_then(|event| event.surface())
            .zip(root.native().and_then(|native| native.surface()))
            .is_some_and(|(event_surface, window_surface)| event_surface == window_surface);
        if !on_window_surface {
            return;
        }
        let target = root.pick(x, y, gtk::PickFlags::DEFAULT);
        for popover in weak.iter().filter_map(glib::WeakRef::upgrade) {
            if !popover.is_visible() {
                continue;
            }
            let on_anchor = popover
                .parent()
                .zip(target.as_ref())
                .is_some_and(|(anchor, target)| target == &anchor || target.is_ancestor(&anchor));
            if !on_anchor {
                popover.popdown();
            }
        }
    });
    root.add_controller(click);
}

/// Clicking a control must not leave the keyboard focus on it: Space would
/// then press that button again instead of pausing. Keyboard users still
/// reach every control with Tab.
pub(super) fn keep_focus_off_clicks(widget: &gtk::Widget) {
    widget.set_focus_on_click(false);
    let mut child = widget.first_child();
    while let Some(current) = child {
        keep_focus_off_clicks(&current);
        child = current.next_sibling();
    }
}
