//! Relm4 wrapper around [`ProgressPage`].
//!
//! Translates the progress page's widget callbacks into typed
//! [`BigConverterProgressOutput`] messages and applies
//! [`BigConverterProgressInput`] to the wrapped page. Manual `impl Component`
//! idiom (no `view!` macro), matching the rest of this crate.

use std::path::{Path, PathBuf};

use relm4::{Component, ComponentParts, ComponentSender};

use crate::ffmpeg::progress::FfmpegProgressSnapshot;

use super::ProgressPage;

/// Messages accepted by [`BigConverterProgress`].
#[derive(Debug)]
pub enum BigConverterProgressInput {
    /// Replace the row set with `(conversion_id, path)` pairs, preserving order.
    SetConversions(Vec<(u64, PathBuf)>),
    /// Record the FFmpeg command line for a conversion and mark it active.
    SetCommand(u64, String),
    /// Replace the stderr buffer for a conversion.
    SetStderrLog(u64, Vec<String>),
    /// Append one stderr line for a conversion.
    AppendStderrLine(u64, String),
    /// Update progress bar, speed/fps/ETA and elapsed time for a conversion.
    UpdateProgress(u64, FfmpegProgressSnapshot),
    /// Mark a conversion completed.
    MarkDone(u64),
    /// Mark a conversion failed with an error description.
    MarkFailed(u64, String),
    /// Mark a conversion cancelled.
    MarkCancelled(u64),
    /// Show the completion banner with the final tally.
    ShowComplete,
    /// Update the headline with the file currently being converted.
    SetCurrentFile(String),
    /// Update the headline with the number of parallel conversions.
    SetCurrentParallel(usize),
    /// Reset every row and clear counters.
    Reset,
}

/// User actions emitted by [`BigConverterProgress`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigConverterProgressOutput {
    /// "Back to queue" clicked after completion.
    Back,
    /// "Cancel all" confirmed.
    CancelAll,
    /// A single conversion row's cancel button was clicked.
    CancelConversion(u64),
}

/// Relm4 wrapper around [`ProgressPage`].
pub struct BigConverterProgress {
    page: ProgressPage,
}

impl Component for BigConverterProgress {
    type Init = ();
    type Input = BigConverterProgressInput;
    type Output = BigConverterProgressOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let page = ProgressPage::build_into(root.clone());

        let s = sender.clone();
        page.connect_back(move || {
            let _ = s.output(BigConverterProgressOutput::Back);
        });
        let s = sender.clone();
        page.connect_cancel_all(move || {
            let _ = s.output(BigConverterProgressOutput::CancelAll);
        });
        let s = sender;
        page.connect_cancel_conversion(move |conversion_id| {
            let _ = s.output(BigConverterProgressOutput::CancelConversion(conversion_id));
        });

        ComponentParts {
            model: Self { page },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            BigConverterProgressInput::SetConversions(conversions) => {
                let conversion_refs: Vec<(u64, &Path)> = conversions
                    .iter()
                    .map(|(conversion_id, path)| (*conversion_id, path.as_path()))
                    .collect();
                self.page.set_conversions(&conversion_refs);
            }
            BigConverterProgressInput::SetCommand(conversion_id, command) => {
                self.page.set_command(conversion_id, &command);
            }
            BigConverterProgressInput::SetStderrLog(conversion_id, lines) => {
                self.page.set_stderr_log(conversion_id, &lines);
            }
            BigConverterProgressInput::AppendStderrLine(conversion_id, line) => {
                self.page.append_stderr_line(conversion_id, &line);
            }
            BigConverterProgressInput::UpdateProgress(conversion_id, progress_snapshot) => {
                self.page.update_progress(conversion_id, &progress_snapshot);
            }
            BigConverterProgressInput::MarkDone(conversion_id) => {
                self.page.mark_done(conversion_id);
            }
            BigConverterProgressInput::MarkFailed(conversion_id, error) => {
                self.page.mark_failed(conversion_id, &error);
            }
            BigConverterProgressInput::MarkCancelled(conversion_id) => {
                self.page.mark_cancelled(conversion_id);
            }
            BigConverterProgressInput::ShowComplete => self.page.show_complete(),
            BigConverterProgressInput::SetCurrentFile(name) => self.page.set_current_file(&name),
            BigConverterProgressInput::SetCurrentParallel(active) => {
                self.page.set_current_parallel(active);
            }
            BigConverterProgressInput::Reset => self.page.reset(),
        }
    }
}
