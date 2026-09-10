//! Progress page builder: overall bar + boxed-list of per-item rows
//! (status icon, filename, ffmpeg fps, time-start/elapsed, cancel + expandable
//! details with cmdline and stderr). Cancel-all uses a shared destructive alert.

use std::path::Path;

use adw::prelude::*;
use big_relm4_components::feedback::alert::{BigConfirmAlertSpec, build_confirm_alert_dialog};
use big_relm4_components::list::bounded_list_box::{BigBoundedListBox, BigBoundedListBoxSpec};

use super::{ProgressPage, ProgressRow, RowState};
use crate::text::text as i18n;

impl ProgressPage {
    /// Build a fresh progress page with no rows and the banner hidden.
    pub fn new() -> Self {
        Self::build_into(gtk::Box::new(gtk::Orientation::Vertical, 0))
    }

    /// Build the progress page into an existing [`gtk::Box`] root.
    ///
    /// Lets the Relm4 wrapper [`super::BigConverterProgress`] reuse the
    /// component's own `Root` widget as the container; [`new`](Self::new)
    /// delegates here with a fresh vertical box.
    #[must_use]
    pub fn build_into(container: gtk::Box) -> Self {
        let banner = adw::Banner::new("");
        banner.set_revealed(false);
        container.append(&banner);

        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let clamp = adw::Clamp::builder()
            .maximum_size(900)
            .tightening_threshold(600)
            .margin_start(24)
            .margin_end(24)
            .margin_top(16)
            .margin_bottom(24)
            .build();

        let content_box = gtk::Box::new(gtk::Orientation::Vertical, 16);

        let overall_label = gtk::Label::builder()
            .label(i18n("Ready"))
            .css_classes(["title-3"])
            .halign(gtk::Align::Start)
            .build();
        content_box.append(&overall_label);

        let overall_bar = gtk::ProgressBar::builder()
            .show_text(true)
            .text("0%")
            .build();
        content_box.append(&overall_bar);

        let list_box =
            BigBoundedListBox::new(BigBoundedListBoxSpec::new(32).css_class("boxed-list"));
        content_box.append(list_box.root());

        let action_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .halign(gtk::Align::Center)
            .margin_top(8)
            .build();

        let cancel_all_button = gtk::Button::builder()
            .label(i18n("Cancel All"))
            .css_classes(["pill", "destructive-action"])
            .build();
        action_bar.append(&cancel_all_button);

        let back_button = gtk::Button::builder()
            .label(i18n("Back to Queue"))
            .css_classes(["pill", "suggested-action"])
            .visible(false)
            .build();
        action_bar.append(&back_button);

        content_box.append(&action_bar);

        clamp.set_child(Some(&content_box));
        scroll.set_child(Some(&clamp));
        container.append(&scroll);

        Self::install_progress_css();

        let page = Self {
            container,
            list_box,
            overall_label,
            overall_bar,
            cancel_all_button,
            back_button,
            banner,
            rows: std::cell::RefCell::new(std::collections::HashMap::new()),
            on_back: std::rc::Rc::new(std::cell::RefCell::new(None)),
            on_cancel_all: std::rc::Rc::new(std::cell::RefCell::new(None)),
            on_cancel_conversion: std::rc::Rc::new(std::cell::RefCell::new(None)),
            total_items: std::cell::RefCell::new(0),
            done_count: std::cell::RefCell::new(0),
            failed_count: std::cell::RefCell::new(0),
            cancelled_count: std::cell::RefCell::new(0),
        };

        let back_callback = page.on_back.clone();
        page.back_button.connect_clicked(move |_| {
            if let Some(f) = back_callback.borrow().as_ref() {
                f();
            }
        });

        let cancel_all_callback = page.on_cancel_all.clone();
        page.cancel_all_button.connect_clicked(move |button| {
            let dialog = build_confirm_alert_dialog(
                &BigConfirmAlertSpec::new(
                    i18n("Cancel All Conversions?"),
                    i18n(
                    "This will stop all active conversions and skip pending items. This cannot be undone.",
                    ),
                    i18n("Continue"),
                    "cancel",
                    i18n("Cancel All"),
                )
                .cancel_id("keep")
                .destructive(true),
            );

            let cancel_all_response_callback = cancel_all_callback.clone();
            dialog.connect_response(None, move |_, resp| {
                if resp == "cancel"
                    && let Some(f) = cancel_all_response_callback.borrow().as_ref()
                {
                    f();
                }
            });

            if let Some(root) = button.root() {
                if let Some(win) = root.downcast_ref::<gtk::Window>() {
                    dialog.present(Some(win));
                } else {
                    dialog.present(gtk::Window::NONE);
                }
            } else {
                dialog.present(gtk::Window::NONE);
            }
        });

        page
    }

    fn install_progress_css() {
        big_relm4_components::theme::load_app_css(
            r"
            .queue-row-active {
                background: alpha(@accent_bg_color, 0.08);
                border-left: 3px solid @accent_color;
            }
            .queue-row-completed {
                background: alpha(@success_bg_color, 0.06);
                border-left: 3px solid @success_color;
            }
            .queue-row-failed {
                background: alpha(@error_bg_color, 0.06);
                border-left: 3px solid @error_color;
            }
            .queue-row-cancelled {
                background: alpha(@warning_bg_color, 0.06);
                border-left: 3px solid @warning_color;
            }
            .queue-row-pending {
                opacity: 0.7;
                border-left: 3px solid transparent;
            }
            .time-info {
                font-size: 0.85em;
                color: alpha(@window_fg_color, 0.55);
                font-variant-numeric: tabular-nums;
            }
            .details-box {
                background: alpha(@card_bg_color, 0.5);
                border-radius: 6px;
                padding: 12px;
                margin-top: 8px;
            }
            .log-view {
                font-family: monospace;
                font-size: 0.9em;
                background: alpha(@view_bg_color, 0.8);
                border-radius: 4px;
                padding: 8px;
            }
            ",
        );
    }

    pub(super) fn add_row(&self, id: u64, path: &Path) {
        let filename = path
            .file_name()
            .map_or_else(|| "?".into(), |name| name.to_string_lossy().to_string());

        let outer_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        outer_box.add_css_class("queue-row-pending");

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_start(12)
            .margin_end(12)
            .margin_top(8)
            .margin_bottom(8)
            .build();

        let status_icon = gtk::Image::from_icon_name("content-loading-symbolic");
        status_icon.set_pixel_size(32);
        status_icon.set_valign(gtk::Align::Center);
        status_icon.add_css_class("dim-label");
        content.append(&status_icon);

        let center = gtk::Box::new(gtk::Orientation::Vertical, 4);
        center.set_hexpand(true);
        center.set_valign(gtk::Align::Center);

        let filename_label = gtk::Label::builder()
            .label(&filename)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .css_classes(["heading"])
            .build();
        center.append(&filename_label);

        let status_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        let status_label = gtk::Label::builder()
            .label(i18n("Waiting in queue"))
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["dim-label"])
            .build();
        status_row.append(&status_label);

        let fps_label = gtk::Label::builder()
            .label("")
            .halign(gtk::Align::End)
            .css_classes(["dim-label"])
            .visible(false)
            .build();
        status_row.append(&fps_label);
        center.append(&status_row);

        let progress_bar = gtk::ProgressBar::builder()
            .margin_top(4)
            .visible(false)
            .build();
        center.append(&progress_bar);
        content.append(&center);

        let time_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .valign(gtk::Align::Center)
            .width_request(90)
            .build();

        let time_start_label = gtk::Label::builder()
            .label("")
            .halign(gtk::Align::End)
            .css_classes(["time-info"])
            .build();
        time_box.append(&time_start_label);

        let time_elapsed_label = gtk::Label::builder()
            .label("")
            .halign(gtk::Align::End)
            .css_classes(["time-info"])
            .build();
        time_box.append(&time_elapsed_label);
        content.append(&time_box);

        let button_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .valign(gtk::Align::Center)
            .build();

        let details_button = gtk::ToggleButton::builder()
            .icon_name("view-more-symbolic")
            .css_classes(["flat", "circular"])
            .sensitive(false)
            .build();
        details_button.update_property(&[gtk::accessible::Property::Label(&i18n(
            "Show conversion details",
        ))]);
        button_box.append(&details_button);

        let cancel_button = gtk::Button::builder()
            .icon_name("process-stop-symbolic")
            .css_classes(["flat", "circular"])
            .visible(false)
            .build();
        big_relm4_components::feedback::tooltip::set(&cancel_button, i18n("Cancel"));
        cancel_button
            .update_property(&[gtk::accessible::Property::Label(&i18n("Cancel conversion"))]);
        button_box.append(&cancel_button);
        content.append(&button_box);
        outer_box.append(&content);

        let details_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .build();

        let details_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_start(56)
            .margin_end(12)
            .margin_bottom(8)
            .css_classes(["details-box"])
            .build();

        let cmd_header = gtk::Label::builder()
            .label(i18n("FFmpeg Command"))
            .halign(gtk::Align::Start)
            .css_classes(["heading"])
            .margin_bottom(4)
            .build();
        details_box.append(&cmd_header);

        let cmd_label = gtk::Label::builder()
            .label("")
            .selectable(true)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::Char)
            .xalign(0.0)
            .css_classes(["log-view"])
            .build();
        details_box.append(&cmd_label);

        let stderr_header = gtk::Label::builder()
            .label(i18n("FFmpeg Output"))
            .halign(gtk::Align::Start)
            .css_classes(["heading"])
            .margin_top(8)
            .margin_bottom(4)
            .build();
        details_box.append(&stderr_header);

        let stderr_view = gtk::TextView::builder()
            .editable(false)
            .cursor_visible(false)
            .wrap_mode(gtk::WrapMode::Char)
            .monospace(true)
            .css_classes(["log-view"])
            .build();

        let stderr_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(120)
            .max_content_height(200)
            .build();
        stderr_scroll.set_child(Some(&stderr_view));
        details_box.append(&stderr_scroll);

        details_revealer.set_child(Some(&details_box));
        outer_box.append(&details_revealer);

        let revealer = details_revealer.clone();
        details_button.connect_toggled(move |button| {
            revealer.set_reveal_child(button.is_active());
        });

        let cancel_conversion_callback = self.on_cancel_conversion.clone();
        cancel_button.connect_clicked(move |_| {
            if let Some(f) = cancel_conversion_callback.borrow().as_ref() {
                f(id);
            }
        });

        self.list_box.append(&outer_box);
        self.rows.borrow_mut().insert(
            id,
            ProgressRow {
                outer_box,
                status_icon,
                status_label,
                fps_label,
                progress_bar,
                time_start_label,
                time_elapsed_label,
                cancel_button,
                details_button,
                cmd_label,
                stderr_view,
                stderr_scroll,
                stderr_header,
                state: std::cell::Cell::new(RowState::Pending),
                start_time: std::cell::Cell::new(None),
                progress_frac: std::cell::Cell::new(0.0),
            },
        );
    }
}
