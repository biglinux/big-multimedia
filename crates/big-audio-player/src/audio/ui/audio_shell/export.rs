//! In-player quick export for the audio player: convert or trim-and-export the
//! playing track via the shared converter engine (`big_converter_ui::export`),
//! off the GTK main loop. No external converter process.

use std::path::{Path, PathBuf};

use big_converter_ui::export::{run_single_export_blocking, QuickExportChoice, QuickExportFormat};
use big_media_runtime::converter::profiles::VideoFilters;

use super::*;

/// `(label, codec)` for the audio output chooser; container is derived from the
/// codec by the export engine.
const AUDIO_FORMATS: [(&str, &str); 3] = [("MP3", "mp3"), ("M4A", "aac"), ("FLAC", "flac")];

impl AudioShell {
    /// Convert the playing track to `codec` (mp3/aac/flac) beside it.
    pub(super) fn convert_track(&self, codec: &str) {
        let choice = QuickExportChoice {
            format: QuickExportFormat::Audio {
                codec: codec.to_string(),
                bitrate: "192k".into(),
            },
            filters: VideoFilters::default(),
        };
        self.run_track_export(choice, Vec::new(), &i18n("Converting…"));
    }

    /// Open the Trim & Export dialog: mark in/out (snappable to the playhead),
    /// pick an audio format, and export just that range beside the source.
    pub(super) fn show_trim_export(&self) {
        let input = self.current_file.clone();
        if input.is_empty() || input.contains("://") {
            self.notify(&i18n("No local track is playing"));
            return;
        }
        let input = PathBuf::from(&input);
        let output_dir = input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let mpv = self.mpv.clone();
        let duration = mpv
            .as_ref()
            .map_or(0.0, |m| m.get_property_f64("duration"))
            .max(1.0);

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

        let labels: Vec<&str> = AUDIO_FORMATS.iter().map(|(label, _)| *label).collect();
        let format_model = gtk::StringList::new(&labels);
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
            .title(i18n("Trim & Export"))
            .modal(true)
            .default_width(440)
            .content(&toolbar)
            .build();
        window.set_transient_for(self.window().as_ref());

        let toast_overlay = self.toast_overlay.clone();
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
                let codec = AUDIO_FORMATS
                    .get(format_row.selected() as usize)
                    .map_or("mp3", |(_, codec)| *codec);
                let choice = QuickExportChoice {
                    format: QuickExportFormat::Audio {
                        codec: codec.to_string(),
                        bitrate: "192k".into(),
                    },
                    filters: VideoFilters::default(),
                };
                let input = input.clone();
                let output_dir = output_dir.clone();
                let done_overlay = toast_overlay.clone();
                toast_overlay.add_toast(adw::Toast::new(&i18n("Exporting clip…")));
                big_media_components::player_chrome::spawn_blocking_result(
                    move || run_single_export_blocking(input, output_dir, &choice, &[(start, end)]),
                    move |result| done_overlay.add_toast(adw::Toast::new(&export_message(&result))),
                );
                window.close();
            }
        ));

        window.present();
    }

    /// Resolve the current local track, then run `choice` (with optional trim
    /// `segments`) on a worker thread and toast the outcome.
    fn run_track_export(
        &self,
        choice: QuickExportChoice,
        segments: Vec<(f64, f64)>,
        busy_message: &str,
    ) {
        let input = self.current_file.clone();
        if input.is_empty() || input.contains("://") {
            self.notify(&i18n("No local track is playing"));
            return;
        }
        let input = PathBuf::from(&input);
        let output_dir = input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        self.notify(busy_message);
        let toast_overlay = self.toast_overlay.clone();
        big_media_components::player_chrome::spawn_blocking_result(
            move || run_single_export_blocking(input, output_dir, &choice, &segments),
            move |result| toast_overlay.add_toast(adw::Toast::new(&export_message(&result))),
        );
    }
}

/// Human-readable toast for a finished export.
fn export_message(result: &Result<PathBuf, String>) -> String {
    match result {
        Ok(path) => format!(
            "{} {}",
            i18n("Saved"),
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
        ),
        Err(error) => format!("{}: {error}", i18n("Export failed")),
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
