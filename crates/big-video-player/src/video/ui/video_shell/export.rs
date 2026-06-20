//! In-player quick export: extract audio from / re-encode the playing file
//! using the shared converter engine (`big_converter_ui::export`), off the GTK
//! main loop. No external converter process, no leaving the player.

use std::path::{Path, PathBuf};

use big_converter_ui::export::{run_single_export_blocking, QuickExportChoice, QuickExportFormat};
use big_media_runtime::converter::profiles::VideoFilters;

use super::*;

impl VideoShell {
    /// Extract the playing file's audio to `codec` (mp3/aac/flac) beside it.
    pub(super) fn extract_audio(&self, codec: &str) {
        self.run_quick_export(
            QuickExportChoice {
                format: QuickExportFormat::Audio {
                    codec: codec.to_string(),
                    bitrate: "192k".into(),
                },
                filters: VideoFilters::default(),
            },
            &i18n("Extracting audio…"),
        );
    }

    /// Re-encode the playing file into `container` (mp4/mkv/webm) beside it.
    pub(super) fn quick_convert(&self, container: &str) {
        self.run_quick_export(
            QuickExportChoice {
                format: QuickExportFormat::Video {
                    container: container.to_string(),
                    codec: "h264".into(),
                    quality: "good".into(),
                },
                filters: VideoFilters::default(),
            },
            &i18n("Converting…"),
        );
    }

    /// Open the Clip & Export dialog: mark in/out (seeded from the playhead),
    /// pick a container, and export just that range beside the source file.
    pub(super) fn show_clip_export(&self, root: &adw::ApplicationWindow) {
        let Some(input) = self
            .mpv
            .borrow()
            .as_ref()
            .and_then(|mpv| mpv.get_current_file())
        else {
            self.toast_overlay
                .add_toast(adw::Toast::new(&i18n("No file is playing")));
            return;
        };
        if super::playback::is_remote_uri(&input) {
            self.toast_overlay
                .add_toast(adw::Toast::new(&i18n("Cannot export a network stream")));
            return;
        }
        let input = PathBuf::from(&input);
        let output_dir = input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let mpv = self.mpv.borrow().as_ref().cloned();
        let duration = if self.duration_secs > 1.0 {
            self.duration_secs
        } else {
            mpv.as_ref()
                .map_or(0.0, |m| m.get_property_f64("duration"))
                .max(1.0)
        };

        let start_row = adw::SpinRow::builder()
            .title(i18n("Start (seconds)"))
            .adjustment(&gtk::Adjustment::new(0.0, 0.0, duration, 1.0, 10.0, 0.0))
            .digits(2)
            .build();
        let end_row = adw::SpinRow::builder()
            .title(i18n("End (seconds)"))
            .adjustment(&gtk::Adjustment::new(
                duration, 0.0, duration, 1.0, 10.0, 0.0,
            ))
            .digits(2)
            .build();
        start_row.add_suffix(&playhead_button(&mpv, &start_row));
        end_row.add_suffix(&playhead_button(&mpv, &end_row));

        let clip_group = adw::PreferencesGroup::builder()
            .title(i18n("Clip range"))
            .description(i18n(
                "Use ⏱ to set a point to the current playback position",
            ))
            .build();
        clip_group.add(&start_row);
        clip_group.add(&end_row);

        let format_model = gtk::StringList::new(&["MP4", "MKV", "WebM"]);
        let format_row = adw::ComboRow::builder()
            .title(i18n("Format"))
            .model(&format_model)
            .build();
        let format_group = adw::PreferencesGroup::new();
        format_group.add(&format_row);

        let export_btn = gtk::Button::builder()
            .label(i18n("Export Clip"))
            .css_classes(["suggested-action", "pill"])
            .halign(gtk::Align::Center)
            .margin_top(12)
            .build();

        let page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        page.append(&clip_group);
        page.append(&format_group);
        page.append(&export_btn);

        let header = adw::HeaderBar::new();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&page));
        let window = adw::Window::builder()
            .title(i18n("Clip & Export"))
            .modal(true)
            .transient_for(root)
            .default_width(440)
            .content(&toolbar)
            .build();

        let toast_overlay = self.toast_overlay.clone();
        let containers = ["mp4", "mkv", "webm"];
        export_btn.connect_clicked(glib::clone!(
            #[weak]
            window,
            #[weak]
            start_row,
            #[weak]
            end_row,
            #[weak]
            format_row,
            move |_| {
                let start = start_row.value();
                let end = end_row.value();
                if end <= start {
                    toast_overlay.add_toast(adw::Toast::new(&i18n("End must be after start")));
                    return;
                }
                let container = containers
                    .get(format_row.selected() as usize)
                    .copied()
                    .unwrap_or("mp4");
                let choice = QuickExportChoice {
                    format: QuickExportFormat::Video {
                        container: container.to_string(),
                        codec: "h264".into(),
                        quality: "good".into(),
                    },
                    filters: VideoFilters::default(),
                };
                let input = input.clone();
                let output_dir = output_dir.clone();
                let done_overlay = toast_overlay.clone();
                toast_overlay.add_toast(adw::Toast::new(&i18n("Exporting clip…")));
                big_media_components::player_chrome::spawn_blocking_result(
                    move || run_single_export_blocking(input, output_dir, &choice, &[(start, end)]),
                    move |result| {
                        let message = match result {
                            Ok(path) => format!(
                                "{} {}",
                                i18n("Saved"),
                                path.file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or_default()
                            ),
                            Err(error) => format!("{}: {error}", i18n("Export failed")),
                        };
                        done_overlay.add_toast(adw::Toast::new(&message));
                    },
                );
                window.close();
            }
        ));

        window.present();
    }

    /// Resolve the current local file, then run `choice` on a worker thread and
    /// toast the outcome. No-op (with a toast) when nothing local is playing.
    fn run_quick_export(&self, choice: QuickExportChoice, busy_message: &str) {
        let Some(input) = self
            .mpv
            .borrow()
            .as_ref()
            .and_then(|mpv| mpv.get_current_file())
        else {
            self.toast_overlay
                .add_toast(adw::Toast::new(&i18n("No file is playing")));
            return;
        };
        if super::playback::is_remote_uri(&input) {
            self.toast_overlay
                .add_toast(adw::Toast::new(&i18n("Cannot export a network stream")));
            return;
        }
        let input = PathBuf::from(&input);
        let output_dir = input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

        self.toast_overlay.add_toast(adw::Toast::new(busy_message));
        let toast_overlay = self.toast_overlay.clone();
        big_media_components::player_chrome::spawn_blocking_result(
            move || run_single_export_blocking(input, output_dir, &choice, &[]),
            move |result| {
                let message = match result {
                    Ok(path) => format!(
                        "{} {}",
                        i18n("Saved"),
                        path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or_default()
                    ),
                    Err(error) => format!("{}: {error}", i18n("Export failed")),
                };
                toast_overlay.add_toast(adw::Toast::new(&message));
            },
        );
    }
}

/// A flat "current position" suffix button that snaps `row` to mpv's `time-pos`.
fn playhead_button(mpv: &Option<Arc<MpvBackend>>, row: &adw::SpinRow) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name("preferences-system-time-symbolic")
        .tooltip_text(i18n("Use current position"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let mpv = mpv.clone();
    let row = row.downgrade();
    button.connect_clicked(move |_| {
        if let (Some(mpv), Some(row)) = (mpv.as_ref(), row.upgrade()) {
            row.set_value(mpv.get_property_f64("time-pos"));
        }
    });
    button
}
