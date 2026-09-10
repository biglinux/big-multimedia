//! Relm4 wrapper around [`QueuePage`].
//!
//! Translates the queue page's per-row widget callbacks into typed
//! [`BigConverterQueueOutput`] messages and applies [`BigConverterQueueInput`]
//! to the wrapped page. Manual `impl Component` idiom (no `view!` macro).
//!
//! The component speaks in the cross-app [`BigConversionQueueEntry`] /
//! [`BigConversionQueueStatus`] types; callers translate their own runtime
//! queue types into these before sending input.

use std::path::PathBuf;

use relm4::{Component, ComponentParts, ComponentSender};

use super::queue::{
    BigConversionQueueEntry, BigConversionQueueMode, BigConversionQueueStatus, QueuePage,
};

/// Messages accepted by [`BigConverterQueue`].
#[derive(Debug, Clone)]
pub enum BigConverterQueueInput {
    /// Append a row at the bottom of the queue.
    AddEntry(BigConversionQueueEntry),
    /// Replace a row's status badge.
    UpdateStatus(u64, BigConversionQueueStatus),
    /// Hide every row's status icon.
    ResetStatusIcons,
    /// Remove every row and show the empty state.
    Clear,
    /// Remove a single row.
    RemoveEntry(u64),
    /// Pin the now-playing row (`None` clears the highlight).
    SetPlayingEntry(Option<u64>),
    /// Pin the now-playing row and toggle its play/pause icon.
    SetPlayingState(Option<u64>, bool),
}

/// User actions emitted by [`BigConverterQueue`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BigConverterQueueOutput {
    /// Remove button clicked on a row.
    Remove(u64),
    /// "Open containing folder" picked for a file.
    OpenFolder(PathBuf),
    /// "More information…" picked for a row.
    Metadata(u64),
    /// "Delete from disk…" picked for a row.
    DeleteDisk(u64),
    /// Play button clicked on a row.
    Play(PathBuf),
    /// A row was activated (selected without entering the editor).
    Select(PathBuf),
    /// Edit button clicked on a row (video mode).
    Edit(PathBuf),
    /// Rows were reordered by drag-and-drop; carries the new id order.
    Reorder(Vec<u64>),
}

/// Relm4 wrapper around [`QueuePage`].
pub struct BigConverterQueue {
    page: QueuePage,
}

impl Component for BigConverterQueue {
    type Init = BigConversionQueueMode;
    type Input = BigConverterQueueInput;
    type Output = BigConverterQueueOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        mode: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let page = QueuePage::build_into(root.clone(), mode);

        let s = sender.clone();
        page.connect_remove_clicked(move |id| {
            let _ = s.output(BigConverterQueueOutput::Remove(id));
        });
        let s = sender.clone();
        page.connect_open_folder(move |path| {
            let _ = s.output(BigConverterQueueOutput::OpenFolder(path));
        });
        let s = sender.clone();
        page.connect_metadata(move |id| {
            let _ = s.output(BigConverterQueueOutput::Metadata(id));
        });
        let s = sender.clone();
        page.connect_delete_disk(move |id| {
            let _ = s.output(BigConverterQueueOutput::DeleteDisk(id));
        });
        let s = sender.clone();
        page.connect_play(move |path| {
            let _ = s.output(BigConverterQueueOutput::Play(path));
        });
        let s = sender.clone();
        page.connect_select(move |path| {
            let _ = s.output(BigConverterQueueOutput::Select(path));
        });
        let s = sender.clone();
        page.connect_edit(move |path| {
            let _ = s.output(BigConverterQueueOutput::Edit(path));
        });
        let s = sender;
        page.connect_reorder(move |order| {
            let _ = s.output(BigConverterQueueOutput::Reorder(order));
        });

        ComponentParts {
            model: Self { page },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            BigConverterQueueInput::AddEntry(entry) => self.page.add_entry(&entry),
            BigConverterQueueInput::UpdateStatus(id, status) => {
                self.page.update_entry_status(id, &status);
            }
            BigConverterQueueInput::ResetStatusIcons => self.page.reset_all_status_icons(),
            BigConverterQueueInput::Clear => self.page.clear(),
            BigConverterQueueInput::RemoveEntry(id) => self.page.remove_entry(id),
            BigConverterQueueInput::SetPlayingEntry(playing) => {
                self.page.set_playing_entry(playing)
            }
            BigConverterQueueInput::SetPlayingState(playing, is_playing) => {
                self.page.set_playing_state(playing, is_playing);
            }
        }
    }
}
