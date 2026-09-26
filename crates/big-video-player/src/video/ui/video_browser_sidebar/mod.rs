//! App-local Relm4 sidebar (push-model) with two modes, switched by a segmented
//! control:
//!   * **Files** — a folder browser (breadcrumb + sub-folders, list/grid with
//!     ffmpeg thumbnails, fuzzy search, kind chips). Activating a video plays it
//!     and queues the folder; the context menu also offers "add to playlist".
//!   * **Playlist** — the current play queue (mirrors the shell's mpv playlist),
//!     a flat list/grid with the playing entry highlighted; remove / clear /
//!     save / load act here.
//!
//! The two lists are distinct: `entries` (Files browse listing) and `queue`
//! (the playlist). The shell owns the real mpv playlist; it pushes the queue via
//! `SetPlaylist` and the playing path via `SetCurrentFile`, and the sidebar
//! emits `PlayFromList` / `Enqueue` / `RemoveEntry` / `Clear`.
//!
//! State lives in an `Rc<BrowserState>` shared with the builder free-functions
//! (`chrome`, `views`) and signal closures; the UI is cleared and rebuilt
//! imperatively rather than via a
//! `view!` tree, because the layout swaps wholesale between list/grid and
//! empty/populated states. The current entry's highlight refreshes in O(1) via
//! `store.splice()` on the old + new positions only.
//!
//! Manual `impl Component` idiom (relm4's `factory`/macro features are off in
//! this workspace).

mod chrome;
mod disk_search;
mod fileops;
mod scrub;
mod views;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::layout::widget_container::clear_box_children;
use gtk::{gio, glib};
use relm4::{Component, ComponentParts, ComponentSender};

use crate::i18n::{i18n, ni18n};
use crate::settings::{Settings, keys};

/// Search-filter debounce.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// Which entry kinds the chip row keeps visible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FilterKind {
    /// Folders and videos (default).
    #[default]
    All,
    /// Folder-navigation rows only.
    Folders,
    /// Video rows only.
    Videos,
}

/// The two top-level sidebar modes, switched by the segmented control: browse
/// the disk (Files) vs. the current play queue (Playlist).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarMode {
    /// Folder browser — navigate the disk and start playback.
    #[default]
    Files,
    /// The current play queue (what the shell loaded into mpv).
    Playlist,
}

/// Display state the shell pushes into the sidebar.
#[derive(Debug)]
pub enum VideoBrowserSidebarInput {
    /// Replace the listing with a flat set of paths opened externally (open
    /// dialog / drag-drop / IPTV / m3u). `folder` is the directory to present as
    /// the browse context (breadcrumb + sub-folders), or `None` for an ad-hoc
    /// set (multiple files / remote stream) shown as a plain list.
    /// Where the playing video is, in seconds (for the playlist's end time).
    SetPosition(f64),
    /// The play queue changed under the tab being shown (an add, a "play
    /// next"): update it without switching tabs.
    SetQueue(Vec<String>),
    SetPlaylist {
        paths: Vec<String>,
        folder: Option<String>,
    },
    /// The currently-playing file changed; re-light the matching row.
    SetCurrentFile(Option<String>),
    /// Switch the active mode (driven by the two main-window header buttons).
    SetMode(SidebarMode),
    /// Re-scan the browsed folder (after a file op moved/removed entries).
    RefreshFolder,
}

/// User intents emitted by the sidebar for the shell to act on.
#[derive(Debug)]
pub enum VideoBrowserSidebarOutput {
    /// Activate a video: make `paths` the playlist and start at `start`.
    PlayFromList { paths: Vec<String>, start: usize },
    /// Append these paths to the mpv playlist without disrupting playback
    /// (Files "add to playlist" / the header add button).
    Enqueue(Vec<String>),
    /// Put `path` right after the playing entry (adding it if absent), and
    /// start it when `now`: "Play next", or a video found elsewhere.
    PlayNext { path: String, now: bool },
    /// The playlist row for `path` was moved to position `to` (dragged, or
    /// Alt+Up/Down): the queue already shows it there.
    MoveEntry { path: String, to: usize },
    /// Remove this path from the mpv playlist (if present); the sidebar already
    /// dropped it from its own queue.
    RemoveEntry(String),
    /// The playlist was cleared.
    Clear,
    /// Open these files in the video converter GUI.
    SendToConverter(Vec<String>),
    /// Move these files to the trash; the shell offers an Undo.
    TrashFiles(Vec<String>),
    /// Tell the person something (a file-op error).
    Notify(String),
    /// The Repeat mode changed (`no` / `file` / `playlist`); already persisted.
    SetLoopMode(String),
    /// Shuffle was toggled; already persisted.
    SetShuffle(bool),
}

/// Shared browser state — held in an `Rc` so the builder free-functions and
/// signal closures can read settings, mutate the listing, and trigger rebuilds.
pub(crate) struct BrowserState {
    /// Sidebar container (cleared + rebuilt on every `rebuild`). Weak: this
    /// `Rc` is captured by closures on widgets INSIDE the container — a strong
    /// field closes a container→widget→closure→Rc→container island that pins
    /// the whole panel after window close.
    container: glib::WeakRef<gtk::Box>,
    /// Player settings (view mode, sort, toggles — read and written here).
    settings: Settings,
    /// Channel back to the shell (play / clear intents).
    sender: ComponentSender<VideoBrowserSidebar>,
    /// Active mode: Files (folder browser) vs Playlist (the play queue).
    mode: Cell<SidebarMode>,
    /// Files mode: the videos in the browsed folder.
    entries: RefCell<Vec<String>>,
    /// Playlist mode: the current play queue (mirrors the shell's mpv playlist).
    queue: RefCell<Vec<String>>,
    /// Browsed folder (`Some` enables breadcrumb + sub-folder rows).
    folder: RefCell<Option<String>>,
    /// Sub-folders of `folder`, enumerated off the main thread alongside the
    /// file scan (so `build_store` never does a blocking `read_dir` on the GTK
    /// loop). Valid for the current `folder`; refreshed on every navigation.
    subfolders: RefCell<Vec<String>>,
    /// Path of the playing file, for the current-row highlight.
    current_path: RefCell<Option<String>>,
    /// Index of `current_path` within the active list (`-1` = none).
    pos: Cell<i64>,
    /// Previous highlighted index (for the O(1) splice refresh).
    prev_pos: Cell<i64>,
    /// Backing model of the active view (`None` between rebuilds).
    store: RefCell<Option<gio::ListStore>>,
    /// Number of leading folder rows (offset from `pos` to the store index).
    folder_entry_count: Cell<u32>,
    /// Wrapping counter bumped on each folder navigation to cancel in-flight scans.
    scan_generation: Cell<u64>,
    /// Whether the last recursive scan hit `SCAN_LIMIT`.
    truncated: Cell<bool>,
    /// Total files found by the last scan (shown when truncated).
    total_found: Cell<usize>,
    /// Files mode: whether multi-select (triage) mode is on.
    select_mode: Cell<bool>,
    /// Files mode: the set of selected video paths (for batch actions).
    selection: RefCell<HashSet<String>>,
    /// Live count label in the batch action bar (updated on each toggle).
    select_count_label: RefCell<Option<gtk::Label>>,
    /// Active kind chip.
    filter_kind: Cell<FilterKind>,
    /// Lowercased search query (read by the custom filter).
    search_text: Rc<RefCell<String>>,
    /// Generation token for the search debounce.
    search_gen: Cell<u64>,
    /// Active view filter, so chips + search can re-run it.
    filter: RefCell<Option<gtk::CustomFilter>>,
    /// Files mode: the "on this computer" results under the folder's list.
    disk_section: RefCell<Option<glib::WeakRef<gtk::Box>>>,
    /// Playlist mode: the count, total length and end time over the list.
    summary: RefCell<Option<glib::WeakRef<gtk::Label>>>,
    /// Length of each queued video once probed, for the summary.
    lengths: Rc<RefCell<std::collections::HashMap<String, f64>>>,
    /// Where the playing video is, in seconds.
    position: Cell<f64>,
}

impl BrowserState {
    /// Upgrade the weak container ref; `None` only mid-teardown.
    fn container(&self) -> Option<gtk::Box> {
        self.container.upgrade()
    }

    /// Toplevel window (FilePicker parent); `None` before the sidebar is mapped.
    fn window(&self) -> Option<gtk::Window> {
        self.container()?.root().and_downcast::<gtk::Window>()
    }

    /// The list shown by the active mode (Files folder listing vs play queue).
    fn active_items(&self) -> Vec<String> {
        match self.mode.get() {
            SidebarMode::Files => self.entries.borrow().clone(),
            SidebarMode::Playlist => self.queue.borrow().clone(),
        }
    }

    /// Recompute `pos` from `current_path` against the active list.
    fn recompute_pos(&self) {
        let items = self.active_items();
        let pos = self
            .current_path
            .borrow()
            .as_ref()
            .and_then(|cur| items.iter().position(|p| p == cur))
            .map_or(-1, |i| i as i64);
        self.pos.set(pos);
    }

    /// The line over the playlist: how many, how long, and when the last one
    /// ends. Lengths not yet known are probed off the main loop (the probe is
    /// cached, so each file is read once).
    fn build_summary(self: &Rc<Self>, items: &[String]) {
        let label = gtk::Label::builder()
            .xalign(0.0)
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(4)
            .css_classes(["dim-label", "caption"])
            .build();
        self.summary.replace(Some(label.downgrade()));
        if let Some(container) = self.container() {
            container.append(&label);
        }
        self.refresh_summary();
        let missing: Vec<String> = {
            let lengths = self.lengths.borrow();
            items
                .iter()
                .filter(|path| !lengths.contains_key(*path))
                .cloned()
                .collect()
        };
        if missing.is_empty() {
            return;
        }
        let state = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let found = gio::spawn_blocking(move || {
                missing
                    .into_iter()
                    .map(|path| {
                        let length = crate::media::playlist::probe_metadata_cached(&path)
                            .and_then(|meta| meta.duration_secs)
                            .unwrap_or(0.0);
                        (path, length)
                    })
                    .collect::<Vec<_>>()
            })
            .await
            .unwrap_or_default();
            if let Some(state) = state.upgrade() {
                state.lengths.borrow_mut().extend(found);
                state.refresh_summary();
            }
        });
    }

    /// Redraw the summary from the lengths known and the position.
    fn refresh_summary(&self) {
        let Some(label) = self
            .summary
            .borrow()
            .as_ref()
            .and_then(glib::WeakRef::upgrade)
        else {
            return;
        };
        let queue = self.queue.borrow();
        let count = ni18n("{} video", "{} videos", queue.len() as u64)
            .replace("{}", &queue.len().to_string());
        let lengths = self.lengths.borrow();
        let known: Option<Vec<f64>> = queue
            .iter()
            .map(|path| lengths.get(path).copied())
            .collect();
        // A stream (length 0) has no end to name.
        let Some(known) = known.filter(|known| known.iter().all(|length| *length > 0.0)) else {
            label.set_label(&count);
            return;
        };
        let total: f64 = known.iter().sum();
        let mut text = format!(
            "{count} · {}",
            big_media_components::time::duration_hms(total)
        );
        if let Ok(current) = usize::try_from(self.pos.get())
            && current < known.len()
        {
            let left = known[current] - self.position.get().min(known[current])
                + known[current + 1..].iter().sum::<f64>();
            if let Some(end) = glib::DateTime::now_local()
                .ok()
                .and_then(|now| now.add_seconds(left).ok())
                .and_then(|end| end.format("%H:%M").ok())
            {
                text.push_str(" · ");
                text.push_str(&i18n("ends at {}").replace("{}", &end));
            }
        }
        label.set_label(&text);
    }

    /// Move the queue entry `from` to `to`, in place: the rows between are
    /// renumbered without rebuilding, so the list keeps its scroll and focus.
    fn move_in_queue(&self, from: usize, to: usize) {
        let path = {
            let mut queue = self.queue.borrow_mut();
            if from >= queue.len() || to >= queue.len() || from == to {
                return;
            }
            let path = queue.remove(from);
            queue.insert(to, path.clone());
            path
        };
        self.recompute_pos();
        self.prev_pos.set(self.pos.get());
        if let Some(store) = self.store.borrow().as_ref() {
            let queue = self.queue.borrow();
            let (low, high) = (from.min(to), from.max(to));
            let rows: Vec<glib::BoxedAnyObject> = (low..=high)
                .map(|index| {
                    glib::BoxedAnyObject::new(crate::media::playlist::PlaylistEntry {
                        display_name: crate::media::playlist::display_name(&queue[index]),
                        path: queue[index].clone(),
                        kind: crate::media::playlist::EntryKind::Video,
                        index,
                    })
                })
                .collect();
            let offset = self.folder_entry_count.get();
            store.splice(offset + low as u32, (high - low + 1) as u32, &rows);
        }
        self.sender
            .output(VideoBrowserSidebarOutput::MoveEntry { path, to })
            .ok();
    }

    /// Re-run the active view filter (after a chip or search change).
    fn refilter(&self) {
        if let Some(filter) = self.filter.borrow().as_ref() {
            filter.changed(gtk::FilterChange::Different);
        }
    }

    /// Scan `folder` (flat or recursive per settings) and rebuild, cancelling
    /// any scan still in flight. Browsing never touches playback — only a video
    /// activation does.
    fn navigate_to_folder(self: &Rc<Self>, folder: &str) {
        let folder_owned = folder.to_string();
        let recursive = self.settings.get_bool(keys::PLAYLIST_FOLDER_RECURSIVE);

        self.folder.replace(Some(folder_owned.clone()));
        self.settings
            .put_string(keys::PLAYLIST_LAST_FOLDER, &folder_owned);

        let generation = self.scan_generation.get().wrapping_add(1);
        self.scan_generation.set(generation);

        // Spinner while the blocking scan runs.
        self.show_scanning_spinner();

        let state = self.clone();
        glib::spawn_future_local(async move {
            let folder_clone = folder_owned.clone();
            // Sub-folder rows (flat mode only) are enumerated in the same
            // off-thread pass as the file scan, so the view builder reads them
            // from the cache instead of doing a blocking `read_dir` on the loop.
            let (scan, subfolders) = gio::spawn_blocking(move || {
                use crate::media::playlist;
                let path = std::path::Path::new(&folder_clone);
                let scan = if recursive {
                    playlist::scan_folder_recursive(path)
                } else {
                    let files = playlist::scan_folder_flat(path);
                    playlist::ScanResult {
                        total_found: files.len(),
                        truncated: false,
                        files,
                    }
                };
                let subfolders = if recursive {
                    Vec::new()
                } else {
                    playlist::list_subfolders(path)
                };
                (scan, subfolders)
            })
            .await
            .unwrap_or_else(|_| {
                (
                    crate::media::playlist::ScanResult {
                        files: Vec::new(),
                        truncated: false,
                        total_found: 0,
                    },
                    Vec::new(),
                )
            });

            // Folder switched again while scanning → drop this stale result.
            if state.scan_generation.get() != generation {
                return;
            }

            state.truncated.set(scan.truncated);
            state.total_found.set(scan.total_found);
            state.entries.replace(scan.files);
            state.subfolders.replace(subfolders);
            state.recompute_pos();
            state.rebuild();
        });
    }

    /// Enumerate the current `folder`'s sub-folders off the main thread, then
    /// re-run the listing so the browse context shows them. Used by the
    /// single-file-open path (which sets `folder` without a full folder scan).
    /// No-op in recursive mode (flat tree → no sub-folder rows) or with no
    /// folder. Guarded by `scan_generation`: a navigation that supersedes this
    /// drops the stale result.
    fn refresh_subfolders(self: &Rc<Self>) {
        if self.settings.get_bool(keys::PLAYLIST_FOLDER_RECURSIVE) {
            return;
        }
        let Some(folder) = self.folder.borrow().clone() else {
            return;
        };
        let generation = self.scan_generation.get();
        let state = self.clone();
        glib::spawn_future_local(async move {
            let scanned_folder = folder.clone();
            let subfolders = gio::spawn_blocking(move || {
                crate::media::playlist::list_subfolders(std::path::Path::new(&folder))
            })
            .await
            .unwrap_or_default();
            // Folder navigated/changed while enumerating → drop this stale
            // result (generation bumps on navigation; identity guards a second
            // single-file open that swapped the folder at the same generation).
            if state.scan_generation.get() != generation
                || state.folder.borrow().as_deref() != Some(scanned_folder.as_str())
            {
                return;
            }
            if subfolders.is_empty() {
                return;
            }
            state.subfolders.replace(subfolders);
            state.rebuild();
        });
    }

    /// Replace the container with a centered scanning spinner (header kept).
    fn show_scanning_spinner(self: &Rc<Self>) {
        let Some(container) = self.container() else {
            return;
        };
        clear_sidebar_container(&container);
        chrome::build_header(self);

        let spinner_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .valign(gtk::Align::Center)
            .vexpand(true)
            .build();
        spinner_box.append(
            &adw::Spinner::builder()
                .width_request(32)
                .height_request(32)
                .halign(gtk::Align::Center)
                .build(),
        );
        spinner_box.append(
            &gtk::Label::builder()
                .label(crate::i18n::i18n("Scanning…"))
                .css_classes(["dim-label"])
                .halign(gtk::Align::Center)
                .build(),
        );
        container.append(&spinner_box);
    }

    /// Clear and rebuild the sidebar: shared chrome (header + mode switcher),
    /// then the mode-specific body.
    fn rebuild(self: &Rc<Self>) {
        let Some(container) = self.container() else {
            return;
        };
        self.store.replace(None);
        self.filter.replace(None);
        self.recompute_pos();
        clear_sidebar_container(&container);
        // The search entry is built anew, empty: so is what it filters by, or
        // the list would stay filtered under an empty box.
        self.search_text.borrow_mut().clear();
        self.search_gen.set(self.search_gen.get().wrapping_add(1));

        chrome::build_header(self);

        match self.mode.get() {
            SidebarMode::Files => self.rebuild_files(),
            SidebarMode::Playlist => self.rebuild_playlist(),
        }
    }

    /// Files mode: breadcrumb + search + kind chips + the folder browser.
    fn rebuild_files(self: &Rc<Self>) {
        let is_grid = self.settings.get_string(keys::PLAYLIST_VIEW) == "grid";
        let recursive_on = self.settings.get_bool(keys::PLAYLIST_FOLDER_RECURSIVE);
        let folder_opt = self.folder.borrow().clone();
        let items = self.entries.borrow().clone();

        if let Some(folder) = folder_opt.as_ref() {
            chrome::build_breadcrumb(self, folder);
        }
        let search_entry = chrome::build_search(self);
        chrome::build_chips(self);

        // Empty *and* not browsing a folder → the welcome status page.
        if items.is_empty() && folder_opt.is_none() {
            chrome::build_files_empty_state(self);
            if let Some(container) = self.container() {
                container.append(&disk_search::build_section(self));
            }
            return;
        }

        chrome::build_count_summary(self, items.len());

        let scroll = if is_grid {
            views::build_grid_view(self, &items, recursive_on, folder_opt.as_ref())
        } else {
            views::build_list_view(self, &items, recursive_on, folder_opt.as_ref())
        };
        self.prev_pos.set(self.pos.get());
        if let Some(container) = self.container() {
            container.append(&scroll);
            container.append(&disk_search::build_section(self));
        }

        // Triage batch-action bar pinned at the bottom while selecting.
        if self.select_mode.get() {
            fileops::build_batch_bar(self);
        }

        chrome::add_key_controller(self, &search_entry);
    }

    /// Playlist mode: search + the compact ordered play queue (numbered rows,
    /// thumbnail, duration, remove button; playing entry marked). A distinct
    /// queue view — not the Files grid/list.
    fn rebuild_playlist(self: &Rc<Self>) {
        let items = self.queue.borrow().clone();

        let search_entry = chrome::build_search(self);

        if items.is_empty() {
            chrome::build_playlist_empty_state(self);
            return;
        }

        self.build_summary(&items);

        let scroll = views::build_queue_view(self, &items);
        self.prev_pos.set(self.pos.get());
        if let Some(container) = self.container() {
            container.append(&scroll);
        }

        chrome::add_key_controller(self, &search_entry);
    }

    /// O(1) highlight refresh: replace the old + new positions so the factory
    /// rebinds just those rows (drop / add the accent). With a copy: the same
    /// object put back is taken for no change, and the row kept its old look.
    fn update_highlight(&self) {
        let store_ref = self.store.borrow();
        let Some(store) = store_ref.as_ref() else {
            return;
        };
        let old_pos = self.prev_pos.get();
        let new_pos = self.pos.get();
        self.prev_pos.set(new_pos);
        if old_pos == new_pos {
            return;
        }
        let folder_count = self.folder_entry_count.get();
        for p in [old_pos, new_pos] {
            if p >= 0 {
                let idx = folder_count + p as u32;
                if let Some(item) = store.item(idx).and_downcast::<glib::BoxedAnyObject>() {
                    let entry = item
                        .borrow::<crate::media::playlist::PlaylistEntry>()
                        .clone();
                    store.splice(idx, 1, &[glib::BoxedAnyObject::new(entry)]);
                }
            }
        }
    }

    /// Route a row activation: folders navigate, videos play the whole listing.
    fn activate(self: &Rc<Self>, model: &gtk::SelectionModel, position: u32) {
        use crate::media::playlist::EntryKind;
        let Some(item) = model.item(position) else {
            return;
        };
        let Ok(obj) = item.downcast::<glib::BoxedAnyObject>() else {
            return;
        };
        let entry: std::cell::Ref<crate::media::playlist::PlaylistEntry> = obj.borrow();
        match entry.kind {
            EntryKind::ParentFolder | EntryKind::Folder => {
                let path = entry.path.clone();
                drop(entry);
                self.navigate_to_folder(&path);
            }
            EntryKind::Video => {
                let idx = entry.index;
                let path = entry.path.clone();
                drop(entry);
                // In triage (select) mode a click toggles selection, not play.
                if self.select_mode.get() {
                    self.toggle_selection(&path);
                    return;
                }
                let paths = self.active_items();
                self.current_path.replace(Some(path));
                self.recompute_pos();
                self.update_highlight();
                self.sender
                    .output(VideoBrowserSidebarOutput::PlayFromList { paths, start: idx })
                    .ok();
            }
        }
    }

    /// Switch the active mode and rebuild (no-op if already in it).
    fn switch_mode(self: &Rc<Self>, mode: SidebarMode) {
        if self.mode.get() != mode {
            self.mode.set(mode);
            self.rebuild();
        }
    }

    /// Enter/leave multi-select (triage) mode; leaving clears the selection.
    fn set_select_mode(self: &Rc<Self>, on: bool) {
        if !on {
            self.selection.borrow_mut().clear();
        }
        self.select_mode.set(on);
        self.rebuild();
    }

    /// Toggle a path's membership in the selection and refresh its row + the
    /// batch-bar count, without a full rebuild.
    fn toggle_selection(self: &Rc<Self>, path: &str) {
        let count = {
            let mut sel = self.selection.borrow_mut();
            if !sel.remove(path) {
                sel.insert(path.to_string());
            }
            sel.len()
        };
        if let Some(label) = self.select_count_label.borrow().as_ref() {
            label.set_label(&fileops::selection_count_text(count));
        }
        self.refresh_path_row(path);
    }

    /// Whether `path` is in the current selection.
    fn is_selected(&self, path: &str) -> bool {
        self.selection.borrow().contains(path)
    }

    /// Splice the store row for `path` so the factory rebinds it (e.g. to repaint
    /// its selection check). Files mode only (uses the folder-row offset). A
    /// *fresh* boxed entry is inserted so GTK always re-runs `bind` (re-splicing
    /// the identical object can be coalesced into a no-op).
    fn refresh_path_row(&self, path: &str) {
        use crate::media::playlist::{self, EntryKind, PlaylistEntry};
        let store_ref = self.store.borrow();
        let Some(store) = store_ref.as_ref() else {
            return;
        };
        let Some(idx) = self.entries.borrow().iter().position(|p| p == path) else {
            return;
        };
        let store_idx = self.folder_entry_count.get() + idx as u32;
        if store_idx < store.n_items() {
            let fresh = glib::BoxedAnyObject::new(PlaylistEntry {
                display_name: playlist::display_name(path),
                path: path.to_string(),
                kind: EntryKind::Video,
                index: idx,
            });
            store.splice(store_idx, 1, &[fresh]);
        }
    }
}

/// Remove every child from the sidebar root before a mode rebuild.
pub(crate) fn clear_sidebar_container(container: &gtk::Box) {
    clear_box_children(container);
}

/// Relm4 folder-browser sidebar component.
pub struct VideoBrowserSidebar {
    state: Rc<BrowserState>,
}

impl Component for VideoBrowserSidebar {
    type Init = Settings;
    type Input = VideoBrowserSidebarInput;
    type Output = VideoBrowserSidebarOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        settings: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let state = Rc::new(BrowserState {
            container: root.downgrade(),
            settings,
            sender,
            mode: Cell::new(SidebarMode::Files),
            entries: RefCell::new(Vec::new()),
            queue: RefCell::new(Vec::new()),
            folder: RefCell::new(None),
            subfolders: RefCell::new(Vec::new()),
            current_path: RefCell::new(None),
            pos: Cell::new(-1),
            prev_pos: Cell::new(-1),
            store: RefCell::new(None),
            folder_entry_count: Cell::new(0),
            scan_generation: Cell::new(0),
            truncated: Cell::new(false),
            total_found: Cell::new(0),
            select_mode: Cell::new(false),
            selection: RefCell::new(HashSet::new()),
            select_count_label: RefCell::new(None),
            filter_kind: Cell::new(FilterKind::All),
            search_text: Rc::new(RefCell::new(String::new())),
            search_gen: Cell::new(0),
            disk_section: RefCell::new(None),
            summary: RefCell::new(None),
            lengths: Rc::default(),
            position: Cell::new(0.0),
            filter: RefCell::new(None),
        });
        state.rebuild();
        ComponentParts {
            model: Self { state },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            VideoBrowserSidebarInput::SetPlaylist { paths, folder } => {
                // The shell loaded files → they become the play queue. When a
                // browse folder is given (single-file open), also show it in
                // Files mode; otherwise reveal the queue in Playlist mode.
                self.state.queue.replace(paths.clone());
                if let Some(folder) = folder {
                    self.state.folder.replace(Some(folder));
                    self.state.entries.replace(paths);
                    self.state.subfolders.replace(Vec::new());
                    self.state.truncated.set(false);
                    self.state.mode.set(SidebarMode::Files);
                    self.state.recompute_pos();
                    self.state.rebuild();
                    // Enumerate sub-folders off-thread, then re-run the listing
                    // (the synchronous rebuild above shows files only; this fills
                    // the browse context without blocking the loop on `read_dir`).
                    self.state.refresh_subfolders();
                } else {
                    self.state.mode.set(SidebarMode::Playlist);
                    self.state.recompute_pos();
                    self.state.rebuild();
                }
            }
            VideoBrowserSidebarInput::SetPosition(seconds) => {
                self.state.position.set(seconds);
                self.state.refresh_summary();
            }
            VideoBrowserSidebarInput::SetQueue(paths) => {
                self.state.queue.replace(paths);
                self.state.recompute_pos();
                if self.state.mode.get() == SidebarMode::Playlist {
                    self.state.rebuild();
                }
            }
            VideoBrowserSidebarInput::SetCurrentFile(path) => {
                self.state.current_path.replace(path);
                self.state.recompute_pos();
                self.state.update_highlight();
            }
            VideoBrowserSidebarInput::SetMode(mode) => self.state.switch_mode(mode),
            VideoBrowserSidebarInput::RefreshFolder => {
                let folder = self.state.folder.borrow().clone();
                if let Some(folder) = folder {
                    self.state.navigate_to_folder(&folder);
                } else {
                    self.state.rebuild();
                }
            }
        }
    }
}

/// Schedule a debounced search-filter update: bump the generation token, then
/// commit the query + refilter only if no newer keystroke superseded it.
pub(crate) fn schedule_search(state: &Rc<BrowserState>, query: String) {
    let generation = state.search_gen.get().wrapping_add(1);
    state.search_gen.set(generation);
    let state = state.clone();
    glib::timeout_add_local_once(SEARCH_DEBOUNCE, move || {
        if state.search_gen.get() != generation {
            return;
        }
        state.search_text.borrow_mut().clone_from(&query);
        state.refilter();
        if state.mode.get() == SidebarMode::Files {
            disk_search::refresh(&state, query);
        }
    });
}
