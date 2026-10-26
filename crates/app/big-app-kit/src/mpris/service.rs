use glib::variant::ToVariant;
use gtk::{gio, glib};
use log::{info, warn};
use std::rc::Rc;
use std::sync::Arc;

use super::properties::{
    get_player_property, get_root_property, handle_player_method, set_player_property,
};
use super::sync::Publisher;
use super::types::{CommandCallback, MprisCommand, StateCallback};
use super::{MPRIS_PATH, MPRIS_PLAYER_IFACE, PLAYER_XML, ROOT_XML};

/// Opens a session-bus connection owned by one service. Two players in one
/// process would collide on `MPRIS_PATH` on the shared connection, and closing
/// it in Drop releases the name and the exported objects together.
pub(crate) fn private_session_bus() -> Result<gio::DBusConnection, glib::Error> {
    let address =
        gio::dbus_address_get_for_bus_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
    gio::DBusConnection::for_address_sync(
        &address,
        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
        None,
        gio::Cancellable::NONE,
    )
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
    /// Released in Drop before the connection closes.
    owner_id: Option<gio::OwnerId>,
    publisher: Rc<Publisher>,
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
        let connection = match private_session_bus() {
            Ok(connection) => connection,
            Err(e) => {
                warn!("MPRIS: failed to connect a private session bus: {e}");
                return None;
            }
        };
        let command_cb = Arc::new(command_cb);
        let state_cb = Arc::new(state_cb);
        let exported = export_root(&connection, command_cb.clone(), desktop_id)
            .and_then(|()| export_player(&connection, &command_cb, &state_cb));
        if let Err(e) = exported {
            warn!("MPRIS: failed to export {e}");
            let _ = connection.close_sync(gio::Cancellable::NONE);
            return None;
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
            taskbar_progress_enabled,
        ));
        publisher.sync();

        info!("MPRIS2 service started on {bus_name}");
        Some(Self {
            owner_id: Some(owner_id),
            publisher,
        })
    }

    /// Publish what changed in the state the callback reads; the owner calls
    /// this after changing it. While playing, the next tick publishes instead,
    /// within a second.
    pub fn sync(&self) {
        self.publisher.sync();
    }

    /// Show or hide the playback progress on the taskbar entry.
    pub fn set_taskbar_progress(&self, enabled: bool) {
        self.publisher.taskbar.set(enabled);
        self.publisher.publish();
    }

    /// Emit the seeked signal on the active D-Bus connection.
    pub fn emit_seeked(&self, position_us: i64) {
        if let Err(error) = self.publisher.connection.emit_signal(
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
        // Unown before closing: a closed connection fires name-lost with a
        // NULL connection, which aborts in gio's closure trampoline.
        if let Some(owner_id) = self.owner_id.take() {
            gio::bus_unown_name(owner_id);
        }
        let _ = self.publisher.connection.close_sync(gio::Cancellable::NONE);
    }
}
