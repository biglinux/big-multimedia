//! Progress row CSS-class state machine + ffmpeg `-c:v` arg → user-facing encoder label.

use adw::prelude::*;

use super::{ProgressPage, ProgressRow, RowState};

impl ProgressPage {
    pub(super) fn set_row_state(pr: &ProgressRow, state: RowState) {
        let old = pr.state.get();
        if old == state {
            return;
        }
        pr.state.set(state);

        for cls in [
            "queue-row-pending",
            "queue-row-active",
            "queue-row-completed",
            "queue-row-failed",
            "queue-row-cancelled",
        ] {
            pr.outer_box.remove_css_class(cls);
        }

        match state {
            RowState::Pending => {
                pr.outer_box.add_css_class("queue-row-pending");
                pr.status_icon
                    .set_icon_name(Some("content-loading-symbolic"));
                pr.status_icon.set_css_classes(&["dim-label"]);
                pr.cancel_button.set_visible(false);
                pr.fps_label.set_visible(false);
            }
            RowState::Active => {
                pr.outer_box.add_css_class("queue-row-active");
                pr.status_icon
                    .set_icon_name(Some("media-playback-start-symbolic"));
                pr.status_icon.set_css_classes(&["accent"]);
                pr.cancel_button.set_visible(true);
                pr.details_button.set_sensitive(true);
            }
            RowState::Completed => {
                pr.outer_box.add_css_class("queue-row-completed");
                pr.status_icon.set_icon_name(Some("emblem-ok-symbolic"));
                pr.status_icon.set_css_classes(&["success"]);
                pr.progress_bar.set_visible(false);
                pr.cancel_button.set_visible(false);
                pr.fps_label.set_visible(false);
                Self::set_elapsed(pr);
            }
            RowState::Failed => {
                pr.outer_box.add_css_class("queue-row-failed");
                pr.status_icon.set_icon_name(Some("dialog-error-symbolic"));
                pr.status_icon.set_css_classes(&["error"]);
                pr.progress_bar.set_visible(false);
                pr.cancel_button.set_visible(false);
                pr.fps_label.set_visible(false);
                Self::set_elapsed(pr);
            }
            RowState::Cancelled => {
                pr.outer_box.add_css_class("queue-row-cancelled");
                pr.status_icon
                    .set_icon_name(Some("action-unavailable-symbolic"));
                pr.status_icon.set_css_classes(&["warning"]);
                pr.progress_bar.set_visible(false);
                pr.cancel_button.set_visible(false);
                pr.fps_label.set_visible(false);
            }
        }
    }

    pub(super) fn set_elapsed(pr: &ProgressRow) {
        if let Some(start) = pr.start_time.get() {
            let elapsed = start.elapsed().as_secs();
            let minutes = elapsed / 60;
            let seconds = elapsed % 60;
            if minutes > 0 {
                pr.time_elapsed_label
                    .set_label(&format!("{minutes}m {seconds:02}s"));
            } else {
                pr.time_elapsed_label.set_label(&format!("{seconds}s"));
            }
        }
    }
}

pub(super) fn extract_encoder_label(command: &str) -> String {
    crate::ffmpeg::labels::video_encoder_label_from_command(command)
}

#[cfg(test)]
mod tests {
    use super::extract_encoder_label;

    #[test]
    fn extract_encoder_label_maps_known_encoder() {
        assert_eq!(
            extract_encoder_label("ffmpeg -i input.mp4 -c:v h264_nvenc -c:a aac out.mp4"),
            "H.264 NVIDIA"
        );
    }

    #[test]
    fn extract_encoder_label_returns_passthrough_for_unknown_encoder() {
        assert_eq!(
            extract_encoder_label("ffmpeg -c:v custom_encoder output.mp4"),
            "custom_encoder"
        );
    }

    #[test]
    fn extract_encoder_label_returns_empty_when_missing() {
        assert!(extract_encoder_label("ffmpeg -i input.mp4 output.mp4").is_empty());
    }
}
