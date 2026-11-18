use glib::variant::ToVariant;
use gtk::{gio, glib};
use log::warn;
use std::cell::{Cell, OnceCell};
use std::rc::Rc;
use std::sync::Arc;

use super::properties::{
    get_player_property, get_root_property, handle_player_method, set_player_property,
};
use super::sync::Publisher;
use super::types::{CommandCallback, MprisCommand, StateCallback};
use super::{MPRIS_PATH, MPRIS_PLAYER_IFACE, PLAYER_XML, ROOT_XML};

/// Connects a session-bus connection owned by one service, off the main loop.
/// Two players in one process would collide on `MPRIS_PATH` on the shared
/// connection, and closing it releases the name and the exported objects together.
pub(crate) async fn private_session_bus() -> Result<gio::DBusConnection, glib::Error> {
    // Without an address in the environment, gio may start a helper to find one.
    let address = gio::spawn_blocking(|| {
        gio::dbus_address_get_for_bus_sync(gio::BusType::Session, gio::Cancellable::NONE)
    })
    .await
    .map_err(|_| glib::Error::new(gio::IOErrorEnum::Failed, "session bus lookup panicked"))??;
    gio::DBusConnection::for_address_future(
        &address,
        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
        None,
    )
    .await
}

fn export_root(
    connection: &gio::DBusConnection,
    command: Arc<CommandCallback>,
    desktop_id: &str,
) -> Result<(), String> {
    let node = gio::DBusNodeInfo::for_xml(ROOT_XML).map_err(|e| format!("root XML: {e}"))?;
    let interface = node
        .interfaces()
        .iter()
        .next()
        .ok_or("root XML has no interface")?;
    let desktop_id = desktop_id.to_owned();
    connection
        .register_object(MPRIS_PATH, interface)
        .method_call(move |_, _, _, _, method, _, invocation| {
            let request = match method {
                "Raise" => MprisCommand::Raise,
                "Quit" => MprisCommand::Quit,
                _ => {
                    invocation.return_dbus_error(
                        "org.freedesktop.DBus.Error.UnknownMethod",
                        "Unknown root method",
                    );
                    return;
                }
            };
            command(request);
            invocation.return_value(None);
        })
        .property(move |_, _, _, _, property| {
            get_root_property(property, &desktop_id).unwrap_or_else(|| false.to_variant())
        })
        // Fullscreen cannot change for this service; GDBus handles
        // unknown properties through introspection before this callback.
        .set_property(|_, _, _, _, _, _| false)
        .build()
        .map(drop)
        .map_err(|e| format!("root interface: {e}"))
}

fn export_player(
    connection: &gio::DBusConnection,
    command: &Arc<CommandCallback>,
    state: &Arc<StateCallback>,
) -> Result<(), String> {
    let node = gio::DBusNodeInfo::for_xml(PLAYER_XML).map_err(|e| format!("player XML: {e}"))?;
    let interface = node
        .interfaces()
        .iter()
        .next()
        .ok_or("player XML has no interface")?;
    let (method_command, method_state) = (command.clone(), state.clone());
    let property_state = state.clone();
    let (set_command, set_state) = (command.clone(), state.clone());
    connection
        .register_object(MPRIS_PATH, interface)
        .method_call(move |_, _, _, _, method, params, invocation| {
            handle_player_method(method, params, invocation, &method_command, &method_state);
        })
        .property(move |_, _, _, _, property| {
            get_player_property(property, &property_state).unwrap_or_else(|| false.to_variant())
        })
        .set_property(move |_, _, _, _, property, value| {
            set_player_property(property, &value, &set_command, &set_state)
        })
        .build()
        .map(drop)
        .map_err(|e| format!("player interface: {e}"))
}

/// MPRIS2 player exported on a private session-bus connection.
pub struct Mpris {
    /// Set once the connection is up and the objects are exported.
    exported: Rc<OnceCell<ExportedPlayer>>,
    taskbar: Rc<Cell<bool>>,
    /// Aborted in Drop, which cancels a connection still being made.
    connecting: glib::JoinHandle<()>,
}

struct ExportedPlayer {
    /// Released in Drop before the connection closes.
    owner_id: Option<gio::OwnerId>,
    publisher: Rc<Publisher>,
}

impl ExportedPlayer {
    /// Export both objects on `connection`, take `bus_name` and publish the
    /// first state; the connection is closed when exporting fails.
    fn start(
        connection: gio::DBusConnection,
        bus_name: &str,
        desktop_id: &str,
        command_cb: CommandCallback,
        state_cb: StateCallback,
        taskbar: Rc<Cell<bool>>,
    ) -> Result<Self, String> {
        let command_cb = Arc::new(command_cb);
        let state_cb = Arc::new(state_cb);
        let exported = export_root(&connection, command_cb.clone(), desktop_id)
            .and_then(|()| export_player(&connection, &command_cb, &state_cb));
        if let Err(e) = exported {
            connection.close(gio::Cancellable::NONE, |_| {});
            return Err(e);
        }
        let owner_id = gio::bus_own_name_on_connection(
            &connection,
            bus_name,
            gio::BusNameOwnerFlags::NONE,
            |_, _| {},
            |_, _| {},
        );
        let publisher = Rc::new(Publisher::new(
            connection,
            state_cb,
            format!("application://{desktop_id}.desktop"),
            taskbar,
        ));
        publisher.sync();
        Ok(Self {
            owner_id: Some(owner_id),
            publisher,
        })
    }
}

impl Mpris {
    /// Build the MPRIS service under `bus_name` for the app whose desktop
    /// entry is `desktop_id`. The bus connection is made in the background; a
    /// failure is logged and leaves the player without MPRIS.
    pub fn new(
        bus_name: &str,
        desktop_id: &str,
        command_cb: CommandCallback,
        state_cb: StateCallback,
        taskbar_progress_enabled: bool,
    ) -> Self {
        let exported = Rc::new(OnceCell::new());
        let taskbar = Rc::new(Cell::new(taskbar_progress_enabled));
        let slot = Rc::downgrade(&exported);
        let publisher_taskbar = taskbar.clone();
        let (bus_name, desktop_id) = (bus_name.to_owned(), desktop_id.to_owned());
        let connecting = glib::spawn_future_local(async move {
            let connection = match private_session_bus().await {
                Ok(connection) => connection,
                Err(e) => {
                    warn!("MPRIS: failed to connect a private session bus: {e}");
                    return;
                }
            };
            let Some(slot) = slot.upgrade() else {
                return;
            };
            match ExportedPlayer::start(
                connection,
                &bus_name,
                &desktop_id,
                command_cb,
                state_cb,
                publisher_taskbar,
            ) {
                Ok(player) => {
                    let _ = slot.set(player);
                }
                Err(e) => warn!("MPRIS: failed to export {e}"),
            }
        });
        Self {
            exported,
            taskbar,
            connecting,
        }
    }

    /// Publish what changed in the state the callback reads; the owner calls
    /// this after changing it. While playing, the next tick publishes instead,
    /// within a second.
    pub fn sync(&self) {
        if let Some(exported) = self.exported.get() {
            exported.publisher.sync();
        }
    }

    /// Show or hide the playback progress on the taskbar entry.
    pub fn set_taskbar_progress(&self, enabled: bool) {
        self.taskbar.set(enabled);
        if let Some(exported) = self.exported.get() {
            exported.publisher.publish();
        }
    }

    /// Emit the seeked signal on the active D-Bus connection, for a position in seconds.
    pub fn emit_seeked(&self, position_secs: f64) {
        let position_us = super::types::micros(position_secs);
        let Some(exported) = self.exported.get() else {
            return;
        };
        if let Err(error) = exported.publisher.connection.emit_signal(
            None::<&str>,
            MPRIS_PATH,
            MPRIS_PLAYER_IFACE,
            "Seeked",
            Some(&(position_us,).to_variant()),
        ) {
            log::warn!("MPRIS: Seeked not sent: {error}");
        }
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        self.connecting.abort();
    }
}

impl Drop for ExportedPlayer {
    fn drop(&mut self) {
        if let Some(tick) = self.publisher.tick.take() {
            big_gtk_kit::source::remove(tick);
        }
        // Unown before closing: a closed connection fires name-lost with a
        // NULL connection, which aborts in gio's closure trampoline.
        if let Some(owner_id) = self.owner_id.take() {
            gio::bus_unown_name(owner_id);
        }
        self.publisher
            .connection
            .close(gio::Cancellable::NONE, |_| {});
    }
}
