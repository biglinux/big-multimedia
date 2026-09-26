// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Off-main-thread task helpers for GTK apps.
//!
//! Run blocking work (directory walks, subprocess spawns, network requests)
//! off the GTK main loop and deliver the result back on it, so the UI never
//! freezes. Shared so apps don't hand-roll `gio::spawn_blocking` +
//! `spawn_future_local` at every call site.
//!
//! [`spawn_blocking_result`] is fire-and-forget: its `on_result` runs on the
//! main loop even after the owning component has been torn down, so callers
//! must only touch state that outlives the task (weak references).

use relm4::gtk;

/// Run `task` off the GTK main thread, then deliver its result to `on_result`
/// back ON the main thread.
///
/// `task` must be `Send` (it crosses to a worker thread); `on_result` need not
/// be — it runs on the GLib main loop, so it can touch `!Send` GTK objects and
/// `Rc` UI state. Use a trailing-edge debouncer
/// ([`crate::input::debounce::BigDebouncedAction`]) before this for slider /
/// spin-button storms, and a generation/stale guard in `on_result` when rapid
/// input can supersede an in-flight task.
pub fn spawn_blocking_result<T, B, U>(task: B, on_result: U)
where
    T: Send + 'static,
    B: FnOnce() -> T + Send + 'static,
    U: FnOnce(T) + 'static,
{
    gtk::glib::spawn_future_local(async move {
        if let Ok(value) = gtk::gio::spawn_blocking(task).await {
            on_result(value);
        }
    });
}

/// Fire-and-forget variant of [`spawn_blocking_result`] — runs `task` off the
/// main thread and discards the result.
pub fn spawn_blocking_task<B>(task: B)
where
    B: FnOnce() + Send + 'static,
{
    spawn_blocking_result(task, |()| {});
}
