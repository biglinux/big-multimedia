use super::{
    properties::{CHANGING, player_property},
    signals::{emit_launcher_entry, emit_properties_changed},
    types::{PlaybackStatus, PlayerState, StateCallback},
};
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

/// Sends the player's changes as D-Bus signals.
pub(super) struct Publisher {
    pub(super) connection: gio::DBusConnection,
    pub(super) state: Arc<StateCallback>,
    pub(super) desktop_uri: String,
    pub(super) taskbar: Cell<bool>,
    previous: RefCell<Option<PlayerState>>,
    sent_launcher: Cell<Option<Option<f64>>>,
    /// The once-a-second tick, only while playing: it publishes every change
    /// then, the taskbar progress included, so a player sending ten position
    /// updates a second does not compare the whole state ten times.
    pub(super) tick: RefCell<Option<glib::SourceId>>,
}

impl Publisher {
    pub(super) fn new(
        connection: gio::DBusConnection,
        state: Arc<StateCallback>,
        desktop_uri: String,
        taskbar: bool,
    ) -> Self {
        Self {
            connection,
            state,
            desktop_uri,
            taskbar: Cell::new(taskbar),
            previous: RefCell::new(None),
            sent_launcher: Cell::new(None),
            tick: RefCell::new(None),
        }
    }

    /// Publishes what changed since the last call; returns whether the player
    /// plays, when the position keeps moving. Every property is compared
    /// through the same getter D-Bus reads, so no signal runs ahead of the state.
    pub(super) fn publish(&self) -> bool {
        let next = (self.state)();
        let mut previous = self.previous.borrow_mut();
        let dict = glib::VariantDict::new(None);
        let mut changed = false;
        for name in CHANGING {
            let value = player_property(name, &next);
            if previous.as_ref().and_then(|old| player_property(name, old)) != value
                && let Some(value) = value
            {
                dict.insert_value(name, &value);
                changed = true;
            }
        }
        if changed {
            emit_properties_changed(&self.connection, dict.end());
        }
        let launcher = launcher_progress(self.taskbar.get(), &next);
        // Every signal wakes the session bus and the desktop's task manager.
        if self.sent_launcher.replace(Some(launcher)) != Some(launcher) {
            emit_launcher_entry(
                &self.connection,
                &self.desktop_uri,
                launcher.is_some(),
                launcher.unwrap_or(0.0),
            );
        }
        let playing = next.status == PlaybackStatus::Playing;
        *previous = Some(next);
        playing
    }

    /// Publishes now, then once a second while playing.
    pub(super) fn sync(self: &Rc<Self>) {
        if self.tick.borrow().is_some() || !self.publish() {
            return;
        }
        let publisher = Rc::downgrade(self);
        let tick = glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            let Some(publisher) = publisher.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if publisher.publish() {
                return glib::ControlFlow::Continue;
            }
            publisher.tick.take();
            glib::ControlFlow::Break
        });
        self.tick.replace(Some(tick));
    }
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
