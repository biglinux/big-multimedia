//! ProgressPage update API: per-conversion state transitions (Active/Completed/
//! Failed/Cancelled), live ffmpeg stats (speed/fps/ETA/size), stderr stream
//! into expandable text view, overall bar combines done count with active
//! conversion fractions for smooth progression in parallel conversions.

use std::path::Path;
use std::time::Instant;

use adw::prelude::*;
use gtk::glib;

use crate::bytes;
use crate::ffmpeg::progress::FfmpegProgressSnapshot;
use crate::text::text as i18n;

use super::row_state::extract_encoder_label;
use super::{ProgressPage, RowState};

impl ProgressPage {
    /// Replace the row set with `conversions` (each `(conversion_id, path)`), preserving order.
    pub fn set_conversions(&self, conversions: &[(u64, &Path)]) {
        self.list_box.clear();
        self.rows.borrow_mut().clear();
        *self.total_items.borrow_mut() = conversions.len();
        *self.done_count.borrow_mut() = 0;
        *self.failed_count.borrow_mut() = 0;
        *self.cancelled_count.borrow_mut() = 0;

        self.overall_bar.set_fraction(0.0);
        self.overall_bar.set_text(Some("0%"));
        self.overall_bar.set_visible(conversions.len() > 1);
        self.overall_label
            .set_label(&format!("{} (0/{})", i18n("Converting"), conversions.len()));
        self.cancel_all_button.set_visible(true);
        self.back_button.set_visible(false);
        self.banner.set_revealed(false);

        for &(conversion_id, path) in conversions {
            self.add_row(conversion_id, path);
        }
    }

    /// Record the FFmpeg command line for `conversion_id` and update its encoder label.
    pub fn set_command(&self, conversion_id: u64, command: &str) {
        let rows = self.rows.borrow();
        if let Some(progress_row) = rows.get(&conversion_id) {
            progress_row.cmd_label.set_label(command);

            let encoder_label = extract_encoder_label(command);
            let status_text = if encoder_label.is_empty() {
                i18n("Converting...")
            } else {
                format!("{} — {}", i18n("Converting..."), encoder_label)
            };

            Self::set_row_state(progress_row, RowState::Active);
            progress_row.start_time.set(Some(Instant::now()));
            progress_row.status_label.set_label(&status_text);
            progress_row.progress_bar.set_visible(true);
            progress_row.progress_bar.set_fraction(0.0);

            if let Ok(now) = glib::DateTime::now_local()
                && let Ok(ts) = now.format("%H:%M:%S")
            {
                progress_row.time_start_label.set_label(&ts);
            }
        }
    }

    /// Replace the stderr buffer for `conversion_id` with `lines`.
    pub fn set_stderr_log(&self, conversion_id: u64, lines: &[String]) {
        let rows = self.rows.borrow();
        if let Some(progress_row) = rows.get(&conversion_id) {
            let buf = progress_row.stderr_view.buffer();
            let text = lines.join("\n");
            buf.set_text(&text);
            let has_content = !text.is_empty();
            progress_row.stderr_scroll.set_visible(has_content);
            progress_row.stderr_header.set_visible(has_content);
        }
    }

    /// Append one stderr line to the buffer for `conversion_id`.
    pub fn append_stderr_line(&self, conversion_id: u64, line: &str) {
        let rows = self.rows.borrow();
        if let Some(progress_row) = rows.get(&conversion_id) {
            let buf = progress_row.stderr_view.buffer();
            let mut end = buf.end_iter();
            let prefix = if buf.char_count() > 0 { "\n" } else { "" };
            buf.insert(&mut end, &format!("{prefix}{line}"));
            progress_row.stderr_scroll.set_visible(true);
            progress_row.stderr_header.set_visible(true);
            let mark = buf.create_mark(None, &buf.end_iter(), false);
            progress_row.stderr_view.scroll_mark_onscreen(&mark);
            buf.delete_mark(&mark);
        }
    }

    /// Update progress bar, FPS, and elapsed time for `conversion_id`.
    pub fn update_progress(&self, conversion_id: u64, progress_snapshot: &FfmpegProgressSnapshot) {
        let rows = self.rows.borrow();
        if let Some(progress_row) = rows.get(&conversion_id) {
            Self::set_row_state(progress_row, RowState::Active);

            let percent = (progress_snapshot.percent / 100.0).clamp(0.0, 1.0);
            progress_row.progress_bar.set_visible(true);
            progress_row.progress_bar.set_fraction(percent);
            progress_row.progress_frac.set(percent);

            let mut parts = Vec::new();
            if progress_snapshot.speed > 0.0 {
                parts.push(format!("{:.1}x", progress_snapshot.speed));
            }
            if progress_snapshot.fps > 0.0 {
                parts.push(format!("{:.0} fps", progress_snapshot.fps));
            }
            if progress_snapshot.eta_secs > 0.0 {
                let eta = progress_snapshot.eta_secs as u64;
                let minutes = eta / 60;
                let seconds = eta % 60;
                parts.push(format!("ETA {minutes}:{seconds:02}"));
            }
            if progress_snapshot.size_bytes > 0 {
                parts.push(bytes::decimal_compact(progress_snapshot.size_bytes));
            }
            let status_text = if parts.is_empty() {
                i18n("Converting...")
            } else {
                parts.join(" · ")
            };
            progress_row.status_label.set_label(&status_text);
            Self::set_elapsed(progress_row);
        }

        self.update_overall();
    }

    /// Switch row `conversion_id` to the completed state.
    pub fn mark_done(&self, conversion_id: u64) {
        let rows = self.rows.borrow();
        if let Some(progress_row) = rows.get(&conversion_id) {
            Self::set_row_state(progress_row, RowState::Completed);
            progress_row.status_label.set_label(&i18n("Completed"));
            progress_row.status_label.set_css_classes(&["success"]);
            progress_row.progress_frac.set(1.0);
        }
        *self.done_count.borrow_mut() += 1;
        self.update_overall();
    }

    /// Switch row `conversion_id` to the failed state with an `error` description.
    pub fn mark_failed(&self, conversion_id: u64, error: &str) {
        let rows = self.rows.borrow();
        if let Some(progress_row) = rows.get(&conversion_id) {
            Self::set_row_state(progress_row, RowState::Failed);
            progress_row.status_label.set_label(error);
            progress_row.status_label.set_css_classes(&["error"]);
        }
        *self.failed_count.borrow_mut() += 1;
        self.update_overall();
    }

    /// Switch row `conversion_id` to the cancelled state.
    pub fn mark_cancelled(&self, conversion_id: u64) {
        let rows = self.rows.borrow();
        if let Some(progress_row) = rows.get(&conversion_id) {
            Self::set_row_state(progress_row, RowState::Cancelled);
            progress_row.status_label.set_label(&i18n("Cancelled"));
            progress_row.status_label.set_css_classes(&["warning"]);
        }
        *self.cancelled_count.borrow_mut() += 1;
        self.update_overall();
    }

    /// Show the completion banner with the final tally of done/failed/cancelled rows.
    pub fn show_complete(&self) {
        let done = *self.done_count.borrow();
        let failed = *self.failed_count.borrow();
        let cancelled = *self.cancelled_count.borrow();

        let mut parts = Vec::new();
        if done > 0 {
            parts.push(format!("{done} {}", i18n("completed")));
        }
        if failed > 0 {
            parts.push(format!("{failed} {}", i18n("failed")));
        }
        if cancelled > 0 {
            parts.push(format!("{cancelled} {}", i18n("cancelled")));
        }
        let summary = if parts.is_empty() {
            i18n("No files processed")
        } else {
            parts.join(", ")
        };

        self.overall_label
            .set_label(&format!("{} — {summary}", i18n("Complete")));
        self.overall_bar.set_fraction(1.0);
        self.overall_bar.set_text(Some("100%"));

        if failed == 0 && cancelled == 0 {
            self.banner
                .set_title(&i18n("All videos converted successfully!"));
            self.banner.add_css_class("success");
        } else if done > 0 {
            self.banner.set_title(&summary);
        } else {
            self.banner
                .set_title(&i18n("All conversions failed or were cancelled"));
            self.banner.add_css_class("error");
        }
        self.banner.set_revealed(true);

        self.cancel_all_button.set_visible(false);
        self.back_button.set_visible(true);
    }

    /// Update the headline that announces which file is currently being converted.
    pub fn set_current_file(&self, name: &str) {
        let total = *self.total_items.borrow();
        let finished = *self.done_count.borrow()
            + *self.failed_count.borrow()
            + *self.cancelled_count.borrow();
        self.overall_label.set_label(&format!(
            "{} ({}/{}) — {}",
            i18n("Converting"),
            finished + 1,
            total,
            name
        ));
    }

    /// Update the headline with the number of parallel conversions active.
    pub fn set_current_parallel(&self, active: usize) {
        let total = *self.total_items.borrow();
        let finished = *self.done_count.borrow()
            + *self.failed_count.borrow()
            + *self.cancelled_count.borrow();
        self.overall_label.set_label(&format!(
            "{} ({}/{}) — {} {}",
            i18n("Converting"),
            finished + active,
            total,
            active,
            i18n("in parallel"),
        ));
    }

    /// Reset every row to the pending state and clear counters.
    pub fn reset(&self) {
        self.list_box.clear();
        self.rows.borrow_mut().clear();
        self.overall_bar.set_fraction(0.0);
        self.overall_bar.set_text(Some("0%"));
        self.overall_label.set_label(&i18n("Ready"));
        self.cancel_all_button.set_visible(true);
        self.back_button.set_visible(false);
        self.banner.set_revealed(false);
        self.banner.remove_css_class("success");
        self.banner.remove_css_class("error");
        *self.total_items.borrow_mut() = 0;
        *self.done_count.borrow_mut() = 0;
        *self.failed_count.borrow_mut() = 0;
        *self.cancelled_count.borrow_mut() = 0;
    }

    // (total, finished) for window-title formatting.
    #[must_use]
    /// Return `(completed, total)` row counts.
    pub fn progress_counts(&self) -> (usize, usize) {
        let finished = *self.done_count.borrow()
            + *self.failed_count.borrow()
            + *self.cancelled_count.borrow();
        (*self.total_items.borrow(), finished)
    }

    fn update_overall(&self) {
        let total = *self.total_items.borrow();
        if total <= 1 {
            self.overall_bar.set_visible(false);
            return;
        }
        self.overall_bar.set_visible(true);

        let finished = *self.done_count.borrow()
            + *self.failed_count.borrow()
            + *self.cancelled_count.borrow();

        let rows = self.rows.borrow();
        let active_progress: f64 = rows
            .values()
            .filter(|row| row.state.get() == RowState::Active)
            .map(|row| row.progress_frac.get())
            .sum();

        let frac = (finished as f64 + active_progress) / total as f64;
        self.overall_bar.set_fraction(frac.clamp(0.0, 1.0));
        self.overall_bar
            .set_text(Some(&format!("{:.0}%", frac * 100.0)));
    }
}
