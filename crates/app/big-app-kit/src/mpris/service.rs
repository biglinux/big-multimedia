use glib::variant::ToVariant;
use gtk::{gio, glib};
use log::{info, warn};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::properties::{
    get_player_property, get_root_property, handle_player_method, set_player_property,
};
use super::signals::emit_launcher_entry;
use super::sync::sync_state;
use super::types::{
    CommandCallback, MprisCommand, PlayerState, StateCallback, previous_state, taskbar_flag,
};
use super::{MPRIS_PATH, MPRIS_PLAYER_IFACE, PLAYER_XML, ROOT_XML};

#[derive(Debug, Clone, PartialEq)]
struct LauncherEntryUpdate {
    desktop_uri: String,
    is_visible: bool,
    progress: f64,
}

fn root_method_command(method: &str) -> Option<MprisCommand> {
    match method {
        "Raise" => Some(MprisCommand::Raise),
        "Quit" => Some(MprisCommand::Quit),
        _ => None,
    }
}

fn private_session_connection_flags() -> gio::DBusConnectionFlags {
    gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
        .union(gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION)
}

fn desktop_application_uri(desktop_id: &str) -> String {
    format!("application://{desktop_id}.desktop")
}

fn hidden_launcher_entry_update(desktop_id: &str) -> LauncherEntryUpdate {
    LauncherEntryUpdate {
        desktop_uri: desktop_application_uri(desktop_id),
        is_visible: false,
        progress: 0.0,
    }
}

fn first_dbus_interface<'a>(
    node: &'a gio::DBusNodeInfo,
    interface_label: &str,
) -> Option<&'a gio::DBusInterfaceInfo> {
    match node.interfaces().iter().next() {
        Some(interface) => Some(interface),
        None => {
            warn!("MPRIS: {interface_label} XML has no interface");
            None
        }
    }
}

/// Sends the player's changes as D-Bus signals.
struct Publisher {
    connection: gio::DBusConnection,
    state: Arc<StateCallback>,
    previous: Arc<Mutex<Option<PlayerState>>>,
    taskbar: Arc<AtomicBool>,
    desktop_uri: String,
    sent_launcher: Cell<Option<Option<f64>>>,
    /// The once-a-second tick, only while playing: it publishes every change
    /// then, the taskbar progress included, so a player sending ten position
    /// updates a second does not compare the whole state ten times.
    tick: RefCell<Option<glib::SourceId>>,
}

impl Publisher {
    /// Publish what changed; returns whether the player plays.
    fn publish(&self) -> bool {
        sync_state(
            &self.connection,
            &self.state,
            &self.previous,
            &self.taskbar,
            &self.desktop_uri,
            &self.sent_launcher,
        )
    }

    fn sync(self: &Rc<Self>) {
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

/// Mpris.
pub struct Mpris {
    /// Owned bus name; released explicitly in Drop BEFORE the connection
    /// closes (a close-first order fires name-lost with a NULL connection,
    /// which aborts inside gio's FFI closure trampoline).
    owner_id: Option<gio::OwnerId>,
    connection: gio::DBusConnection,
    publisher: Rc<Publisher>,
    taskbar_progress: Arc<AtomicBool>,
    desktop_id: String,
}

impl Mpris {
    /// Build the MPRIS service with the BigVideoPlayer bus name and
    /// desktop id. Returns `None` if the bus connection cannot be
    /// established (no session bus, name taken, ...).
    pub fn new(command_cb: CommandCallback, state_cb: StateCallback) -> Option<Self> {
        Self::new_with_name(
            "org.mpris.MediaPlayer2.BigVideoPlayer",
            "br.com.biglinux.VideoPlayer",
            command_cb,
            state_cb,
            false,
        )
    }

    /// Build the MPRIS service with caller-supplied bus name and
    /// desktop id; used by apps that need a different identity than
    /// BigVideoPlayer. Returns `None` if the bus connection fails.
    pub fn new_with_name(
        bus_name: &str,
        desktop_id: &str,
        command_cb: CommandCallback,
        state_cb: StateCallback,
        taskbar_progress_enabled: bool,
    ) -> Option<Self> {
        let root_node = match gio::DBusNodeInfo::for_xml(ROOT_XML) {
            Ok(info) => info,
            Err(e) => {
                warn!("MPRIS: failed to parse root XML: {e}");
                return None;
            }
        };
        let player_node = match gio::DBusNodeInfo::for_xml(PLAYER_XML) {
            Ok(info) => info,
            Err(e) => {
                warn!("MPRIS: failed to parse player XML: {e}");
                return None;
            }
        };
        let root_iface = first_dbus_interface(&root_node, "root")?;
        let player_iface = first_dbus_interface(&player_node, "player")?;

        // PRIVATE session-bus connection per service instance. The shared
        // process connection (`bus_get_sync`) broke multicall hosts twice:
        // the interfaces stayed exported after the window closed (reopen →
        // "object already exported"), and two players in one process would
        // collide on the same /org/mpris/MediaPlayer2 path. A private
        // connection gives each module its own bus identity, and closing it
        // in Drop releases the name AND the exported objects atomically.
        let address =
            match gio::dbus_address_get_for_bus_sync(gio::BusType::Session, gio::Cancellable::NONE)
            {
                Ok(address) => address,
                Err(e) => {
                    warn!("MPRIS: failed to resolve session bus address: {e}");
                    return None;
                }
            };
        let connection = match gio::DBusConnection::for_address_sync(
            &address,
            private_session_connection_flags(),
            None,
            gio::Cancellable::NONE,
        ) {
            Ok(c) => c,
            Err(e) => {
                warn!("MPRIS: failed to connect a private session bus: {e}");
                return None;
            }
        };

        let command_cb = Arc::new(command_cb);
        let state_cb = Arc::new(state_cb);
        let prev_state = previous_state();

        {
            let cmd = command_cb.clone();
            let desktop_entry = desktop_id.to_string();
            let result = connection
                .register_object(MPRIS_PATH, root_iface)
                .method_call(
                    move |_conn, _sender, _path, _interface, method, _params, invocation| {
                        if let Some(command) = root_method_command(method) {
                            cmd(command);
                        } else {
                            invocation.return_dbus_error(
                                "org.freedesktop.DBus.Error.UnknownMethod",
                                "Unknown root method",
                            );
                            return;
                        }
                        invocation.return_value(None);
                    },
                )
                .property(
                    move |_conn, _sender, _path, _interface, property| -> glib::Variant {
                        get_root_property(property, &desktop_entry)
                            .unwrap_or_else(|| false.to_variant())
                    },
                )
                // Fullscreen cannot change for this service; GDBus handles
                // unknown properties through introspection before this callback.
                .set_property(move |_, _, _, _, _, _| false)
                .build();
            if let Err(e) = result {
                warn!("MPRIS: failed to register root interface: {e}");
                let _ = connection.close_sync(gio::Cancellable::NONE);
                return None;
            }
        }

        {
            let cmd = command_cb.clone();
            let st_for_prop = state_cb.clone();
            let cmd_for_set = command_cb.clone();
            let state_for_method = state_cb.clone();
            let state_for_set = state_cb.clone();

            let result = connection
                .register_object(MPRIS_PATH, player_iface)
                .method_call(
                    move |_conn, _sender, _path, _interface, method, params, invocation| {
                        handle_player_method(method, params, invocation, &cmd, &state_for_method);
                    },
                )
                .property(
                    move |_conn, _sender, _path, _interface, property| -> glib::Variant {
                        get_player_property(property, &st_for_prop)
                            .unwrap_or_else(|| false.to_variant())
                    },
                )
                .set_property(
                    move |_conn, _sender, _path, _interface, property, value| -> bool {
                        set_player_property(property, &value, &cmd_for_set, &state_for_set)
                    },
                )
                .build();
            if let Err(e) = result {
                warn!("MPRIS: failed to register player interface: {e}");
                let _ = connection.close_sync(gio::Cancellable::NONE);
                return None;
            }
        }

        let owner_id = gio::bus_own_name_on_connection(
            &connection,
            bus_name,
            gio::BusNameOwnerFlags::NONE,
            |_, _| {},
            |_, _| {},
        );

        let taskbar_flag = taskbar_flag(taskbar_progress_enabled);
        let publisher = Rc::new(Publisher {
            connection: connection.clone(),
            state: state_cb,
            previous: prev_state,
            taskbar: taskbar_flag.clone(),
            desktop_uri: desktop_application_uri(desktop_id),
            sent_launcher: Cell::new(None),
            tick: RefCell::new(None),
        });
        publisher.sync();

        info!("MPRIS2 service started on {bus_name}");
        Some(Self {
            owner_id: Some(owner_id),
            connection,
            publisher,
            taskbar_progress: taskbar_flag,
            desktop_id: desktop_id.to_string(),
        })
    }

    /// Publish what changed in the state the callback reads; the owner calls
    /// this after changing it. While playing, the next tick publishes instead,
    /// within a second.
    pub fn sync(&self) {
        self.publisher.sync();
    }

    /// Update the taskbar progress field in place.
    pub fn set_taskbar_progress(&self, enabled: bool) {
        self.taskbar_progress.store(enabled, Ordering::Relaxed);
        if !enabled {
            let update = hidden_launcher_entry_update(&self.desktop_id);
            emit_launcher_entry(
                &self.connection,
                &update.desktop_uri,
                update.is_visible,
                update.progress,
            );
        }
    }

    /// Emit the seeked signal on the active D-Bus connection.
    pub fn emit_seeked(&self, position_us: i64) {
        if let Err(error) = self.connection.emit_signal(
            None::<&str>,
            MPRIS_PATH,
            MPRIS_PLAYER_IFACE,
            "Seeked",
            Some(&(position_us,).to_variant()),
        ) {
            log::debug!("MPRIS: Seeked not sent: {error}");
        }
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        if let Some(tick) = self.publisher.tick.take() {
            big_gtk_kit::source::remove(tick);
        }
        // Order matters: release the bus name FIRST (with the connection
        // alive), THEN close the private connection. Closing first makes
        // GLib fire the name-lost callback with a NULL connection, which
        // panics in gio's closure trampoline: a non-unwinding abort.
        if let Some(owner_id) = self.owner_id.take() {
            gio::bus_unown_name(owner_id);
        }
        let _ = self.connection.close_sync(gio::Cancellable::NONE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_known_roots_dispatch() {
        assert_eq!(root_method_command("Raise"), Some(MprisCommand::Raise));
        assert_eq!(root_method_command("Bogus"), None);
    }
    #[test]
    fn private_bus_flags_are_preserved() {
        assert!(
            private_session_connection_flags()
                .contains(gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION)
        );
    }
}
