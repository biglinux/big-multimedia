//! Progress page — per-file queue rows with status CSS, FPS, elapsed time,
//! per-file cancel, Cancel All (confirmed), expandable FFmpeg command+log,
//! AdwBanner on completion, AdwClamp for responsive layout.

mod actions;
mod build;
mod component;
mod row_state;
mod updates;

pub use component::{BigConverterProgress, BigConverterProgressInput, BigConverterProgressOutput};

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use big_relm4_components::list::bounded_list_box::BigBoundedListBox;

type ProgressCallback = Rc<RefCell<Option<Box<dyn Fn()>>>>;
type CancelConversionCallback = Rc<RefCell<Option<Box<dyn Fn(u64)>>>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowState {
    Pending,
    Active,
    Completed,
    Failed,
    Cancelled,
}

struct ProgressRow {
    outer_box: gtk::Box,
    status_icon: gtk::Image,
    status_label: gtk::Label,
    fps_label: gtk::Label,
    progress_bar: gtk::ProgressBar,
    time_start_label: gtk::Label,
    time_elapsed_label: gtk::Label,
    cancel_button: gtk::Button,
    details_button: gtk::ToggleButton,
    cmd_label: gtk::Label,
    stderr_view: gtk::TextView,
    stderr_scroll: gtk::ScrolledWindow,
    stderr_header: gtk::Label,
    state: Cell<RowState>,
    start_time: Cell<Option<Instant>>,
    progress_frac: Cell<f64>,
}

/// Per-job progress page rendered while a conversion is running.
///
/// Owns one row per queued file with progress bar, FPS, elapsed time,
/// per-file cancel button, and an expandable detail view that shows the
/// FFmpeg command line and the most recent stderr lines. Methods that
/// mutate row state ([`set_command`](Self::set_command),
/// [`update_progress`](Self::update_progress), [`mark_done`](Self::mark_done),
/// etc.) are safe to call from the GTK main thread only.
pub struct ProgressPage {
    container: gtk::Box,
    list_box: BigBoundedListBox,
    overall_label: gtk::Label,
    overall_bar: gtk::ProgressBar,
    cancel_all_button: gtk::Button,
    back_button: gtk::Button,
    banner: adw::Banner,
    rows: RefCell<HashMap<u64, ProgressRow>>,
    on_back: ProgressCallback,
    on_cancel_all: ProgressCallback,
    on_cancel_conversion: CancelConversionCallback,
    total_items: RefCell<usize>,
    done_count: RefCell<usize>,
    failed_count: RefCell<usize>,
    cancelled_count: RefCell<usize>,
}

impl Default for ProgressPage {
    fn default() -> Self {
        Self::new()
    }
}
