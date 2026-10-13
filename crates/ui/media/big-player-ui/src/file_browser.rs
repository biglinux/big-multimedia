//! Relm4 file-system browser tab: flat directory listing with a `..` parent,
//! instant filter, and a right-click "add to library" menu.
//!
//! Generic: the playable-file predicate, settings store, start directory and
//! menu labels come from a `FileBrowserConfig` at launch, so it serves any
//! media app (audio, video, …).
//!
//! Push-model: navigation, filtering and last-dir persistence are internal;
//! activating a file emits `FileBrowserOutput::PlayFile` and the library menu
//! emits `FileBrowserOutput::ScanFolder` for the shell to act on (queue/mpv
//! and the library scan live in the shell). Manual `impl Component` idiom.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::actions::install_action;
use gtk::{gio, glib};
use relm4::{Component, ComponentParts, ComponentSender};

use crate::browser_node::{BrowserNode, BrowserNodeKind};
use big_media_components::player_chrome::spawn_blocking_result;
use big_media_data::i18n::i18n;
use big_media_data::settings::{Settings, keys};

/// Launch-time configuration making the browser media-kind agnostic.
pub struct FileBrowserConfig {
    /// Predicate deciding which files are listed and playable
    /// (e.g. `is_audio_file` / `is_video_file`).
    pub is_media_file: fn(&str) -> bool,
    /// Settings store persisting the last-visited directory.
    pub settings: Settings,
    /// Directory shown when there is no saved last directory
    /// (e.g. the XDG Music or Videos dir).
    pub fallback_dir: PathBuf,
    /// Context-menu label: add the *current* folder to the library.
    pub add_current_label: String,
    /// Context-menu label: add the *selected* folder to the library.
    pub add_selected_label: String,
    /// Placeholder text for the filter entry.
    pub filter_placeholder: String,
}

/// Intents emitted by the file browser.
#[derive(Debug)]
pub enum FileBrowserOutput {
    /// Activated a media file: queue and (if idle) play it.
    PlayFile {
        /// Absolute file path.
        path: String,
        /// Display title (file name).
        title: String,
    },
    /// Add a folder to the library (recursive scan).
    ScanFolder(String),
}

/// Relm4 file-browser component.
///
/// Stateless model: the directory store lives via the widget tree
/// (filter model → selection → list view) and the current-dir / settings
/// handles live in the navigation closures, so nothing needs holding here.
pub struct FileBrowser;

impl Component for FileBrowser {
    type Init = FileBrowserConfig;
    type Input = ();
    type Output = FileBrowserOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        config: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let FileBrowserConfig {
            is_media_file,
            settings,
            fallback_dir,
            add_current_label,
            add_selected_label,
            filter_placeholder,
        } = config;
        let filter_text = Rc::new(RefCell::new(String::new()));
        let current_dir = Rc::new(RefCell::new(PathBuf::new()));
        // Monotonic scan generation: each populate_dir bumps it so a slow scan
        // superseded by a newer navigation (incl. A→B→A back to the same dir)
        // drops its result instead of double-appending to the store.
        let scan_gen = Rc::new(Cell::new(0u64));

        let path_label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::Start)
            .margin_start(4)
            .css_classes(["caption"])
            .build();
        let filter_entry = gtk::SearchEntry::builder()
            .hexpand(true)
            .placeholder_text(&filter_placeholder)
            .build();
        filter_entry.update_property(&[gtk::accessible::Property::Label(&i18n("Filter files"))]);

        let toolbar = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .margin_start(8)
            .margin_end(8)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        toolbar.append(&path_label);
        toolbar.append(&filter_entry);
        root.append(&toolbar);

        let store = gio::ListStore::new::<BrowserNode>();

        let filter = gtk::CustomFilter::new(glib::clone!(
            #[strong] // strong: Rc filter-text data that the filter must keep alive
            filter_text,
            move |obj| {
                let needle = filter_text.borrow().clone();
                if needle.is_empty() {
                    return true;
                }
                let Some(node) = obj.downcast_ref::<BrowserNode>() else {
                    return false;
                };
                // `..` parent entry must stay visible regardless of filter.
                node.label() == ".." || node.label().to_lowercase().contains(&needle)
            }
        ));
        let filter_model = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
        let selection = gtk::SingleSelection::new(Some(filter_model.clone()));
        let list_view = build_list_view(&selection);

        // Restore last directory from settings; fall back to the configured dir.
        let saved = settings.get_string(keys::FILES_LAST_DIR);
        let start_dir = if !saved.is_empty() && Path::new(&saved).is_dir() {
            PathBuf::from(saved)
        } else {
            fallback_dir
        };
        current_dir.replace(start_dir.clone());
        populate_dir(&store, &scan_gen, &start_dir, is_media_file);
        path_label.set_label(&start_dir.to_string_lossy());

        wire_activate(
            &list_view,
            &filter_model,
            &store,
            &current_dir,
            &scan_gen,
            &filter_entry,
            &filter_text,
            &path_label,
            &settings,
            is_media_file,
            &sender,
        );

        let filter_ref = filter.clone();
        filter_entry.connect_search_changed(glib::clone!(
            #[strong] // strong: Rc filter-text data, so no widget⇄closure cycle
            filter_text,
            move |entry| {
                filter_text.replace(entry.text().trim().to_lowercase());
                filter_ref.changed(gtk::FilterChange::Different);
            }
        ));

        install_library_menu(
            &list_view,
            &selection,
            &current_dir,
            &add_current_label,
            &add_selected_label,
            &sender,
        );

        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list_view)
            .build();
        root.append(&scroll);

        ComponentParts {
            model: Self,
            widgets: (),
        }
    }

    fn update(
        &mut self,
        _message: Self::Input,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        // No inbound messages: the browser is self-driven.
    }
}

/// Build the list view with an icon+label row factory.
fn build_list_view(selection: &gtk::SingleSelection) -> gtk::ListView {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_top(2)
            .margin_bottom(2)
            .margin_start(8)
            .margin_end(8)
            .build();
        row.append(&gtk::Image::new());
        row.append(
            &gtk::Label::builder()
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build(),
        );
        list_item.set_child(Some(&row));
    });
    factory.connect_bind(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(node) = list_item.item().and_downcast::<BrowserNode>() else {
            return;
        };
        let Some(row) = list_item.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let Some(icon) = row.first_child().and_downcast::<gtk::Image>() else {
            return;
        };
        let Some(label) = icon.next_sibling().and_downcast::<gtk::Label>() else {
            return;
        };
        icon.set_icon_name(Some(&node.icon_name()));
        label.set_label(&node.label());
    });

    let list_view = gtk::ListView::builder()
        .model(selection)
        .factory(&factory)
        .vexpand(true)
        .build();
    list_view.add_css_class("navigation-sidebar");
    list_view.update_property(&[gtk::accessible::Property::Label(&i18n("File browser"))]);
    list_view
}

/// Wire row activation: folders navigate in-place, files emit `PlayFile`.
#[allow(clippy::too_many_arguments)]
fn wire_activate(
    list_view: &gtk::ListView,
    filter_model: &gtk::FilterListModel,
    store: &gio::ListStore,
    current_dir: &Rc<RefCell<PathBuf>>,
    scan_gen: &Rc<Cell<u64>>,
    filter_entry: &gtk::SearchEntry,
    filter_text: &Rc<RefCell<String>>,
    path_label: &gtk::Label,
    settings: &Settings,
    is_media_file: fn(&str) -> bool,
    sender: &ComponentSender<FileBrowser>,
) {
    let filter_model = filter_model.clone();
    let store = store.clone();
    let current_dir = current_dir.clone();
    let scan_gen = scan_gen.clone();
    let filter_entry = filter_entry.clone();
    let filter_text = filter_text.clone();
    let path_label = path_label.clone();
    let settings = settings.clone();
    let sender = sender.clone();
    list_view.connect_activate(move |_lv, pos| {
        let Some(obj) = filter_model.item(pos) else {
            return;
        };
        let Some(node) = obj.downcast_ref::<BrowserNode>() else {
            return;
        };
        match node.kind() {
            BrowserNodeKind::Folder => {
                let new_dir = PathBuf::from(node.key());
                filter_entry.set_text("");
                filter_text.replace(String::new());
                current_dir.replace(new_dir.clone());
                populate_dir(&store, &scan_gen, &new_dir, is_media_file);
                path_label.set_label(&new_dir.to_string_lossy());
                let _ = settings.put_string(keys::FILES_LAST_DIR, &new_dir.to_string_lossy());
                settings.flush();
            }
            _ => {
                let _ = sender.output(FileBrowserOutput::PlayFile {
                    path: node.key(),
                    title: node.label(),
                });
            }
        }
    });
}

/// Attach the right-click "add to library" context menu.
fn install_library_menu(
    list_view: &gtk::ListView,
    selection: &gtk::SingleSelection,
    current_dir: &Rc<RefCell<PathBuf>>,
    add_current_label: &str,
    add_selected_label: &str,
    sender: &ComponentSender<FileBrowser>,
) {
    let context_menu = gio::Menu::new();
    context_menu.append(
        Some(add_current_label),
        Some("files.add-current-to-library"),
    );
    context_menu.append(
        Some(add_selected_label),
        Some("files.add-selected-to-library"),
    );
    // bigagents: popover-parent-managed: single reused context menu
    // (gesture pops it up per right-click) parented on the list view.
    // GtkListView does not unparent foreign children at dispose (it
    // finalizes "with children left" and the popover leaks), so detach at
    // the list's unrealize, while everything is still alive.
    let popover = gtk::PopoverMenu::from_model(Some(&context_menu));
    popover.set_parent(list_view);
    popover.set_has_arrow(false);
    {
        // `destroy`, not `unrealize`: a Files tab the user never opens never
        // realizes, so unrealize never fires and the popover still leaked.
        // `destroy` fires at the start of dispose with the widget valid.
        let popover = popover.downgrade();
        list_view.connect_destroy(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
                if popover.parent().is_some() {
                    popover.unparent();
                }
            }
        });
    }

    let action_group = gio::SimpleActionGroup::new();
    let dir_ref = current_dir.clone();
    let s = sender.clone();
    install_action(&action_group, "add-current-to-library", move || {
        let _ = s.output(FileBrowserOutput::ScanFolder(
            dir_ref.borrow().to_string_lossy().to_string(),
        ));
    });

    let context_selected_path = Rc::new(RefCell::new(String::new()));
    let path_ref = context_selected_path.clone();
    let s = sender.clone();
    let add_selected = install_action(&action_group, "add-selected-to-library", move || {
        let path = path_ref.borrow().clone();
        if !path.is_empty() {
            let _ = s.output(FileBrowserOutput::ScanFolder(path));
        }
    });
    list_view.insert_action_group("files", Some(&action_group));

    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    let selection = selection.clone();
    gesture.connect_pressed(move |_gest, _n, x, y| {
        let mut on_folder = false;
        if let Some(obj) = selection.item(selection.selected())
            && let Some(node) = obj.downcast_ref::<BrowserNode>()
            && node.kind() == BrowserNodeKind::Folder
            && node.label() != ".."
        {
            on_folder = true;
            context_selected_path.replace(node.key());
        }
        add_selected.set_enabled(on_folder);
        let point = gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1);
        popover.set_pointing_to(Some(&point));
        popover.popup();
    });
    list_view.add_controller(gesture);
}

/// Populate `store` with the media files (per `is_media_file`) and sub-folders
/// of `dir` (folders first, then case-insensitive alphabetical; `.` files
/// skipped).
///
/// The `read_dir` + per-entry `stat` walk runs OFF the main thread (it can hit
/// a network mount or a huge directory and would otherwise freeze the UI on
/// every folder activation); the store is repopulated back on the main thread.
/// A stale-scan guard drops results once `current_dir` has moved on (rapid
/// folder navigation), so a slow scan never overwrites a newer directory.
fn populate_dir(
    store: &gio::ListStore,
    scan_gen: &Rc<Cell<u64>>,
    dir: &Path,
    is_media_file: fn(&str) -> bool,
) {
    store.remove_all();

    if let Some(parent) = dir.parent() {
        store.append(&BrowserNode::folder("..", &parent.to_string_lossy()));
    }

    // Bump the generation and capture it; only this scan's result is current.
    // A path comparison is not enough: navigating A→B→A spawns two scans of
    // the same dir, and both would pass a `current_dir == A` check and double
    // the store. The generation uniquely identifies each populate_dir call.
    let generation = scan_gen.get().wrapping_add(1);
    scan_gen.set(generation);

    let dir = dir.to_path_buf();
    let store = store.clone();
    let scan_gen = scan_gen.clone();
    spawn_blocking_result(
        move || scan_dir_entries(&dir, is_media_file),
        move |entries| {
            if scan_gen.get() != generation {
                return;
            }
            for (name, path_str, is_dir) in entries {
                if is_dir {
                    store.append(&BrowserNode::folder(&name, &path_str));
                } else {
                    store.append(&BrowserNode::file_node(&name, &path_str));
                }
            }
        },
    );
}

/// Blocking directory walk run off the main thread: returns
/// `(name, path, is_dir)` for each visible sub-folder and media file, folders
/// first then case-insensitive alphabetical.
fn scan_dir_entries(dir: &Path, is_media_file: fn(&str) -> bool) -> Vec<(String, String, bool)> {
    let mut entries: Vec<(String, PathBuf, bool)> = Vec::new();
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                entries.push((name, path, true));
            } else if is_media_file(&path.to_string_lossy()) {
                entries.push((name, path, false));
            }
        }
    }

    entries.sort_by(|a, b| {
        b.2.cmp(&a.2)
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });

    entries
        .into_iter()
        .map(|(name, path, is_dir)| (name, path.to_string_lossy().to_string(), is_dir))
        .collect()
}
