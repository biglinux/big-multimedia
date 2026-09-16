use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use gtk::{gio, glib};
use super::{properties::{CHANGING, player_property}, signals::{emit_properties_changed, emit_launcher_entry}, types::{PlayerState, StateCallback, PlaybackStatus}};
/// The snapshot owner is authoritative. Every published property is compared
/// through the same getter used by D-Bus; there are no optimistic signal paths.
pub(super) fn sync_state(connection: &gio::DBusConnection, callback: &StateCallback,
    previous: &Arc<Mutex<Option<PlayerState>>>, taskbar: &AtomicBool, desktop_uri: &str) {
    let next = callback();
    let mut old = previous.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dict = glib::VariantDict::new(None); let mut changed = false;
    for name in CHANGING {
        let value = player_property(name, &next);
        if old.as_ref().and_then(|old| player_property(name, old)) != value {
            if let Some(value) = value { dict.insert_value(name, &value); changed = true; }
        }
    }
    if changed { emit_properties_changed(connection, dict.end()); }
    let visible = taskbar.load(Ordering::Relaxed) && next.status == PlaybackStatus::Playing
        && next.position_secs.is_finite() && next.duration_secs.is_finite() && next.duration_secs > 0.0;
    emit_launcher_entry(connection, desktop_uri, visible,
        if visible { (next.position_secs / next.duration_secs).clamp(0.0, 1.0) } else { 0.0 });
    *old = Some(next);
}
