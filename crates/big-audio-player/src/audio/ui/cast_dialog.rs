//! Audio cast (Chromecast) device-picker dialog. Restored app-local from the
//! god-object's `show_audio_cast_dialog`. The cast engine lives in
//! `big_media_runtime::cast::chromecast`; this dialog only lists discovered
//! devices and routes the chosen one back to the shell, which owns the current
//! file path and performs the cast.

use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::{
    layout::{
        dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
        dialog_page::BigDialogPage,
    },
    list::action_nav_row::{BigActionNavRow, BigActionNavRowSpec},
};
use gtk::glib;
use relm4::Sender;

use crate::audio::ui::audio_shell::AudioShellInput;
use crate::cast::chromecast::CastManager;
use crate::i18n::i18n;

/// Present the cast dialog: discover devices, pick one (→ `CastToDeviceIndex`),
/// or stop (→ `CastStop`). The shell does the actual cast/stop + toasts.
pub(crate) fn present_cast(
    parent: &impl IsA<gtk::Widget>,
    cast_device_registry: Arc<CastManager>,
    input: Sender<AudioShellInput>,
) {
    let dialog = adw::Dialog::builder()
        .title(i18n("Cast Audio"))
        .content_width(560)
        .content_height(420)
        .build();

    let page = BigDialogPage::scrolled_clamped(520, 440);
    page.content().set_spacing(12);
    dialog.set_child(Some(page.root()));

    let status = gtk::Label::new(Some(&i18n("Searching for devices...")));
    status.add_css_class("dim-label");
    status.set_halign(gtk::Align::Start);
    status.set_wrap(true);
    page.content().append(&status);

    let list_box = gtk::ListBox::new();
    list_box.set_selection_mode(gtk::SelectionMode::None);
    list_box.add_css_class("boxed-list");

    list_box.set_vexpand(true);
    page.content().append(&list_box);

    let footer = BigDialogFooter::new(
        &BigDialogFooterSpec::new(i18n("Close"), i18n("Stop Casting"))
            .with_accept_css_classes(&["destructive-action"]),
    );
    footer
        .accept_button()
        .set_sensitive(cast_device_registry.is_casting());
    page.root().add_bottom_bar(footer.root());

    cast_device_registry.start_discovery(8);

    // Poll discovery every 800 ms while the dialog lives; rebuild the list.
    let cast_device_registry_for_poll = Arc::clone(&cast_device_registry);
    let list_clone = list_box.clone();
    let status_clone = status.clone();
    let dialog_weak = glib::WeakRef::new();
    dialog_weak.set(Some(&dialog));
    glib::timeout_add_local(Duration::from_millis(800), move || {
        big_app_kit::containment::guard("audio.cast_dialog.poll", || {
            if dialog_weak.upgrade().is_none() {
                return glib::ControlFlow::Break;
            }
            let devices = cast_device_registry_for_poll.get_devices();
            while let Some(row) = list_clone.row_at_index(0) {
                list_clone.remove(&row);
            }
            for dev in &devices {
                let label = if dev.model.is_empty() {
                    dev.name.clone()
                } else {
                    format!("{} ({})", dev.name, dev.model)
                };
                let row = BigActionNavRow::new(
                    BigActionNavRowSpec::new(&label)
                        .subtitle(format!("{}:{}", dev.host, dev.port))
                        .action_label(i18n("Cast to this device")),
                )
                .into_root();
                list_clone.append(&row);
            }
            if devices.is_empty() {
                status_clone.set_text(&i18n("Searching for devices..."));
            } else {
                status_clone.set_text(&format!("{} {}", devices.len(), i18n("device(s) found")));
            }
            if !cast_device_registry_for_poll.is_discovering() {
                cast_device_registry_for_poll.start_discovery(8);
            }
            glib::ControlFlow::Continue
        })
        .unwrap_or(glib::ControlFlow::Break)
    });

    let cast_input = input.clone();
    let stop_cast_button = footer.accept_button().clone();
    list_box.connect_row_activated(move |_, row| {
        let idx = row.index() as usize;
        let _ = cast_input.send(AudioShellInput::CastToDeviceIndex(idx));
        // Optimistic: the shell reports failures via toast.
        stop_cast_button.set_sensitive(true);
    });

    let stop_input = input.clone();
    footer.accept_button().connect_clicked(move |button| {
        let _ = stop_input.send(AudioShellInput::CastStop);
        button.set_sensitive(false);
    });

    let dialog_weak = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    dialog.present(Some(parent));
}
