//! Conversion queue page list-view: per-row drag/play/edit/remove + right-click menu
//! (open folder / info / delete from disk), drag-and-drop reorder with
//! striped backgrounds, now-playing CSS class, empty-state StatusPage.

#![allow(clippy::needless_borrows_for_generic_args, clippy::type_complexity)]

use crate::bytes;
use adw::prelude::*;
use big_relm4_components::display::status_page::BigStatusPage;
use big_relm4_components::feedback::tooltip;
use big_relm4_components::list::bounded_list_box::{BigBoundedListBox, BigBoundedListBoxSpec};
use big_relm4_components::list::button_strip_row::{BigButtonStripRow, BigButtonStripRowSpec};
use big_relm4_components::state::empty::BigEmptyStateSpec;
use gtk::{gdk, gio, glib};
use relm4::{Component, ComponentController, Controller};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Once;

use crate::text::text as i18n;
use crate::time::duration_or_placeholder;

/// Distinguishes video- from audio-conversion sessions; controls which
/// empty-state status page is shown and whether the per-row video-editor
/// button is rendered. Audio rows hide the editor since trimming uses
/// a different surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigConversionQueueMode {
    /// Video conversion mode; rows show an "edit" button.
    Video,
    /// Audio conversion mode; rows hide the editor button.
    Audio,
}

impl BigConversionQueueMode {
    /// `true` if `self` is [`BigConversionQueueMode::Audio`].
    #[must_use]
    pub fn is_audio(self) -> bool {
        matches!(self, Self::Audio)
    }
}

/// One row in the conversion queue.
#[derive(Debug, Clone, PartialEq)]
pub struct BigConversionQueueEntry {
    /// Stable identifier the app uses to address this row.
    pub id: u64,
    /// Absolute input file path shown in the row.
    pub input: PathBuf,
    /// Duration in seconds, used for the row's metadata label; `0.0` is rendered as placeholder.
    pub duration_secs: f64,
}

/// Status icon shown at the right edge of a queue row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BigConversionQueueStatus {
    /// Job is waiting to start; spinner icon.
    Pending,
    /// Job is running; accent-coloured icon.
    Converting,
    /// Job finished successfully.
    Done,
    /// Job ended with a non-zero exit status.
    Failed,
    /// Job was cancelled by the user.
    Cancelled,
}

/// Scrollable list of [`BigConversionQueueEntry`] rows with drag-reorder and a popup menu.
pub struct QueuePage {
    mode: BigConversionQueueMode,
    container: gtk::Box,
    list_box: BigBoundedListBox,
    status_page: Controller<BigStatusPage>,
    rows: Rc<RefCell<HashMap<u64, QueueRow>>>,
    row_order: Rc<RefCell<Vec<u64>>>,
    on_remove: Rc<RefCell<Option<Box<dyn Fn(u64)>>>>,
    on_open_folder: Rc<RefCell<Option<Box<dyn Fn(PathBuf)>>>>,
    on_metadata: Rc<RefCell<Option<Box<dyn Fn(u64)>>>>,
    on_delete_disk: Rc<RefCell<Option<Box<dyn Fn(u64)>>>>,
    on_play: Rc<RefCell<Option<Box<dyn Fn(PathBuf)>>>>,
    on_select: Rc<RefCell<Option<Box<dyn Fn(PathBuf)>>>>,
    on_edit: Rc<RefCell<Option<Box<dyn Fn(PathBuf)>>>>,
    on_reorder: Rc<RefCell<Option<Box<dyn Fn(Vec<u64>)>>>>,
}

struct QueueRow {
    row: adw::ActionRow,
    status_icon: gtk::Image,
    play_button: gtk::Button,
    meta_label: gtk::Label,
}

impl Default for QueuePage {
    fn default() -> Self {
        Self::new(BigConversionQueueMode::Video)
    }
}

fn install_queue_css() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let css = gtk::CssProvider::new();
        css.load_from_string(
            r"
            .converter-queue-row-even {
                background: alpha(@window_fg_color, 0.03);
            }
            .converter-queue-row-odd {
                background: transparent;
            }
            .converter-queue-row-even:hover,
            .converter-queue-row-odd:hover {
                background: alpha(@accent_bg_color, 0.10);
            }
            .converter-queue-row.queue-now-playing {
                background: alpha(@accent_bg_color, 0.15);
            }
            .queue-meta-label {
                color: alpha(@window_fg_color, 0.70);
                font-variant-numeric: tabular-nums;
            }
            ",
        );
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
}

impl QueuePage {
    /// Build a queue widget tree for the given mode.
    pub fn new(mode: BigConversionQueueMode) -> Self {
        Self::build_into(gtk::Box::new(gtk::Orientation::Vertical, 0), mode)
    }

    /// Build the queue page into an existing [`gtk::Box`] root.
    ///
    /// Lets the Relm4 wrapper [`super::queue_component::BigConverterQueue`]
    /// reuse the component's own `Root` widget; [`new`](Self::new) delegates
    /// here with a fresh vertical box.
    #[must_use]
    pub fn build_into(container: gtk::Box, mode: BigConversionQueueMode) -> Self {
        install_queue_css();

        // Each literal must be a direct `i18n(...)` argument so xgettext can
        // extract it; wrapping the `if` inside `i18n(...)` hides both strings
        // from string extraction (they shipped untranslated).
        let title = if mode.is_audio() {
            i18n("No Audio Files")
        } else {
            i18n("No Video Files")
        };
        let icon = if mode.is_audio() {
            "folder-music-symbolic"
        } else {
            "folder-videos-symbolic"
        };
        let empty_spec = BigEmptyStateSpec::new(icon, title)
            .with_body(i18n("Drag files here or use the Add Files button"));
        let status_page = BigStatusPage::builder().launch(empty_spec).detach();
        status_page.widget().set_vexpand(true);

        let list_box = BigBoundedListBox::new(
            BigBoundedListBoxSpec::new(64)
                .css_class("boxed-list")
                .margins(12, 12, 12, 12)
                .visible(false),
        );

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        scroll.set_child(Some(list_box.root()));

        container.append(status_page.widget());
        container.append(&scroll);

        Self {
            mode,
            container,
            list_box,
            status_page,
            rows: Rc::new(RefCell::new(HashMap::new())),
            row_order: Rc::new(RefCell::new(Vec::new())),
            on_remove: Rc::new(RefCell::new(None)),
            on_open_folder: Rc::new(RefCell::new(None)),
            on_metadata: Rc::new(RefCell::new(None)),
            on_delete_disk: Rc::new(RefCell::new(None)),
            on_play: Rc::new(RefCell::new(None)),
            on_select: Rc::new(RefCell::new(None)),
            on_edit: Rc::new(RefCell::new(None)),
            on_reorder: Rc::new(RefCell::new(None)),
        }
    }

    /// Borrow the root [`gtk::Box`] for embedding.
    pub fn widget(&self) -> &gtk::Box {
        &self.container
    }

    /// Append `entry` as a new row at the bottom of the queue.
    pub fn add_entry(&self, entry: &BigConversionQueueEntry) {
        self.status_page.widget().set_visible(false);
        self.list_box.set_visible(true);

        let filename = entry
            .input
            .file_name()
            .map_or_else(|| "?".into(), |n| n.to_string_lossy().to_string());

        let parent = entry
            .input
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let size = entry
            .input
            .metadata()
            .map(|m| bytes::decimal_compact(m.len()))
            .unwrap_or_default();

        let row = BigButtonStripRow::new(
            BigButtonStripRowSpec::new(&filename).subtitle(format!("{parent} • {size}")),
        )
        .into_root();
        row.add_css_class("converter-queue-row");
        row.set_activatable(true);

        let entry_id = entry.id;
        let input_path = entry.input.clone();
        let ext = entry
            .input
            .extension()
            .and_then(|e| e.to_str())
            .map_or_else(|| "FILE".to_string(), str::to_uppercase);
        let duration = duration_or_placeholder(entry.duration_secs);

        let handle = gtk::Image::builder()
            .icon_name("list-drag-handle-symbolic")
            .css_classes(["dim-label"])
            .build();
        set_named_cursor(&handle, "grab");

        let drag_src = gtk::DragSource::builder()
            .actions(gdk::DragAction::MOVE)
            .build();
        let drag_entry_id = entry_id;
        drag_src.connect_prepare(move |_src, _x, _y| {
            Some(gdk::ContentProvider::for_value(
                &(drag_entry_id as i64).to_value(),
            ))
        });
        row.add_controller(drag_src);

        {
            // bigagents: app-local-dnd. Queue reorder transfers internal row ids.
            let dt = gtk::DropTarget::builder()
                .actions(gdk::DragAction::MOVE)
                .build();
            dt.set_types(&[glib::Type::I64]);
            let order_ref = self.row_order.clone();
            let lb = self.list_box.clone();
            let rows_map = self.rows.clone();
            let on_reorder_cb = self.on_reorder.clone();
            let target_id = entry_id;
            dt.connect_drop(move |_target, value, _x, _y| {
                let src_id = match value.get::<i64>() {
                    Ok(v) => v as u64,
                    Err(_) => return false,
                };
                if src_id == target_id {
                    return false;
                }
                let mut order = order_ref.borrow_mut();
                let src_pos = order.iter().position(|&id| id == src_id);
                let tgt_pos = order.iter().position(|&id| id == target_id);
                if let (Some(si), Some(ti)) = (src_pos, tgt_pos) {
                    let removed = order.remove(si);
                    let new_ti = order.iter().position(|&id| id == target_id).unwrap_or(ti);
                    order.insert(new_ti, removed);

                    let rows = rows_map.borrow();
                    if let (Some(src_qr), Some(tgt_qr)) = (rows.get(&src_id), rows.get(&target_id))
                    {
                        lb.remove(&src_qr.row);
                        let tgt_idx = tgt_qr.row.index();
                        lb.insert(&src_qr.row, tgt_idx);
                    }
                    for (index, row_id) in order.iter().enumerate() {
                        if let Some(qr) = rows.get(row_id) {
                            qr.row.remove_css_class("converter-queue-row-even");
                            qr.row.remove_css_class("converter-queue-row-odd");
                            if index % 2 == 0 {
                                qr.row.add_css_class("converter-queue-row-even");
                            } else {
                                qr.row.add_css_class("converter-queue-row-odd");
                            }
                        }
                    }
                    std::mem::drop(rows);
                    let new_order = order.clone();
                    std::mem::drop(order);
                    if let Some(f) = on_reorder_cb.borrow().as_ref() {
                        f(new_order);
                    }
                }
                true
            });
            row.add_controller(dt);
        }

        let play_button = tooltip::icon_button(
            "media-playback-start-symbolic",
            &i18n("Play"),
            &["flat", "circular"],
        );
        play_button.set_valign(gtk::Align::Center);
        set_named_cursor(&play_button, "pointer");
        {
            let cb = self.on_play.clone();
            let p = input_path.clone();
            play_button.connect_clicked(move |_| {
                if let Some(f) = cb.borrow().as_ref() {
                    f(p.clone());
                }
            });
        }

        {
            let cb = self.on_select.clone();
            let p = input_path.clone();
            row.connect_activated(move |_| {
                if let Some(f) = cb.borrow().as_ref() {
                    f(p.clone());
                }
            });
        }

        // Edit button: video mode only — audio mode has inline preview row.
        if !self.mode.is_audio() {
            let edit_button = tooltip::icon_button(
                "document-edit-symbolic",
                &i18n("Open video editor for this file"),
                &["flat", "circular"],
            );
            edit_button.update_property(&[gtk::accessible::Property::Label(&i18n("Edit file"))]);
            edit_button.set_valign(gtk::Align::Center);
            set_named_cursor(&edit_button, "pointer");
            {
                let cb = self.on_edit.clone();
                let p = input_path.clone();
                edit_button.connect_clicked(move |_| {
                    if let Some(f) = cb.borrow().as_ref() {
                        f(p.clone());
                    }
                });
            }
            row.add_prefix(&edit_button);
        }

        // add_prefix prepends — reverse visual order.
        row.add_prefix(&play_button);
        row.add_prefix(&handle);

        let remove_button = tooltip::icon_button(
            "user-trash-symbolic",
            &i18n("Remove from Queue"),
            &["flat", "circular"],
        );
        remove_button.set_valign(gtk::Align::Center);
        set_named_cursor(&remove_button, "pointer");
        {
            let cb = self.on_remove.clone();
            let lb = self.list_box.clone();
            let r = row.clone();
            remove_button.connect_clicked(move |_| {
                lb.remove(&r);
                if let Some(f) = cb.borrow().as_ref() {
                    f(entry_id);
                }
            });
        }
        row.add_suffix(&remove_button);

        let meta_label = gtk::Label::builder()
            .label(&format!("{ext} • {duration}"))
            .halign(gtk::Align::End)
            .valign(gtk::Align::Center)
            .build();
        meta_label.add_css_class("queue-meta-label");
        meta_label.add_css_class("dim-label");
        row.add_suffix(&meta_label);

        let status_icon = gtk::Image::from_icon_name("content-loading-symbolic");
        status_icon.set_visible(false);
        row.add_suffix(&status_icon);

        let menu = gio::Menu::new();
        menu.append(
            Some(&i18n("Open Containing Folder")),
            Some("row.open-folder"),
        );
        menu.append(Some(&i18n("More Information...")), Some("row.more-info"));
        menu.append(Some(&i18n("Delete from Disk...")), Some("row.delete-disk"));

        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_parent(&row);
        popover.set_has_arrow(true);
        // A parented popover is never unparented automatically; without this
        // the row finalizes with the popover still attached ("Finalizing ...,
        // but it still has children left") and the popover leaks.
        big_relm4_components::feedback::popover_lifecycle::unparent_popover_on_parent_teardown(
            row.upcast_ref(),
            &popover,
        );

        let action_group = gio::SimpleActionGroup::new();

        let cb = self.on_open_folder.clone();
        let p = input_path.clone();
        let open_folder = gio::SimpleAction::new("open-folder", None);
        open_folder.connect_activate(move |_, _| {
            if let Some(f) = cb.borrow().as_ref() {
                f(p.clone());
            }
        });
        action_group.add_action(&open_folder);

        let cb = self.on_metadata.clone();
        let metadata_action = gio::SimpleAction::new("more-info", None);
        metadata_action.connect_activate(move |_, _| {
            if let Some(f) = cb.borrow().as_ref() {
                f(entry_id);
            }
        });
        action_group.add_action(&metadata_action);

        let cb = self.on_delete_disk.clone();
        let delete_action = gio::SimpleAction::new("delete-disk", None);
        delete_action.connect_activate(move |_, _| {
            if let Some(f) = cb.borrow().as_ref() {
                f(entry_id);
            }
        });
        action_group.add_action(&delete_action);

        row.insert_action_group("row", Some(&action_group));

        let gesture = gtk::GestureClick::new();
        gesture.set_button(3);
        let pop = popover.clone();
        gesture.connect_released(move |g, _, x, y| {
            let rect = gdk::Rectangle::new(x as i32, y as i32, 1, 1);
            pop.set_pointing_to(Some(&rect));
            pop.popup();
            g.set_state(gtk::EventSequenceState::Claimed);
        });
        row.add_controller(gesture);

        self.list_box.append(&row);
        self.row_order.borrow_mut().push(entry.id);
        self.rows.borrow_mut().insert(
            entry.id,
            QueueRow {
                row,
                status_icon,
                play_button,
                meta_label,
            },
        );
        self.refresh_row_stripes();
    }

    /// Replace the status icon for `entry_id`.
    pub fn update_entry_status(&self, entry_id: u64, status: &BigConversionQueueStatus) {
        let rows = self.rows.borrow();
        if let Some(qr) = rows.get(&entry_id) {
            let (icon, css) = match status {
                BigConversionQueueStatus::Pending => ("content-loading-symbolic", "dim-label"),
                BigConversionQueueStatus::Converting => ("emblem-synchronizing-symbolic", "accent"),
                BigConversionQueueStatus::Done => ("emblem-ok-symbolic", "success"),
                BigConversionQueueStatus::Failed => ("dialog-error-symbolic", "error"),
                BigConversionQueueStatus::Cancelled => ("process-stop-symbolic", "dim-label"),
            };
            qr.status_icon.set_icon_name(Some(icon));
            qr.status_icon.set_css_classes(&[css]);
            qr.status_icon.set_visible(true);
        }
    }

    /// Hide every per-row status icon.
    pub fn reset_all_status_icons(&self) {
        let rows = self.rows.borrow();
        for qr in rows.values() {
            qr.status_icon.set_visible(false);
        }
    }

    /// Install the callback fired when the user clicks the remove button on a row.
    pub fn connect_remove_clicked<F: Fn(u64) + 'static>(&self, f: F) {
        *self.on_remove.borrow_mut() = Some(Box::new(f));
    }

    /// Install the callback fired when the user picks "Open containing folder".
    pub fn connect_open_folder<F: Fn(PathBuf) + 'static>(&self, f: F) {
        *self.on_open_folder.borrow_mut() = Some(Box::new(f));
    }

    /// Install the callback fired when the user picks "More information…".
    pub fn connect_metadata<F: Fn(u64) + 'static>(&self, f: F) {
        *self.on_metadata.borrow_mut() = Some(Box::new(f));
    }

    /// Install the callback fired when the user picks "Delete from disk…".
    pub fn connect_delete_disk<F: Fn(u64) + 'static>(&self, f: F) {
        *self.on_delete_disk.borrow_mut() = Some(Box::new(f));
    }

    /// Install the callback fired when the user clicks the play button on a row.
    pub fn connect_play<F: Fn(PathBuf) + 'static>(&self, f: F) {
        *self.on_play.borrow_mut() = Some(Box::new(f));
    }

    /// Install the callback fired when the user activates a row.
    pub fn connect_select<F: Fn(PathBuf) + 'static>(&self, f: F) {
        *self.on_select.borrow_mut() = Some(Box::new(f));
    }

    /// Install the callback fired when the user clicks the edit button on a row.
    pub fn connect_edit<F: Fn(PathBuf) + 'static>(&self, f: F) {
        *self.on_edit.borrow_mut() = Some(Box::new(f));
    }

    /// Install the callback fired after a drag-and-drop reorder.
    ///
    /// The argument is the new row order (a permutation of all current ids).
    pub fn connect_reorder<F: Fn(Vec<u64>) + 'static>(&self, f: F) {
        *self.on_reorder.borrow_mut() = Some(Box::new(f));
    }

    /// Remove every row and re-show the empty-state status page.
    pub fn clear(&self) {
        self.list_box.clear();
        self.rows.borrow_mut().clear();
        self.row_order.borrow_mut().clear();
        self.list_box.set_visible(false);
        self.status_page.widget().set_visible(true);
    }

    /// Remove the row with `entry_id` and re-show the empty state if the queue is now empty.
    pub fn remove_entry(&self, entry_id: u64) {
        let is_empty_after = {
            let mut rows = self.rows.borrow_mut();
            if let Some(qr) = rows.remove(&entry_id) {
                self.list_box.remove(&qr.row);
            }
            rows.is_empty()
        };
        self.row_order.borrow_mut().retain(|&id| id != entry_id);
        self.refresh_row_stripes();
        if is_empty_after {
            self.list_box.set_visible(false);
            self.status_page.widget().set_visible(true);
        }
    }

    /// Shortcut for [`set_playing_state`](Self::set_playing_state) with `is_playing = true`.
    pub fn set_playing_entry(&self, playing: Option<u64>) {
        self.set_playing_state(playing, true);
    }

    /// Mark `playing` as the now-playing row and switch its play button icon.
    pub fn set_playing_state(&self, playing: Option<u64>, is_playing: bool) {
        let rows = self.rows.borrow();
        for (id, qr) in rows.iter() {
            let active = Some(*id) == playing;
            qr.play_button.set_icon_name(if active && is_playing {
                "media-playback-pause-symbolic"
            } else {
                "media-playback-start-symbolic"
            });
            if active {
                qr.row.add_css_class("queue-now-playing");
                qr.meta_label.remove_css_class("dim-label");
            } else {
                qr.row.remove_css_class("queue-now-playing");
                qr.meta_label.add_css_class("dim-label");
            }
        }
    }

    fn refresh_row_stripes(&self) {
        let order = self.row_order.borrow();
        let rows = self.rows.borrow();
        for (index, entry_id) in order.iter().enumerate() {
            if let Some(qr) = rows.get(entry_id) {
                qr.row.remove_css_class("converter-queue-row-even");
                qr.row.remove_css_class("converter-queue-row-odd");
                if index % 2 == 0 {
                    qr.row.add_css_class("converter-queue-row-even");
                } else {
                    qr.row.add_css_class("converter-queue-row-odd");
                }
            }
        }
    }
}

fn set_named_cursor(widget: &impl IsA<gtk::Widget>, name: &str) {
    if let Some(cursor) = gdk::Cursor::from_name(name, None) {
        widget.set_cursor(Some(&cursor));
    }
}
