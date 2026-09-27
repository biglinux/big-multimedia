//! Relm4 download-queue panel — a slide-up revealer with a per-download
//! progress row, driven by a 500 ms tick.
//!
//! Self-contained Component: it owns the task list, the HTTP worker spawns
//! (shared `transfer` worker), and the tick timer. The shell only sends
//! `DownloadQueueInput::Enqueue` (after picking a destination); everything
//! else — progress, cancel, clear, hide — is internal. Manual `impl Component`
//! idiom.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use adw::prelude::*;
use big_media_components::bytes;
use big_relm4_components::feedback::tooltip;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::download::transfer::download_file;
use crate::download::{DownloadProgress, DownloadStatus, DownloadTask};
use big_media_data::i18n::i18n;

/// Messages accepted by [`DownloadQueuePanel`].
#[derive(Debug)]
pub enum DownloadQueueInput {
    /// Start downloading `url` (display name `title`) to `dest`.
    Enqueue {
        /// Source URL.
        url: String,
        /// Display title for the row.
        title: String,
        /// Destination file path.
        dest: PathBuf,
    },
    /// Internal 500 ms tick: refresh progress rows.
    Tick,
    /// Remove finished/cancelled/failed rows; hide if none remain.
    ClearFinished,
    /// Hide the panel.
    Hide,
}

/// Relm4 download-queue panel component.
pub struct DownloadQueuePanel {
    revealer: gtk::Revealer,
    list_box: gtk::Box,
    tasks: Vec<Rc<DownloadTask>>,
    timer: Option<gtk::glib::SourceId>,
}

impl Drop for DownloadQueuePanel {
    fn drop(&mut self) {
        for task in &self.tasks {
            if task.status.get() == DownloadStatus::Downloading {
                task.progress.cancelled.store(true, Ordering::Relaxed);
            }
        }
        if let Some(timer) = self.timer.take() {
            timer.remove();
        }
    }
}

impl Component for DownloadQueuePanel {
    type Init = ();
    type Input = DownloadQueueInput;
    type Output = ();
    type CommandOutput = ();
    type Root = gtk::Revealer;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .transition_duration(200)
            .reveal_child(false)
            .build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let panel = gtk::Box::new(gtk::Orientation::Vertical, 0);

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        header.set_margin_start(12);
        header.set_margin_end(8);
        header.set_margin_top(6);
        header.set_margin_bottom(2);

        let header_icon = gtk::Image::from_icon_name("folder-download-symbolic");
        header_icon.add_css_class("dim-label");
        header.append(&header_icon);

        let header_label = gtk::Label::builder()
            .label(i18n("Downloads"))
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["heading"])
            .build();
        header.append(&header_label);

        let clear_button = tooltip::icon_button(
            "edit-clear-all-symbolic",
            &i18n("Clear completed downloads"),
            &[],
        );
        clear_button.set_has_frame(false);
        let s = sender.clone();
        clear_button.connect_clicked(move |_| s.input(DownloadQueueInput::ClearFinished));
        header.append(&clear_button);

        let close_button = tooltip::icon_button(
            "window-close-symbolic",
            &i18n("Hide downloads panel"),
            &["circular"],
        );
        close_button.set_has_frame(false);
        let s = sender.clone();
        close_button.connect_clicked(move |_| s.input(DownloadQueueInput::Hide));
        header.append(&close_button);

        panel.append(&header);
        panel.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        let list_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        list_box.set_margin_top(4);
        list_box.set_margin_bottom(4);
        list_box.set_margin_start(12);
        list_box.set_margin_end(12);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vscrollbar_policy(gtk::PolicyType::Automatic)
            .min_content_height(140)
            .max_content_height(280)
            .child(&list_box)
            .build();
        panel.append(&scroll);

        root.set_child(Some(&panel));

        ComponentParts {
            model: Self {
                revealer: root.clone(),
                list_box,
                tasks: Vec::new(),
                timer: None,
            },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            DownloadQueueInput::Enqueue { url, title, dest } => {
                self.enqueue(&url, &title, &dest, &sender);
            }
            DownloadQueueInput::Tick => {
                if self.tick_downloads()
                    && let Some(id) = self.timer.take()
                {
                    id.remove();
                }
            }
            DownloadQueueInput::ClearFinished => self.clear_finished(),
            DownloadQueueInput::Hide => self.revealer.set_reveal_child(false),
        }
    }
}

impl DownloadQueuePanel {
    /// Build a progress row, register the task, reveal the panel, and spawn the
    /// streamed HTTP download on a worker thread.
    fn enqueue(&mut self, url: &str, title: &str, dest: &Path, sender: &ComponentSender<Self>) {
        let progress = Arc::new(DownloadProgress {
            bytes_downloaded: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            finished: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            error_msg: std::sync::Mutex::new(None),
        });

        let row_box = gtk::Box::new(gtk::Orientation::Vertical, 2);
        row_box.set_margin_top(4);
        row_box.set_margin_bottom(4);
        row_box.add_css_class("download-panel-row");

        let header = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let title_label = gtk::Label::builder()
            .label(title)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(40)
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["caption-heading"])
            .build();
        let cancel_button = tooltip::icon_button(
            "process-stop-symbolic",
            &i18n("Cancel download"),
            &["circular"],
        );
        header.append(&title_label);
        header.append(&cancel_button);

        let progress_bar = gtk::ProgressBar::new();
        progress_bar.set_show_text(false);

        let status_label = gtk::Label::builder()
            .label(i18n("Starting…"))
            .xalign(0.0)
            .css_classes(["caption", "dim-label"])
            .build();

        row_box.append(&header);
        row_box.append(&progress_bar);
        row_box.append(&status_label);

        let prog_ref = Arc::clone(&progress);
        cancel_button.connect_clicked(move |button| {
            if !prog_ref.finished.load(Ordering::Relaxed) {
                prog_ref.cancelled.store(true, Ordering::Relaxed);
                button.set_sensitive(false);
            }
        });

        let task = Rc::new(DownloadTask {
            title: title.to_string(),
            progress: Arc::clone(&progress),
            status: Cell::new(DownloadStatus::Downloading),
            row_box: row_box.clone(),
            progress_bar,
            status_label,
            cancel_button,
        });

        self.list_box.append(&row_box);
        self.tasks.push(Rc::clone(&task));
        self.revealer.set_reveal_child(true);
        self.ensure_timer(sender);

        let url = url.to_string();
        let dest = dest.to_path_buf();
        let progress_cb = Arc::clone(&progress);
        big_media_components::player_chrome::spawn_blocking_result(
            move || download_file(&url, &dest, &progress),
            move |result| {
                if result.is_ok() {
                    // Publication completed before a late click reached the main loop.
                    progress_cb.cancelled.store(false, Ordering::Relaxed);
                }
                if let Err(e) = result {
                    *progress_cb
                        .error_msg
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(e);
                }
                progress_cb.finished.store(true, Ordering::Relaxed);
            },
        );
    }

    /// Start the 500 ms progress tick if it is not already running.
    fn ensure_timer(&mut self, sender: &ComponentSender<Self>) {
        if self.timer.is_some() {
            return;
        }
        let s = sender.clone();
        let id = gtk::glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            s.input(DownloadQueueInput::Tick);
            gtk::glib::ControlFlow::Continue
        });
        self.timer = Some(id);
    }

    /// Refresh every in-flight row; return `true` once none are downloading.
    fn tick_downloads(&self) -> bool {
        let mut all_done = true;
        for task in &self.tasks {
            if task.status.get() != DownloadStatus::Downloading {
                continue;
            }
            let prog = &task.progress;

            if prog.finished.load(Ordering::Relaxed) {
                let err = prog
                    .error_msg
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if prog.cancelled.load(Ordering::Relaxed) {
                    task.status.set(DownloadStatus::Cancelled);
                    task.status_label.set_label(&i18n("Cancelled"));
                    task.progress_bar.set_fraction(0.0);
                    task.cancel_button.set_visible(false);
                } else if let Some(e) = err {
                    task.status.set(DownloadStatus::Failed);
                    task.status_label.set_label(&format!("⚠ {e}"));
                    task.cancel_button.set_visible(false);
                    log::warn!("Download failed for {}: {e}", task.title);
                } else {
                    task.status.set(DownloadStatus::Complete);
                    task.status_label
                        .set_label(&format!("✓ {}", i18n("Complete")));
                    task.progress_bar.set_fraction(1.0);
                    task.progress_bar.add_css_class("accent");
                    task.cancel_button.set_visible(false);
                }
                continue;
            }

            all_done = false;

            let downloaded = prog.bytes_downloaded.load(Ordering::Relaxed);
            let total = prog.total_bytes.load(Ordering::Relaxed);
            if total > 0 {
                let frac = downloaded as f64 / total as f64;
                task.progress_bar.set_fraction(frac.min(1.0));
                task.status_label.set_label(&format!(
                    "{} / {}  ({}%)",
                    bytes::binary_transfer(downloaded),
                    bytes::binary_transfer(total),
                    (frac * 100.0) as u32,
                ));
            } else {
                task.progress_bar.pulse();
                if downloaded > 0 {
                    task.status_label
                        .set_label(&format!("{}…", bytes::binary_transfer(downloaded)));
                }
            }
        }
        all_done
    }

    /// Drop every finished/cancelled/failed row; hide the panel when empty.
    fn clear_finished(&mut self) {
        let list = &self.list_box;
        self.tasks.retain(|t| {
            if t.status.get() == DownloadStatus::Downloading {
                true
            } else {
                list.remove(&t.row_box);
                false
            }
        });
        if self.tasks.is_empty() {
            self.revealer.set_reveal_child(false);
        }
    }
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;

    #[test]
    #[ignore = "requires an isolated GTK session"]
    fn dropping_download_panel_removes_progress_source() {
        gtk::init().unwrap();
        let context = gtk::glib::MainContext::default();
        for _ in 0..25 {
            let timer = gtk::glib::timeout_add_local(std::time::Duration::from_secs(60), || {
                gtk::glib::ControlFlow::Continue
            });
            let source = context.find_source_by_id(&timer).unwrap();
            let progress = Arc::new(DownloadProgress {
                bytes_downloaded: AtomicU64::new(0),
                total_bytes: AtomicU64::new(0),
                finished: AtomicBool::new(false),
                cancelled: AtomicBool::new(false),
                error_msg: std::sync::Mutex::new(None),
            });
            let panel = DownloadQueuePanel {
                revealer: gtk::Revealer::new(),
                list_box: gtk::Box::new(gtk::Orientation::Vertical, 0),
                tasks: vec![Rc::new(DownloadTask {
                    title: String::new(),
                    progress: progress.clone(),
                    status: Cell::new(DownloadStatus::Downloading),
                    row_box: gtk::Box::new(gtk::Orientation::Vertical, 0),
                    progress_bar: gtk::ProgressBar::new(),
                    status_label: gtk::Label::new(None),
                    cancel_button: gtk::Button::new(),
                })],
                timer: Some(timer),
            };
            drop(panel);
            assert!(
                progress.cancelled.load(Ordering::Relaxed),
                "worker was not cancelled"
            );
            assert!(
                source.is_destroyed(),
                "download progress source outlived its panel"
            );
        }
    }
}
