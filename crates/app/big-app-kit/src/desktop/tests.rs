// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Tests for desktop integration helpers.

use super::*;
use gtk::prelude::*;

#[test]
fn system_power_actions_resolve_to_systemctl_args() {
    assert_eq!(BigSystemPowerAction::Suspend.command_args(), ["suspend"]);
    assert_eq!(BigSystemPowerAction::PowerOff.command_args(), ["poweroff"]);
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_action_runs_callback() {
    use std::cell::Cell;
    use std::rc::Rc;

    let group = gio::SimpleActionGroup::new();
    let called = Rc::new(Cell::new(false));
    let called_clone = called.clone();
    install_action(&group, "demo", move || called_clone.set(true));

    group.activate_action("demo", None);

    assert!(called.get());
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_action_enabled_blocks_callback_when_disabled() {
    use std::cell::Cell;
    use std::rc::Rc;

    let group = gio::SimpleActionGroup::new();
    let called = Rc::new(Cell::new(false));
    let called_clone = called.clone();
    install_action_enabled(&group, "demo", false, move || called_clone.set(true));

    group.activate_action("demo", None);

    assert!(!called.get());
}

#[test]
#[cfg_attr(miri, ignore)]
fn install_quit_action_registers_application_action() {
    let app = gio::Application::new(
        Some("br.com.biglinux.big-app-kit.quit-test"),
        gio::ApplicationFlags::empty(),
    );

    install_quit_action(&app);

    assert!(app.lookup_action("quit").is_some());
}

/// What the stand-in portal received: the method and the URI, or the
/// contents read through the passed descriptor.
type Received = std::rc::Rc<std::cell::RefCell<Vec<(String, String)>>>;

/// An OpenURI portal on `host`, answering every call and recording it.
fn stand_in_portal(host: &gio::DBusConnection) -> (Received, gio::RegistrationId, gio::OwnerId) {
    use crate::tray::tests::iterate_until;
    use gio::prelude::UnixFDListExtManual;
    use std::io::Read;

    const OPEN_URI_XML: &str = r#"<node><interface name="org.freedesktop.portal.OpenURI">
        <method name="OpenURI"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="a{sv}" direction="in"/><arg type="o" direction="out"/></method>
        <method name="OpenFile"><arg type="s" direction="in"/><arg type="h" direction="in"/><arg type="a{sv}" direction="in"/><arg type="o" direction="out"/></method>
        <method name="OpenDirectory"><arg type="s" direction="in"/><arg type="h" direction="in"/><arg type="a{sv}" direction="in"/><arg type="o" direction="out"/></method>
    </interface></node>"#;
    let interface = gio::DBusNodeInfo::for_xml(OPEN_URI_XML)
        .unwrap()
        .lookup_interface(PORTAL_OPEN_URI)
        .unwrap();
    let received = Received::default();
    let sink = received.clone();
    let object = host
        .register_object(PORTAL_PATH, &interface)
        .method_call(move |_, _, _, _, method, params, invocation| {
            let what = if method == "OpenURI" {
                params.child_value(1).get::<String>().unwrap()
            } else {
                let fd = invocation.message().unix_fd_list().unwrap().get(0).unwrap();
                let mut text = String::new();
                std::fs::File::from(fd).read_to_string(&mut text).unwrap();
                text
            };
            sink.borrow_mut().push((method.to_owned(), what));
            let request =
                glib::variant::ObjectPath::try_from("/org/freedesktop/portal/desktop/request/1/t")
                    .unwrap();
            invocation.return_value(Some(&(request,).to_variant()));
        })
        .build()
        .unwrap();
    let owned = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = owned.clone();
    let owner = gio::bus_own_name_on_connection(
        host,
        PORTAL_NAME,
        gio::BusNameOwnerFlags::NONE,
        move |_, _| flag.set(true),
        |_, _| {},
    );
    iterate_until(|| owned.get());
    (received, object, owner)
}

/// Links go to the portal as URIs, files and folders as descriptors.
#[test]
#[ignore = "needs its own session bus: dbus-run-session -- cargo test -p big-app-kit portal -- --ignored"]
fn portal_receives_links_and_files() {
    let context = glib::MainContext::default();
    let host = context
        .block_on(crate::mpris::private_session_bus())
        .unwrap();
    let (received, object, owner) = stand_in_portal(&host);

    let dir = tempfile::tempdir().unwrap();
    let song = dir.path().join("song.txt");
    std::fs::write(&song, "the song").unwrap();
    context
        .block_on(portal_call(PortalTarget::Uri(
            "https://example.org/".into(),
        )))
        .unwrap();
    context
        .block_on(portal_call(PortalTarget::File(song.clone())))
        .unwrap();
    context
        .block_on(portal_call(PortalTarget::Selected(song)))
        .unwrap();
    assert_eq!(
        *received.borrow(),
        [
            ("OpenURI".to_owned(), "https://example.org/".to_owned()),
            ("OpenFile".to_owned(), "the song".to_owned()),
            ("OpenDirectory".to_owned(), "the song".to_owned()),
        ]
    );
    gio::bus_unown_name(owner);
    let _ = host.unregister_object(object);
    let _ = host.close_sync(gio::Cancellable::NONE);
    while context.pending() {
        context.iteration(false);
    }
}
