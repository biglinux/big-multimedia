//! Smartphone integration dialog.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, atomic::AtomicBool};

use adw::prelude::*;
use big_app_kit::remote_control::{self, RemoteHandlers, ServerHandle};
use big_media_data::i18n::i18n;
use big_relm4_components::{
    layout::{dialog_page::BigDialogPage, preferences::BigPreferencesGroupSpec},
    list::navigation_row::{BigNavigationRow, BigNavigationRowSpec},
};

use super::qr::build_qr_widget;

fn detect_lan_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

/// Present the smartphone web-remote dialog.
pub fn present<F>(parent: &impl IsA<gtk::Widget>, build_remote: F)
where
    F: Fn(Arc<AtomicBool>) -> RemoteHandlers + 'static,
{
    let dialog = adw::Dialog::builder()
        .title(i18n("Smartphone"))
        .content_width(460)
        .content_height(640)
        .build();

    let dialog_page = BigDialogPage::plain();
    let page = adw::PreferencesPage::new();
    page.set_vexpand(true);

    let remote_group = BigPreferencesGroupSpec::new(i18n("Web Remote"))
        .description(i18n(
            "Turn on to control this player from a phone or tablet on the same network. \
             HTTP is not encrypted: use only a trusted private network. \
             Scan the QR code with the phone's camera to open the remote.",
        ))
        .build();

    let enable_row = adw::SwitchRow::builder()
        .title(i18n("Enable web remote"))
        .build();
    remote_group.add(&enable_row);

    let qr_row = adw::PreferencesRow::builder()
        .title(i18n("QR code"))
        .activatable(false)
        .focusable(false)
        .build();
    let qr_box = gtk::Box::new(gtk::Orientation::Vertical, 12);
    qr_box.set_margin_top(18);
    qr_box.set_margin_bottom(18);
    qr_box.set_margin_start(18);
    qr_box.set_margin_end(18);
    qr_row.set_child(Some(&qr_box));
    remote_group.add(&qr_row);

    let status_label = gtk::Label::builder()
        .label(i18n("Web remote is off."))
        .wrap(true)
        .justify(gtk::Justification::Center)
        .build();
    status_label.add_css_class("dim-label");
    qr_box.append(&status_label);

    let url_label = gtk::Label::builder()
        .selectable(true)
        .wrap(true)
        .justify(gtk::Justification::Center)
        .visible(false)
        .build();
    url_label.add_css_class("caption");
    qr_box.append(&url_label);

    page.add(&remote_group);

    let kde_connect_group = BigPreferencesGroupSpec::new(i18n("KDE Connect / GSConnect"))
        .description(i18n(
            "If you have KDE Connect (Plasma, Android) or GSConnect (GNOME) installed, \
             your paired phone already shows media controls, track information and \
             volume for this player - no extra setup is required on our side.",
        ))
        .build();

    let kde_connect_row = BigNavigationRow::new(
        BigNavigationRowSpec::new(i18n("Install KDE Connect on your phone"))
            .subtitle(i18n(
                "Open the link on the phone's app store. Pair once - then media keys, \
                 notifications and file sharing work both ways.",
            ))
            .trailing_icon_name("adw-external-link-symbolic")
            .action_label(i18n("Open KDE Connect download page")),
    )
    .into_root();

    let parent_weak = parent.downgrade();
    kde_connect_row.connect_activated(move |_| {
        if let Some(parent) = parent_weak.upgrade() {
            let launcher = gtk::UriLauncher::new("https://kdeconnect.kde.org/download.html");
            launcher.launch(
                parent.root().and_downcast_ref::<gtk::Window>(),
                gtk::gio::Cancellable::NONE,
                |_| {},
            );
        }
    });
    kde_connect_group.add(&kde_connect_row);

    page.add(&kde_connect_group);

    dialog_page.content().append(&page);
    dialog.set_child(Some(dialog_page.root()));

    let server_handle: Rc<RefCell<Option<ServerHandle>>> = Rc::new(RefCell::new(None));
    let qr_box_for_toggle = qr_box.clone();
    let status_for_toggle = status_label.clone();
    let url_for_toggle = url_label.clone();
    let server_for_toggle = Rc::clone(&server_handle);
    let build_remote = Rc::new(build_remote);
    let build_for_toggle = Rc::clone(&build_remote);

    enable_row.connect_active_notify(move |switch| {
        let active = switch.is_active();
        if active {
            let revoked = Arc::new(AtomicBool::new(false));
            match remote_control::start_scoped(
                "0.0.0.0:0".parse().expect("static socket address"),
                (build_for_toggle)(revoked.clone()),
                revoked,
            ) {
                Ok(handle) => {
                    let ip = detect_lan_ip().unwrap_or_else(|| "127.0.0.1".to_string());
                    let url = format!("http://{}:{}/{}/", ip, handle.port, handle.token);
                    replace_qr(&qr_box_for_toggle, &url);
                    url_for_toggle.set_text(&url);
                    url_for_toggle.set_visible(true);
                    status_for_toggle.set_text(&i18n("Scan the code with the phone's camera."));
                    *server_for_toggle.borrow_mut() = Some(handle);
                }
                Err(error) => {
                    switch.set_active(false);
                    status_for_toggle
                        .set_text(&format!("{}: {error}", i18n("Could not start server")));
                    url_for_toggle.set_visible(false);
                    clear_qr(&qr_box_for_toggle);
                }
            }
        } else {
            server_for_toggle.borrow_mut().take();
            url_for_toggle.set_visible(false);
            status_for_toggle.set_text(&i18n("Web remote is off."));
            clear_qr(&qr_box_for_toggle);
        }
    });

    let server_for_close = Rc::clone(&server_handle);
    dialog.connect_closed(move |_| {
        server_for_close.borrow_mut().take();
    });

    dialog.present(Some(parent));
}

fn replace_qr(parent: &gtk::Box, qr_payload: &str) {
    clear_qr(parent);
    let qr = build_qr_widget(qr_payload, 260);
    parent.prepend(&qr);
}

fn clear_qr(parent: &gtk::Box) {
    let mut child = parent.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        if widget.is::<gtk::DrawingArea>() {
            parent.remove(&widget);
        }
        child = next;
    }
}
