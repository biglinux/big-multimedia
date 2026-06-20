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
