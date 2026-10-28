// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! System tray icon: a StatusNotifierItem with a `com.canonical.dbusmenu`
//! menu, the protocol of KDE Plasma's tray and GNOME's AppIndicator extension.
//!
//! The item lives on its own session-bus connection, like the MPRIS service:
//! dropping [`Tray`] closes it, and the tray host removes the icon when the
//! connection's name vanishes.

mod menu;

use glib::variant::{ObjectPath, ToVariant};
use gtk::{gio, glib};
use log::{debug, info, warn};
use std::rc::Rc;

const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

const ITEM_XML: &str = include_str!("item.xml");
const MENU_XML: &str = include_str!("menu.xml");

/// One entry of the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayMenuItem {
    /// A clickable entry; clicking it sends [`TrayCommand::Menu`] with `action`.
    Action {
        /// Name the owner matches in [`TrayCommand::Menu`].
        action: String,
        /// Text shown in the menu.
        label: String,
        /// Optional themed icon next to the label.
        icon_name: Option<String>,
    },
    /// A line between groups of entries.
    Separator,
}

impl TrayMenuItem {
    /// A clickable entry without an icon.
    pub fn action(action: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Action {
            action: action.into(),
            label: label.into(),
            icon_name: None,
        }
    }

    /// The same entry with a themed icon.
    #[must_use]
    pub fn with_icon(self, icon: impl Into<String>) -> Self {
        match self {
            Self::Action { action, label, .. } => Self::Action {
                action,
                label,
                icon_name: Some(icon.into()),
            },
            Self::Separator => Self::Separator,
        }
    }
}

/// What the tray icon shows.
#[derive(Debug, Clone)]
pub struct TraySpec {
    /// Stable application id, e.g. the binary name.
    pub id: String,
    /// Name shown by the host, also the tooltip.
    pub title: String,
    /// Themed icon; a `-symbolic` name follows the panel's colors.
    pub icon_name: String,
    /// The menu opened by a right click.
    pub items: Vec<TrayMenuItem>,
}

/// A request from the tray host, delivered on the GTK main thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayCommand {
    /// Primary (left) click on the icon.
    Activate,
    /// Secondary (middle) click on the icon.
    SecondaryActivate,
    /// A menu entry was clicked: its [`TrayMenuItem::Action`] `action`.
    Menu(String),
}

/// Receives the host's requests.
pub type TrayCallback = Rc<dyn Fn(TrayCommand)>;

fn interface(xml: &str) -> Result<gio::DBusInterfaceInfo, String> {
    let node = gio::DBusNodeInfo::for_xml(xml).map_err(|e| e.to_string())?;
    node.interfaces()
        .iter()
        .next()
        .cloned()
        .ok_or_else(|| "XML has no interface".to_owned())
}

fn no_pixmaps() -> glib::Variant {
    Vec::<(i32, i32, Vec<u8>)>::new().to_variant()
}

fn item_property(spec: &TraySpec, property: &str) -> Option<glib::Variant> {
    Some(match property {
        "Category" => "ApplicationStatus".to_variant(),
        "Id" => spec.id.to_variant(),
        "Title" => spec.title.to_variant(),
        "Status" => "Active".to_variant(),
        "WindowId" => 0_i32.to_variant(),
        "IconName" => spec.icon_name.to_variant(),
        "IconThemePath" | "OverlayIconName" | "AttentionIconName" | "AttentionMovieName" => {
            "".to_variant()
        }
        "IconPixmap" | "OverlayIconPixmap" | "AttentionIconPixmap" => no_pixmaps(),
        "ToolTip" => (
            String::new(),
            Vec::<(i32, i32, Vec<u8>)>::new(),
            spec.title.clone(),
            String::new(),
        )
            .to_variant(),
        "ItemIsMenu" => false.to_variant(),
        "Menu" => ObjectPath::try_from(MENU_PATH).ok()?.to_variant(),
        _ => return None,
    })
}

fn export_item(
    connection: &gio::DBusConnection,
    spec: &Rc<TraySpec>,
    command: &TrayCallback,
) -> Result<(), String> {
    let (spec, command) = (spec.clone(), command.clone());
    connection
        .register_object(ITEM_PATH, &interface(ITEM_XML)?)
        .method_call(move |_, _, _, _, method, _, invocation| {
            match method {
                "Activate" => command(TrayCommand::Activate),
                "SecondaryActivate" => command(TrayCommand::SecondaryActivate),
                // The host opens the menu itself; scrolling does nothing.
                "ContextMenu" | "Scroll" => {}
                _ => {
                    invocation.return_dbus_error(
                        "org.freedesktop.DBus.Error.UnknownMethod",
                        "Unknown StatusNotifierItem method",
                    );
                    return;
                }
            }
            invocation.return_value(None);
        })
        .property(move |_, _, _, _, property| {
            item_property(&spec, property).unwrap_or_else(|| "".to_variant())
        })
        .build()
        .map(drop)
        .map_err(|e| format!("item interface: {e}"))
}

fn menu_property(property: &str) -> glib::Variant {
    match property {
        "Version" => 3_u32.to_variant(),
        "TextDirection" => "ltr".to_variant(),
        "Status" => "normal".to_variant(),
        _ => Vec::<String>::new().to_variant(),
    }
}

fn handle_menu_method(
    items: &[TrayMenuItem],
    method: &str,
    params: &glib::Variant,
    command: &TrayCallback,
) -> Result<Option<glib::Variant>, &'static str> {
    let invalid = "Invalid arguments";
    match method {
        "GetLayout" => {
            let (parent, depth, names) = params.get::<(i32, i32, Vec<String>)>().ok_or(invalid)?;
            let node = menu::layout(items, parent, depth, &names).ok_or("Unknown menu id")?;
            Ok(Some((menu::REVISION, node).to_variant()))
        }
        "GetGroupProperties" => {
            let (ids, names) = params.get::<(Vec<i32>, Vec<String>)>().ok_or(invalid)?;
            let found: Vec<_> = ids
                .into_iter()
                .filter_map(|id| Some((id, menu::properties(items, id, &names)?)))
                .collect();
            Ok(Some((found,).to_variant()))
        }
        "GetProperty" => {
            let (id, name) = params.get::<(i32, String)>().ok_or(invalid)?;
            let value = menu::properties(items, id, &[name])
                .and_then(|properties| properties.into_values().next())
                .ok_or("Unknown menu property")?;
            Ok(Some((value,).to_variant()))
        }
        "Event" => {
            let (id, event, _, _) = params
                .get::<(i32, String, glib::Variant, u32)>()
                .ok_or(invalid)?;
            menu_event(items, id, &event, command);
            Ok(None)
        }
        "EventGroup" => {
            let events = params
                .get::<(Vec<(i32, String, glib::Variant, u32)>,)>()
                .ok_or(invalid)?
                .0;
            let mut errors = Vec::new();
            for (id, event, _, _) in events {
                if menu::item(items, id).is_none() {
                    errors.push(id);
                }
                menu_event(items, id, &event, command);
            }
            Ok(Some((errors,).to_variant()))
        }
        // The menu is fixed: nothing to refresh before it opens.
        "AboutToShow" => Ok(Some((false,).to_variant())),
        "AboutToShowGroup" => Ok(Some((Vec::<i32>::new(), Vec::<i32>::new()).to_variant())),
        _ => Err("Unknown dbusmenu method"),
    }
}

fn menu_event(items: &[TrayMenuItem], id: i32, event: &str, command: &TrayCallback) {
    if event != "clicked" {
        return;
    }
    if let Some(TrayMenuItem::Action { action, .. }) = menu::item(items, id) {
        command(TrayCommand::Menu(action.clone()));
    }
}

fn export_menu(
    connection: &gio::DBusConnection,
    spec: &Rc<TraySpec>,
    command: &TrayCallback,
) -> Result<(), String> {
    let (spec, command) = (spec.clone(), command.clone());
    connection
        .register_object(MENU_PATH, &interface(MENU_XML)?)
        .method_call(move |_, _, _, _, method, params, invocation| {
            match handle_menu_method(&spec.items, method, &params, &command) {
                Ok(reply) => invocation.return_value(reply.as_ref()),
                Err(message) => {
                    invocation.return_dbus_error("org.freedesktop.DBus.Error.InvalidArgs", message)
                }
            }
        })
        .property(|_, _, _, _, property| menu_property(property))
        .build()
        .map(drop)
        .map_err(|e| format!("menu interface: {e}"))
}

/// Announce the item to the tray host. The object path alone is enough: the
/// watcher pairs it with this connection's unique name.
fn register_with_watcher(connection: &gio::DBusConnection, shown: Rc<std::cell::Cell<bool>>) {
    connection.call(
        Some(WATCHER_NAME),
        WATCHER_PATH,
        WATCHER_NAME,
        "RegisterStatusNotifierItem",
        Some(&(ITEM_PATH,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        move |result| match result {
            Ok(_) => {
                debug!("tray: registered with {WATCHER_NAME}");
                shown.set(true);
            }
            Err(e) => warn!("tray: {WATCHER_NAME} refused the icon: {e}"),
        },
    );
}

/// A tray icon; dropping it removes the icon.
pub struct Tray {
    connection: gio::DBusConnection,
    /// Stops watching for the tray host; run in Drop before the connection
    /// closes. gio does not export the watcher id's type, so it stays captured.
    unwatch: Option<Box<dyn FnOnce()>>,
    /// Whether a tray host took the icon and still runs.
    shown: Rc<std::cell::Cell<bool>>,
}

impl Tray {
    /// Export the icon and its menu and show it in the tray, now or as soon as
    /// a tray host starts. `None` if the session bus cannot be reached.
    pub fn new(spec: TraySpec, command: TrayCallback) -> Option<Self> {
        let connection = match crate::mpris::private_session_bus() {
            Ok(connection) => connection,
            Err(e) => {
                warn!("tray: failed to connect a private session bus: {e}");
                return None;
            }
        };
        let spec = Rc::new(spec);
        let exported = export_item(&connection, &spec, &command)
            .and_then(|()| export_menu(&connection, &spec, &command));
        if let Err(e) = exported {
            warn!("tray: failed to export {e}");
            let _ = connection.close_sync(gio::Cancellable::NONE);
            return None;
        }
        // The host may start, restart or never run (GNOME without the
        // AppIndicator extension): register each time the watcher appears.
        let shown = Rc::new(std::cell::Cell::new(false));
        let watcher = gio::bus_watch_name_on_connection(
            &connection,
            WATCHER_NAME,
            gio::BusNameWatcherFlags::NONE,
            {
                let shown = shown.clone();
                move |connection, _, _| register_with_watcher(&connection, shown.clone())
            },
            {
                let shown = shown.clone();
                move |_, _| {
                    debug!("tray: no {WATCHER_NAME} on the session bus");
                    shown.set(false);
                }
            },
        );
        info!("tray: icon {} exported", spec.icon_name);
        Some(Self {
            connection,
            unwatch: Some(Box::new(move || gio::bus_unwatch_name(watcher))),
            shown,
        })
    }

    /// Whether a tray host shows the icon now: with none (GNOME without the
    /// AppIndicator extension), a window hidden to the tray could not come
    /// back.
    #[must_use]
    pub fn is_shown(&self) -> bool {
        self.shown.get()
    }

    /// [`Self::is_shown`], readable where the tray itself is not at hand.
    #[must_use]
    pub fn shown_flag(&self) -> Rc<std::cell::Cell<bool>> {
        self.shown.clone()
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // Unwatch before closing: a closed connection would call the vanished
        // handler with a NULL connection.
        if let Some(unwatch) = self.unwatch.take() {
            unwatch();
        }
        let _ = self.connection.close_sync(gio::Cancellable::NONE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn spec() -> TraySpec {
        TraySpec {
            id: "big-audio-player".into(),
            title: "Big Audio Player".into(),
            icon_name: "big-audio-player-symbolic".into(),
            items: vec![
                TrayMenuItem::action("show", "Show"),
                TrayMenuItem::Separator,
                TrayMenuItem::action("quit", "Quit"),
            ],
        }
    }

    #[test]
    fn interface_xml_parses() {
        assert_eq!(
            interface(ITEM_XML).unwrap().name(),
            "org.kde.StatusNotifierItem"
        );
        assert_eq!(
            interface(MENU_XML).unwrap().name(),
            "com.canonical.dbusmenu"
        );
    }

    /// Each property of the XML and its type: gio offers no list to walk.
    fn declared_properties(xml: &str) -> Vec<(String, String)> {
        xml.lines()
            .filter_map(|line| {
                let line = line.trim().strip_prefix("<property name='")?;
                let (name, rest) = line.split_once('\'')?;
                let signature = rest.split("type='").nth(1)?.split('\'').next()?;
                Some((name.to_owned(), signature.to_owned()))
            })
            .collect()
    }

    #[test]
    fn every_item_property_matches_its_declared_type() {
        let spec = spec();
        let declared = declared_properties(ITEM_XML);
        assert_eq!(declared.len(), 16);
        for (name, signature) in declared {
            let value =
                item_property(&spec, &name).unwrap_or_else(|| panic!("{name} has no value"));
            assert_eq!(value.type_().as_str(), signature, "{name}");
        }
    }

    #[test]
    fn every_menu_property_matches_its_declared_type() {
        let declared = declared_properties(MENU_XML);
        assert_eq!(declared.len(), 4);
        for (name, signature) in declared {
            assert_eq!(menu_property(&name).type_().as_str(), signature, "{name}");
        }
    }

    #[test]
    fn clicking_an_entry_sends_its_action() {
        let sent = Rc::new(RefCell::new(Vec::new()));
        let command: TrayCallback = {
            let sent = sent.clone();
            Rc::new(move |command| sent.borrow_mut().push(command))
        };
        let items = spec().items;
        let click = |id: i32, event: &str| {
            let params = (id, event, "".to_variant(), 0_u32).to_variant();
            handle_menu_method(&items, "Event", &params, &command).unwrap();
        };
        click(3, "clicked");
        click(1, "hovered");
        click(2, "clicked");
        click(9, "clicked");
        assert_eq!(*sent.borrow(), [TrayCommand::Menu("quit".into())]);
    }

    #[test]
    fn menu_replies_match_the_declared_signatures() {
        let command: TrayCallback = Rc::new(|_| {});
        let items = spec().items;
        let info = interface(MENU_XML).unwrap();
        // The out arguments of menu.xml.
        let calls = [
            (
                "GetLayout",
                (0_i32, -1_i32, Vec::<String>::new()).to_variant(),
                "(u(ia{sv}av))",
            ),
            (
                "GetGroupProperties",
                (vec![1_i32, 2], Vec::<String>::new()).to_variant(),
                "(a(ia{sv}))",
            ),
            ("GetProperty", (1_i32, "label").to_variant(), "(v)"),
            ("AboutToShow", (0_i32,).to_variant(), "(b)"),
            ("AboutToShowGroup", (vec![0_i32],).to_variant(), "(aiai)"),
            (
                "EventGroup",
                (vec![(1_i32, "opened", "".to_variant(), 0_u32)],).to_variant(),
                "(ai)",
            ),
        ];
        for (method, params, reply_type) in calls {
            assert!(
                info.lookup_method(method).is_some(),
                "{method} is not declared"
            );
            let reply = handle_menu_method(&items, method, &params, &command)
                .unwrap()
                .unwrap();
            assert_eq!(reply.type_().as_str(), reply_type, "{method}");
        }
    }

    const WATCHER_XML: &str = "<node><interface name='org.kde.StatusNotifierWatcher'>\
        <method name='RegisterStatusNotifierItem'><arg type='s' direction='in'/></method>\
        </interface></node>";

    fn iterate_until(done: impl Fn() -> bool) {
        let context = glib::MainContext::default();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "timed out");
            context.iteration(true);
        }
    }

    fn call(
        connection: &gio::DBusConnection,
        dest: &str,
        path: &str,
        interface: &str,
        method: &str,
        params: glib::Variant,
    ) -> glib::Variant {
        let reply = Rc::new(RefCell::new(None));
        let slot = reply.clone();
        connection.call(
            Some(dest),
            path,
            interface,
            method,
            Some(&params),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            move |result| *slot.borrow_mut() = Some(result),
        );
        iterate_until(|| reply.borrow().is_some());
        let result = reply.borrow_mut().take().unwrap();
        result.unwrap_or_else(|e| panic!("{interface}.{method}: {e}"))
    }

    /// A tray host registers the icon, reads it and clicks a menu entry.
    #[test]
    #[ignore = "needs its own session bus: dbus-run-session -- cargo test -p big-app-kit tray -- --ignored"]
    fn a_tray_host_registers_reads_and_clicks_the_icon() {
        let host = crate::mpris::private_session_bus().unwrap();
        let registered = Rc::new(RefCell::new(None::<(String, String)>));
        let slot = registered.clone();
        host.register_object("/StatusNotifierWatcher", &interface(WATCHER_XML).unwrap())
            .method_call(move |_, sender, _, _, _, params, invocation| {
                let (service,) = params.get::<(String,)>().unwrap();
                *slot.borrow_mut() = Some((sender.unwrap_or_default().to_owned(), service));
                invocation.return_value(None);
            })
            .build()
            .unwrap();
        let owned = Rc::new(RefCell::new(false));
        let flag = owned.clone();
        let owner = gio::bus_own_name_on_connection(
            &host,
            WATCHER_NAME,
            gio::BusNameOwnerFlags::NONE,
            move |_, _| *flag.borrow_mut() = true,
            |_, _| {},
        );
        iterate_until(|| *owned.borrow());

        let sent = Rc::new(RefCell::new(Vec::new()));
        let command: TrayCallback = {
            let sent = sent.clone();
            Rc::new(move |command| sent.borrow_mut().push(command))
        };
        let tray = Tray::new(spec(), command).unwrap();
        iterate_until(|| registered.borrow().is_some());
        let (item, path) = registered.borrow().clone().unwrap();
        assert_eq!(path, ITEM_PATH);
        iterate_until(|| tray.is_shown());

        let all = call(
            &host,
            &item,
            ITEM_PATH,
            "org.freedesktop.DBus.Properties",
            "GetAll",
            ("org.kde.StatusNotifierItem",).to_variant(),
        );
        let properties = glib::VariantDict::new(Some(&all.child_value(0)));
        let icon = properties.lookup::<String>("IconName").unwrap();
        assert_eq!(icon.as_deref(), Some("big-audio-player-symbolic"));

        let layout = call(
            &host,
            &item,
            MENU_PATH,
            "com.canonical.dbusmenu",
            "GetLayout",
            (0_i32, -1_i32, Vec::<String>::new()).to_variant(),
        );
        assert_eq!(layout.child_value(1).child_value(2).n_children(), 3);
        call(
            &host,
            &item,
            MENU_PATH,
            "com.canonical.dbusmenu",
            "Event",
            (3_i32, "clicked", "".to_variant(), 0_u32).to_variant(),
        );
        call(
            &host,
            &item,
            ITEM_PATH,
            "org.kde.StatusNotifierItem",
            "Activate",
            (0_i32, 0_i32).to_variant(),
        );
        assert_eq!(
            *sent.borrow(),
            [TrayCommand::Menu("quit".into()), TrayCommand::Activate]
        );

        drop(tray);
        gio::bus_unown_name(owner);
        let _ = host.close_sync(gio::Cancellable::NONE);
    }
}
