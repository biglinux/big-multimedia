use super::{
    properties::{CHANGING, player_property},
    signals::{emit_launcher_entry, emit_properties_changed},
    types::{PlaybackStatus, PlayerState, StateCallback},
};
use gtk::{gio, glib};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
/// The snapshot owner is authoritative. Every published property is compared
/// through the same getter used by D-Bus; there are no optimistic signal paths.
/// Returns whether the player is playing, when the position keeps moving.
pub(super) fn sync_state(
    connection: &gio::DBusConnection,
    callback: &StateCallback,
    previous: &Arc<Mutex<Option<PlayerState>>>,
    taskbar: &AtomicBool,
    desktop_uri: &str,
    sent_launcher: &std::cell::Cell<Option<Option<f64>>>,
) -> bool {
    let next = callback();
    let mut old = previous
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dict = glib::VariantDict::new(None);
    let mut changed = false;
    for name in CHANGING {
        let value = player_property(name, &next);
        if old.as_ref().and_then(|old| player_property(name, old)) != value
            && let Some(value) = value
        {
            dict.insert_value(name, &value);
            changed = true;
        }
    }
    if changed {
        emit_properties_changed(connection, dict.end());
    }
    let launcher = launcher_progress(taskbar.load(Ordering::Relaxed), &next);
    // A signal every second wakes the session bus and every listener (the
    // desktop's task manager) even while nothing plays: send changes only.
    if sent_launcher.replace(Some(launcher)) != Some(launcher) {
        emit_launcher_entry(
            connection,
            desktop_uri,
            launcher.is_some(),
            launcher.unwrap_or(0.0),
        );
    }
    let playing = next.status == PlaybackStatus::Playing;
    *old = Some(next);
    playing
}

/// The taskbar progress for `state`, or `None` when the bar is hidden.
fn launcher_progress(show_taskbar: bool, state: &PlayerState) -> Option<f64> {
    (show_taskbar
        && state.status == PlaybackStatus::Playing
        && state.position_secs.is_finite()
        && state.duration_secs.is_finite()
        && state.duration_secs > 0.0)
        .then(|| (state.position_secs / state.duration_secs).clamp(0.0, 1.0))
}
