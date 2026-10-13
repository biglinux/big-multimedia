// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Tests for desktop integration helpers.

use super::*;
use gtk::prelude::*;
use relm4::gtk::Application;

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

#[test]
#[cfg_attr(miri, ignore)]
fn install_static_accels_records_action_bindings() {
    let app = Application::builder()
        .application_id("br.com.biglinux.big-app-kit.accel-test")
        .build();

    install_static_accels(&app, &[("app.open", &["<Primary>O", "F10"])]);

    assert_eq!(
        app.accels_for_action("app.open")
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        vec!["<Control>o", "F10"]
    );
}
