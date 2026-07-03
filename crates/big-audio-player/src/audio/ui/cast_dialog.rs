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
    list::{
        action_nav_row::{BigActionNavRow, BigActionNavRowSpec},
        bounded_list_box::{clear_list_box_rows, BigBoundedListBox, BigBoundedListBoxSpec},
    },
};
use gtk::glib;
use relm4::Sender;

use crate::audio::ui::audio_shell::AudioShellInput;
use crate::cast::chromecast::CastSessionController;
use crate::i18n::i18n;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CastDialogSpec {
    title: &'static str,
    content_width: i32,
    content_height: i32,
    page_width: i32,
    page_height: i32,
    content_spacing: i32,
    initial_status: &'static str,
    list_accessible_label: &'static str,
    device_action_label: &'static str,
    close_label: &'static str,
    stop_label: &'static str,
    discovery_seconds: u64,
    poll_interval_ms: u64,
    list_row_limit: u32,
}

fn cast_dialog_spec() -> CastDialogSpec {
    CastDialogSpec {
        title: "Cast Audio",
        content_width: 560,
        content_height: 420,
        page_width: 520,
        page_height: 440,
        content_spacing: 12,
        initial_status: "Searching for devices...",
        list_accessible_label: "Cast devices",
        device_action_label: "Cast to this device",
        close_label: "Close",
        stop_label: "Stop Casting",
        discovery_seconds: 8,
        poll_interval_ms: 800,
        list_row_limit: 16,
    }
}

fn cast_device_row_label(name: &str, model: &str) -> String {
    if model.is_empty() {
        name.to_string()
    } else {
        format!("{name} ({model})")
    }
}

fn cast_device_row_subtitle(host: &str, port: u16) -> String {
    format!("{host}:{port}")
}

fn cast_status_text(device_count: usize) -> String {
    if device_count == 0 {
        i18n(cast_dialog_spec().initial_status)
    } else {
        format!("{} {}", device_count, i18n("device(s) found"))
    }
}

fn should_restart_cast_discovery(is_discovering: bool) -> bool {
    !is_discovering
}

/// Present the cast dialog: discover devices, pick one (→ `CastToDeviceIndex`),
/// or stop (→ `CastStop`). The shell does the actual cast/stop + toasts.
pub(crate) fn present_cast(
    parent: &impl IsA<gtk::Widget>,
    cast_device_registry: Arc<CastSessionController>,
    input: Sender<AudioShellInput>,
) {
    let dialog_spec = cast_dialog_spec();
    let dialog = adw::Dialog::builder()
        .title(i18n(dialog_spec.title))
        .content_width(dialog_spec.content_width)
        .content_height(dialog_spec.content_height)
        .build();

    let page = BigDialogPage::scrolled_clamped(dialog_spec.page_width, dialog_spec.page_height);
    page.content().set_spacing(dialog_spec.content_spacing);
    dialog.set_child(Some(page.root()));

    let status = gtk::Label::new(Some(&i18n(dialog_spec.initial_status)));
    status.add_css_class("dim-label");
    status.set_halign(gtk::Align::Start);
    status.set_wrap(true);
    page.content().append(&status);

    let cast_device_list = BigBoundedListBox::new(
        BigBoundedListBoxSpec::new(dialog_spec.list_row_limit).css_class("boxed-list"),
    );
    let list_box = cast_device_list.root().clone();
    list_box.update_property(&[gtk::accessible::Property::Label(&i18n(
        dialog_spec.list_accessible_label,
    ))]);
    list_box.set_vexpand(true);
    page.content().append(&list_box);

    let footer = BigDialogFooter::new(
        &BigDialogFooterSpec::new(i18n(dialog_spec.close_label), i18n(dialog_spec.stop_label))
            .with_accept_css_classes(&["destructive-action"]),
    );
    footer
        .accept_button()
        .set_sensitive(cast_device_registry.is_casting());
    page.root().add_bottom_bar(footer.root());

    cast_device_registry.start_discovery(dialog_spec.discovery_seconds);

    // Poll discovery every 800 ms while the dialog lives; rebuild the list.
    let cast_device_registry_for_poll = Arc::clone(&cast_device_registry);
    let list_clone = list_box.clone();
    let status_clone = status.clone();
    let dialog_weak = glib::WeakRef::new();
    dialog_weak.set(Some(&dialog));
    glib::timeout_add_local(
        Duration::from_millis(dialog_spec.poll_interval_ms),
        move || {
            big_app_kit::containment::guard("audio.cast_dialog.poll", || {
                if dialog_weak.upgrade().is_none() {
                    return glib::ControlFlow::Break;
                }
                let devices = cast_device_registry_for_poll.get_devices();
                clear_list_box_rows(&list_clone);
                for dev in &devices {
                    let label = cast_device_row_label(&dev.name, &dev.model);
                    let row = BigActionNavRow::new(
                        BigActionNavRowSpec::new(&label)
                            .subtitle(cast_device_row_subtitle(&dev.host, dev.port))
                            .action_label(i18n(dialog_spec.device_action_label)),
                    )
                    .into_root();
                    list_clone.append(&row);
                }
                status_clone.set_text(&cast_status_text(devices.len()));
                if should_restart_cast_discovery(cast_device_registry_for_poll.is_discovering()) {
                    cast_device_registry_for_poll.start_discovery(dialog_spec.discovery_seconds);
                }
                glib::ControlFlow::Continue
            })
            .unwrap_or(glib::ControlFlow::Break)
        },
    );

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cast_dialog_spec_keeps_expected_labels_and_sizing() {
        assert_eq!(
            cast_dialog_spec(),
            CastDialogSpec {
                title: "Cast Audio",
                content_width: 560,
                content_height: 420,
                page_width: 520,
                page_height: 440,
                content_spacing: 12,
                initial_status: "Searching for devices...",
                list_accessible_label: "Cast devices",
                device_action_label: "Cast to this device",
                close_label: "Close",
                stop_label: "Stop Casting",
                discovery_seconds: 8,
                poll_interval_ms: 800,
                list_row_limit: 16,
            }
        );
    }

    #[test]
    fn cast_device_row_text_uses_model_only_when_present() {
        assert_eq!(cast_device_row_label("Living Room", ""), "Living Room");
        assert_eq!(
            cast_device_row_label("Living Room", "Chromecast"),
            "Living Room (Chromecast)"
        );
        assert_eq!(
            cast_device_row_subtitle("192.168.1.10", 8009),
            "192.168.1.10:8009"
        );
    }

    #[test]
    fn cast_status_text_reports_search_or_count() {
        assert_eq!(cast_status_text(0), "Searching for devices...");
        assert_eq!(cast_status_text(2), "2 device(s) found");
    }

    #[test]
    fn discovery_restarts_only_after_discovery_stops() {
        assert!(should_restart_cast_discovery(false));
        assert!(!should_restart_cast_discovery(true));
    }
}
