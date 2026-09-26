// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Panic containment at the Relm4/glib callback boundary.
//!
//! - A panic inside a Relm4 `update()` unwinds into the glib executor task and
//!   dies there: the process survives, but the component runtime is dead and
//!   its window stays orphaned on screen. [`contain_component_update`](crate::containment::contain_component_update) logs it
//!   and destroys the window cleanly instead.
//! - A panic that escapes a raw glib callback (timeout / signal trampoline)
//!   unwinds across the C boundary and aborts the process. A panic caught
//!   inside the callback body is contained; [`guard`](crate::containment::guard) is that catch.

use std::panic::{AssertUnwindSafe, catch_unwind};

use gtk::glib;
use gtk::prelude::{Cast, CastNone, GtkWindowExt, IsA, WidgetExt};

/// Run one `update()` body, converting a panic into a clean component
/// teardown: the error is logged and the component's window is destroyed on
/// the next idle (never during the unwind).
pub fn contain_component_update(
    window: &impl IsA<gtk::Window>,
    component_label: &str,
    update: impl FnOnce(),
) {
    if catch_unwind(AssertUnwindSafe(update)).is_ok() {
        return;
    }
    log::error!(
        "component '{component_label}' panicked in update(); \
         destroying its window"
    );
    let window = window.clone().upcast::<gtk::Window>();
    glib::idle_add_local_once(move || window.destroy());
}

/// Like [`contain_component_update`] but for a component whose `Root` is not
/// itself a [`gtk::Window`] (e.g. a `ToastOverlay` or `Box`): it derives the
/// toplevel window from `root_widget`. If the widget is not yet rooted, it
/// falls back to logging only (still containing the panic).
pub fn contain_embedded_update(
    root_widget: &impl IsA<gtk::Widget>,
    component_label: &str,
    update: impl FnOnce(),
) {
    if let Some(window) = root_widget.root().and_downcast::<gtk::Window>() {
        contain_component_update(&window, component_label, update);
    } else if catch_unwind(AssertUnwindSafe(update)).is_err() {
        log::error!(
            "component '{component_label}' panicked in update() before it was rooted; \
             panic contained"
        );
    }
}

/// Run a raw glib callback body (signal / `idle_add` / `timeout_add` closure),
/// containing any panic: it is logged and `None` is returned instead of
/// unwinding across the C trampoline (which would abort the process). `context` names the call site for the log line.
///
/// Place this INSIDE the closure, around the fallible body, and map `None` to a
/// safe default for the signal's return type (e.g. `glib::Propagation::Proceed`,
/// `glib::ControlFlow::Break`, `true` for a filter). The default panic hook
/// still runs first, so the message + location reach the journal.
///
/// `AssertUnwindSafe`: GTK widgets and `RefCell`s are not `UnwindSafe`, but a
/// contained panic only skips one callback — every borrow guard releases as the
/// stack unwinds, so the shared state stays usable afterwards (at worst with a
/// half-applied update, still strictly better than a dead process).
#[inline]
pub fn guard<R>(context: &str, body: impl FnOnce() -> R) -> Option<R> {
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(value) => Some(value),
        Err(_) => {
            log::error!("contained a panic in `{context}`; action skipped");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::guard;

    #[test]
    fn returns_value_when_no_panic() {
        assert_eq!(guard("ok", || 7), Some(7));
    }

    #[test]
    fn returns_none_and_releases_borrow_when_body_panics() {
        let cell = RefCell::new(0);
        let outcome = guard("boom", || {
            let mut value = cell.borrow_mut();
            *value += 1;
            panic!("widget bug");
            #[allow(unreachable_code)]
            *value
        });
        assert_eq!(outcome, None);
        // The borrow released as the stack unwound — the cell is usable again.
        assert_eq!(*cell.borrow(), 1);
    }
}
