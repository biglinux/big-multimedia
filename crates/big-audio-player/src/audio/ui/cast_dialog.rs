//! Audio cast (Chromecast) device-picker dialog. Restored app-local from the
//! god-object's `show_audio_cast_dialog`. The cast engine lives in
//! `big_media_runtime::cast::chromecast`; this dialog only lists discovered
//! devices and routes the chosen one back to the shell, which owns the current
//! file path and performs the cast.

use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::list::navigation_row::{BigNavigationRow, BigNavigationRowSpec};
use gtk::glib;
use relm4::Sender;

use crate::audio::ui::audio_shell::AudioShellInput;
use crate::cast::chromecast::CastManager;
use crate::i18n::i18n;

/// Present the cast dialog: discover devices, pick one (→ `CastToDeviceIndex`),
/// or stop (→ `CastStop`). The shell does the actual cast/stop + toasts.
pub(crate) fn present_cast(
    parent: &impl IsA<gtk::Widget>,
    manager: Arc<CastManager>,
    input: Sender<AudioShellInput>,
) {
    let (dialog, content) = crate::shared::dialogs::new_content_dialog(&i18n("Cast Audio"));

    let status = gtk::Label::new(Some(&i18n("Searching for devices...")));
    status.add_css_class("dim-label");
    content.append(&status);

    let list_box = gtk::ListBox::new();
    list_box.set_selection_mode(gtk::SelectionMode::None);
    list_box.add_css_class("boxed-list");

    let scroll = gtk::ScrolledWindow::builder()
        .child(&list_box)
        .min_content_height(120)
        .build();
    content.append(&scroll);

    let stop_btn = gtk::Button::with_label(&i18n("Stop Casting"));
    stop_btn.add_css_class("destructive-action");
    stop_btn.set_sensitive(manager.is_casting());
    content.append(&stop_btn);

    manager.start_discovery(8);

    // Poll discovery every 800 ms while the dialog lives; rebuild the list.
    let manager_poll = Arc::clone(&manager);
    let list_clone = list_box.clone();
    let status_clone = status.clone();
    let dialog_weak = glib::WeakRef::new();
    dialog_weak.set(Some(&dialog));
    glib::timeout_add_local(Duration::from_millis(800), move || {
        if dialog_weak.upgrade().is_none() {
            return glib::ControlFlow::Break;
        }
        let devices = manager_poll.get_devices();
        while let Some(row) = list_clone.row_at_index(0) {
            list_clone.remove(&row);
        }
        for dev in &devices {
            let label = if dev.model.is_empty() {
                dev.name.clone()
            } else {
                format!("{} ({})", dev.name, dev.model)
            };
            let row = BigNavigationRow::new(
                BigNavigationRowSpec::new(&label).subtitle(format!("{}:{}", dev.host, dev.port)),
            )
            .into_root();
            list_clone.append(&row);
        }
        if devices.is_empty() {
            status_clone.set_text(&i18n("Searching for devices..."));
        } else {
            status_clone.set_text(&format!("{} {}", devices.len(), i18n("device(s) found")));
        }
        if !manager_poll.is_discovering() {
            manager_poll.start_discovery(8);
        }
        glib::ControlFlow::Continue
    });

    let cast_input = input.clone();
    let stop_btn_ref = stop_btn.clone();
    list_box.connect_row_activated(move |_, row| {
        let idx = row.index() as usize;
        let _ = cast_input.send(AudioShellInput::CastToDeviceIndex(idx));
        // Optimistic: the shell reports failures via toast.
        stop_btn_ref.set_sensitive(true);
    });

    let stop_input = input.clone();
    stop_btn.connect_clicked(move |btn| {
        let _ = stop_input.send(AudioShellInput::CastStop);
        btn.set_sensitive(false);
    });

    dialog.present(Some(parent));
}
