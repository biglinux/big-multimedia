//! Left-pane library tree for the `LibraryPanel` (push-model).
//!
//! A `gtk::ListView` over a `gtk::TreeListModel` whose root is a
//! `gio::ListStore` of [`BrowserNode`]. The shell runs the full group+preload
//! query once and pushes the ENTIRE tree as pure (non-GTK) [`LibraryTreeSnapshot`]
//! via `SetLibraryTree`; the `TreeListModel` create-func then builds each child
//! `gio::ListStore` SYNCHRONOUSLY from the preloaded [`LibraryTreeNode`]
//! children (never a DB hit on expand — atomic preload is the generation guard).
//!
//! Row activation emits [`LibraryPanelOutput::NodeSelected`]; the shell resolves
//! the node into tracks and pushes them back via `PopulateDetail`. Letter
//! dividers are inserted when a group level has more than ten real entries
//! (mirrors `window::library::group_nodes::group_divider_letter`). Per-row
//! gestures are recycled-safe (signal id stashed on bind, disconnected on
//! unbind); the tree data changes via `splice`, never a full view rebuild.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use relm4::ComponentSender;

use big_relm4_components::feedback::context_popover::BigContextPopover;
use big_relm4_components::list::signal_factory;

use crate::audio::browser_node::{BrowserNode, BrowserNodeKind};
use crate::db::models::Track;
use crate::i18n::i18n;

use super::library_panel::{LibraryPanel, LibraryPanelInput, LibraryPanelOutput};
use super::playlist_geom::find_clicked_position;

/// Threshold above which a group level gets A/B/C… letter dividers.
const DIVIDER_THRESHOLD: usize = 10;
/// Album-art thumbnail edge (px) for file leaves.
const LEAF_ART_PX: i32 = 48;

/// Pure (non-GTK) library-tree payload pushed by the shell via `SetLibraryTree`.
/// Built on a worker thread from the group+preload query, then converted to
/// `BrowserNode` stores on the main thread.
#[derive(Debug, Clone, Default)]
pub struct LibraryTreeSnapshot {
    /// Top-level nodes (All Tracks, Various Artists, then groups), in order.
    pub level0: Vec<LibraryTreeNode>,
}

/// One node in the preloaded tree. Carries its already-resolved children so the
/// `TreeListModel` create-func never queries on expand.
#[derive(Debug, Clone)]
pub struct LibraryTreeNode {
    /// Node kind (drives icon, expandability, and `NodeSelected`).
    pub kind: BrowserNodeKind,
    /// Display label.
    pub label: String,
    /// Resolution key (artist/album/genre/year string, file path, …).
    pub key: String,
    /// Track count badge (0 = hidden).
    pub count: i32,
    /// Secondary payload: album artist, or the tab-joined leaf metadata
    /// (`album\tduration\tartist\tart_hash`) used by the file-leaf binder.
    pub secondary: String,
    /// Preloaded children (already grouped/sorted by the shell). Empty = leaf or
    /// not-yet-expandable; a non-empty vec is mounted synchronously on expand.
    pub children: Vec<LibraryTreeNode>,
    /// The resolved track for a `File` leaf (drag/export source), if any.
    pub track: Option<Track>,
}

impl Default for LibraryTreeNode {
    fn default() -> Self {
        Self {
            kind: BrowserNodeKind::All,
            label: String::new(),
            key: String::new(),
            count: 0,
            secondary: String::new(),
            children: Vec::new(),
            track: None,
        }
    }
}

/// Maps a node's identity to its preloaded child `gio::ListStore`. Keyed exactly
/// like the legacy window (`"{kind as u32}:{key}"`) so the create-func is a pure
/// HashMap lookup with no fallback DB path.
type ChildStores = Rc<RefCell<HashMap<String, gio::ListStore>>>;

/// Resolved-track lookup for drag-export, keyed by file path. Kept off the
/// GObject so the drag-prepare closure stays DB-free (push-model).
type LeafTracks = Rc<RefCell<HashMap<String, Track>>>;

/// Handle the panel keeps to push tree data after construction. Owns the root
/// store + the preloaded child stores; `set_tree` splices both atomically.
pub(super) struct LibraryTreeHandle {
    root_store: gio::ListStore,
    child_stores: ChildStores,
    leaf_tracks: LeafTracks,
}

impl LibraryTreeHandle {
    /// Replace the whole tree from freshly-preloaded data. Rebuilds the child-
    /// store map, then splices the root store in place (the `ListView`/
    /// `TreeListModel`/selection are never rebuilt — the RSS-leak invariant).
    pub(super) fn set_tree(&self, tree_snapshot: LibraryTreeSnapshot) {
        let mut stores = self.child_stores.borrow_mut();
        let mut leaves = self.leaf_tracks.borrow_mut();
        stores.clear();
        leaves.clear();

        let roots: Vec<BrowserNode> = tree_snapshot
            .level0
            .iter()
            .map(|node| build_node_recursive(node, &mut stores, &mut leaves))
            .collect();
        drop(stores);
        drop(leaves);

        self.root_store.splice(0, self.root_store.n_items(), &roots);
    }
}

/// Lowercase-insensitive first letter for a group divider; non-alpha → `#`.
/// Reimplements `window::library::group_nodes::group_divider_letter`.
fn group_divider_letter(value: &str) -> char {
    let first = value.chars().next().unwrap_or('#');
    if first.is_alphabetic() {
        first.to_uppercase().next().unwrap_or(first)
    } else {
        '#'
    }
}

/// Convert one `LibraryTreeNode` (and its subtree) into a `BrowserNode`,
/// registering its child store (with letter dividers) into `stores` so the
/// create-func can mount it synchronously on expand.
fn build_node_recursive(
    node: &LibraryTreeNode,
    stores: &mut HashMap<String, gio::ListStore>,
    leaves: &mut HashMap<String, Track>,
) -> BrowserNode {
    let gnode = to_browser_node(node);
    if let Some(track) = &node.track {
        leaves.insert(node.key.clone(), track.clone());
    }

    if !node.children.is_empty() {
        let cache_key = format!("{}:{}", node.kind as u32, node.key);
        let child_store = gio::ListStore::new::<BrowserNode>();
        let mut last_letter: Option<char> = None;
        let use_dividers = node.children.len() > DIVIDER_THRESHOLD;
        for child in &node.children {
            if use_dividers && child.kind != BrowserNodeKind::Divider && !child.label.is_empty() {
                let letter = group_divider_letter(&child.label);
                if last_letter != Some(letter) {
                    last_letter = Some(letter);
                    child_store.append(&BrowserNode::divider(&letter.to_string()));
                }
            }
            child_store.append(&build_node_recursive(child, stores, leaves));
        }
        stores.insert(cache_key, child_store);
    }
    gnode
}

/// Build the leaf-level `BrowserNode` for a tree node (no children attached —
/// expansion is resolved through the child-store map).
fn to_browser_node(node: &LibraryTreeNode) -> BrowserNode {
    let gnode = match node.kind {
        BrowserNodeKind::All => BrowserNode::all(&node.label, node.count),
        BrowserNodeKind::Artist => BrowserNode::artist(&node.label, node.count),
        BrowserNodeKind::Album => BrowserNode::album(&node.label, &node.secondary, node.count),
        BrowserNodeKind::Genre => BrowserNode::genre(&node.label, node.count),
        BrowserNodeKind::Year => BrowserNode::year(node.key.parse().unwrap_or(0), node.count),
        BrowserNodeKind::VariousArtists => BrowserNode::various_artists(node.count),
        BrowserNodeKind::Divider => BrowserNode::divider(&node.label),
        BrowserNodeKind::File => {
            let duration = node.track.as_ref().map_or(0.0, |t| t.duration_secs);
            let gnode = BrowserNode::track_node(&node.label, &node.key, duration);
            if !node.secondary.is_empty() {
                gnode.set_secondary(node.secondary.clone());
            }
            gnode
        }
        _ => BrowserNode::file_node(&node.label, &node.key),
    };
    // `key` is authoritative for resolution (year stores the raw string).
    if !node.key.is_empty() {
        gnode.set_key(node.key.clone());
    }
    gnode
}

/// Build the library tree widget. Returns the `ListView` (the panel mounts it
/// into its left-pane `gtk::ScrolledWindow` via `set_child`) and a
/// [`LibraryTreeHandle`] the panel keeps to apply `SetLibraryTree`.
pub(super) fn build_library_tree(
    sender: &ComponentSender<LibraryPanel>,
) -> (gtk::ListView, LibraryTreeHandle) {
    let root_store = gio::ListStore::new::<BrowserNode>();
    let child_stores: ChildStores = Rc::new(RefCell::new(HashMap::new()));
    let leaf_tracks: LeafTracks = Rc::new(RefCell::new(HashMap::new()));

    let tree_model = build_tree_model(&root_store, &child_stores);
    let selection = gtk::MultiSelection::new(Some(tree_model));
    let factory = build_tree_factory(&leaf_tracks, sender);

    let list_view = gtk::ListView::builder()
        .model(&selection)
        .factory(&factory)
        .vexpand(true)
        .build();
    list_view.add_css_class("navigation-sidebar");
    list_view.update_property(&[gtk::accessible::Property::Label(&i18n("Music library"))]);

    wire_activation(&list_view, sender);
    wire_context_menu(&list_view, &leaf_tracks, sender);
    list_view.add_controller(build_leaf_drag_source(&selection, &leaf_tracks));

    let handle = LibraryTreeHandle {
        root_store,
        child_stores,
        leaf_tracks,
    };
    (list_view, handle)
}

/// `TreeListModel` whose create-func mounts the preloaded child store for a row
/// (synchronous HashMap lookup; `None` for leaves/dividers).
fn build_tree_model(root_store: &gio::ListStore, child_stores: &ChildStores) -> gtk::TreeListModel {
    let stores = child_stores.clone();
    gtk::TreeListModel::new(root_store.clone(), false, false, move |model_item| {
        let node = model_item.downcast_ref::<BrowserNode>()?;
        if matches!(
            node.kind(),
            BrowserNodeKind::Divider | BrowserNodeKind::File
        ) {
            return None;
        }
        let cache_key = format!("{}:{}", node.kind() as u32, node.key());
        let store = stores.borrow().get(&cache_key)?.clone();
        Some(store.upcast::<gio::ListModel>())
    })
}

/// Row factory: a `TreeExpander` wrapping icon + avatar + label/artist + count.
fn build_tree_factory(
    leaf_tracks: &LeafTracks,
    sender: &ComponentSender<LibraryPanel>,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        let expander = gtk::TreeExpander::builder().indent_for_icon(true).build();
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_top(1)
            .margin_bottom(1)
            .margin_start(2)
            .margin_end(6)
            .build();
        let icon = gtk::Image::new();
        let avatar = adw::Avatar::builder()
            .size(32)
            .show_initials(true)
            .visible(false)
            .build();
        let text_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        let label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(24)
            .build();
        let artist_label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(24)
            .visible(false)
            .build();
        artist_label.add_css_class("browser-artist");
        let track_count_label = gtk::Label::builder().halign(gtk::Align::End).build();
        track_count_label.add_css_class("caption");
        text_box.append(&label);
        text_box.append(&artist_label);
        row.append(&icon);
        row.append(&avatar);
        row.append(&text_box);
        row.append(&track_count_label);
        row.append(&build_show_tracks_button());
        expander.set_child(Some(&row));
        list_item.set_child(Some(&expander));
    });

    let leaves = leaf_tracks.clone();
    let out = sender.clone();
    factory.connect_bind(move |_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        bind_tree_row(list_item, &leaves, &out);
    });
    factory.connect_unbind(|_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        let Some(expander) = signal_factory::child_as::<gtk::TreeExpander>(list_item) else {
            return;
        };
        let Some(row) = expander.child().and_downcast::<gtk::Box>() else {
            return;
        };
        let Some(show_tracks_button) = row.last_child().and_downcast::<gtk::Button>() else {
            return;
        };
        disconnect_show_tracks_button(&show_tracks_button);
    });

    factory
}

/// Bind one `BrowserNode` onto the row widgets; stashes `vis-pos` for hit-tests
/// and kicks off the off-thread art thumbnail for file leaves.
fn bind_tree_row(
    list_item: &gtk::ListItem,
    leaves: &LeafTracks,
    sender: &ComponentSender<LibraryPanel>,
) {
    let Some(tree_row) = signal_factory::bound_model_as::<gtk::TreeListRow>(list_item) else {
        return;
    };
    let Some(node) = tree_row.item().and_downcast::<BrowserNode>() else {
        return;
    };
    let Some(expander) = signal_factory::child_as::<gtk::TreeExpander>(list_item) else {
        return;
    };
    expander.set_list_row(Some(&tree_row));

    let Some(row) = expander.child().and_downcast::<gtk::Box>() else {
        return;
    };
    // SAFETY: read by the right-click hit-test to resolve a clicked row index.
    unsafe {
        row.set_data("vis-pos", list_item.position());
    }
    let Some(icon) = row.first_child().and_downcast::<gtk::Image>() else {
        return;
    };
    let Some(avatar) = icon.next_sibling().and_downcast::<adw::Avatar>() else {
        return;
    };
    let Some(text_box) = avatar.next_sibling().and_downcast::<gtk::Box>() else {
        return;
    };
    let Some(label) = text_box.first_child().and_downcast::<gtk::Label>() else {
        return;
    };
    let Some(artist_label) = label.next_sibling().and_downcast::<gtk::Label>() else {
        return;
    };
    let Some(track_count_label) = text_box.next_sibling().and_downcast::<gtk::Label>() else {
        return;
    };
    let Some(show_tracks_button) = track_count_label
        .next_sibling()
        .and_downcast::<gtk::Button>()
    else {
        return;
    };
    disconnect_show_tracks_button(&show_tracks_button);

    let kind = node.kind();
    if kind == BrowserNodeKind::Divider {
        icon.set_visible(false);
        avatar.set_visible(false);
        label.set_label(&node.label());
        label.add_css_class("heading");
        artist_label.set_visible(false);
        track_count_label.set_visible(false);
        show_tracks_button.set_visible(false);
        list_item.set_activatable(false);
        list_item.set_selectable(false);
        return;
    }
    label.remove_css_class("heading");
    list_item.set_activatable(true);
    list_item.set_selectable(true);
    icon.set_icon_name(Some(&node.icon_name()));
    label.set_label(&node.label());

    if kind == BrowserNodeKind::File {
        bind_file_leaf(
            &node,
            &icon,
            &avatar,
            &label,
            &artist_label,
            &track_count_label,
            leaves,
        );
        show_tracks_button.set_visible(false);
        return;
    }

    artist_label.set_visible(false);
    label.remove_css_class("browser-title");
    big_relm4_components::feedback::tooltip::set(&text_box, &node.label());

    let count = node.track_count();
    track_count_label.set_visible(count > 0);
    track_count_label.set_label(&if count > 0 {
        count.to_string()
    } else {
        String::new()
    });

    let is_album = kind == BrowserNodeKind::Album;
    let is_va = kind == BrowserNodeKind::VariousArtists;
    avatar.set_visible(is_album || is_va);
    icon.set_visible(!is_album && !is_va);
    if is_album {
        avatar.set_text(Some(&node.label()));
        avatar.set_custom_image(None::<&gdk::Paintable>);
    } else if is_va {
        avatar.set_text(Some("Various Artists"));
        avatar.set_icon_name(Some("system-users-symbolic"));
    }
    bind_show_tracks_button(&show_tracks_button, &node, tree_row.depth(), sender);
}

fn build_show_tracks_button() -> gtk::Button {
    gtk::Button::builder()
        .icon_name("go-next-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular"])
        .visible(false)
        .build()
}

fn bind_show_tracks_button(
    button: &gtk::Button,
    node: &BrowserNode,
    depth: u32,
    sender: &ComponentSender<LibraryPanel>,
) {
    let action_label = format!("{}: {}", i18n("Show tracks"), node.label());
    button.set_visible(true);
    button.update_property(&[gtk::accessible::Property::Label(&action_label)]);
    big_relm4_components::feedback::tooltip::set(button, &action_label);

    let out = sender.clone();
    let kind = node.kind() as u32;
    let key = node.key();
    let secondary = node.secondary();
    let click_signal_id = button.connect_clicked(move |_| {
        let _ = out.output(LibraryPanelOutput::NodeSelected {
            kind,
            key: key.clone(),
            secondary: secondary.clone(),
            depth,
        });
    });
    unsafe {
        button.set_data("show-tracks-click-signal-id", click_signal_id);
    }
}

fn disconnect_show_tracks_button(button: &gtk::Button) {
    unsafe {
        if let Some(signal_id) =
            button.steal_data::<glib::SignalHandlerId>("show-tracks-click-signal-id")
        {
            button.disconnect(signal_id);
        }
    }
}

/// Bind a file leaf: parse the tab-joined secondary (`album\tduration\tartist\
/// thash`) into the duration badge + artist sub-label, then load embedded art
/// off-thread (push-model file IO via `spawn_blocking_result`).
fn bind_file_leaf(
    node: &BrowserNode,
    icon: &gtk::Image,
    avatar: &adw::Avatar,
    label: &gtk::Label,
    artist_label: &gtk::Label,
    track_count_label: &gtk::Label,
    leaves: &LeafTracks,
) {
    let secondary = node.secondary();
    let parts: Vec<&str> = secondary.splitn(4, '\t').collect();
    if parts.len() < 2 {
        avatar.set_visible(false);
        icon.set_visible(true);
        artist_label.set_visible(false);
        track_count_label.set_visible(!secondary.is_empty());
        track_count_label.set_label(&secondary);
        return;
    }

    let duration = parts[1];
    let artist = parts.get(2).copied().unwrap_or("");
    track_count_label.set_label(duration);
    track_count_label.set_visible(true);
    track_count_label.add_css_class("browser-duration");
    avatar.set_visible(true);
    icon.set_visible(false);
    label.add_css_class("browser-title");
    if artist.is_empty() {
        artist_label.set_visible(false);
    } else {
        artist_label.set_label(artist);
        artist_label.set_visible(true);
    }
    let initials = if artist.is_empty() {
        node.label()
    } else {
        artist.to_string()
    };
    avatar.set_text(Some(&initials));
    avatar.set_custom_image(None::<&gdk::Paintable>);

    // Resolve the source path from the leaf-track map (set on splice) and load
    // the embedded thumbnail off-thread. No DB hit — file IO only.
    let file_path = node.key();
    let has_art = leaves
        .borrow()
        .get(&file_path)
        .is_some_and(|t| t.art_hash.as_deref().is_some_and(|h| !h.is_empty()));
    if !has_art {
        return;
    }
    let avatar = avatar.clone();
    big_media_components::player_chrome::spawn_blocking_result(
        move || crate::library::metadata::extract_album_art(&file_path),
        move |data| {
            let Some(data) = data else { return };
            if let Some(texture) =
                big_media_components::media::images::bytes_to_thumbnail(&data, LEAF_ART_PX)
            {
                avatar.set_custom_image(Some(&texture));
            }
        },
    );
}

/// Single-click/keyboard activation → resolve the row's node → `NodeSelected`
/// (the shell queries tracks → `PopulateDetail`). Dividers are inert.
fn wire_activation(list_view: &gtk::ListView, sender: &ComponentSender<LibraryPanel>) {
    let out = sender.clone();
    list_view.connect_activate(move |list_view, position| {
        let Some(model) = list_view.model() else {
            return;
        };
        let Some(tree_row) = model.item(position).and_downcast::<gtk::TreeListRow>() else {
            return;
        };
        let Some(node) = tree_row.item().and_downcast::<BrowserNode>() else {
            return;
        };
        if node.kind() == BrowserNodeKind::Divider {
            return;
        }
        let _ = out.output(LibraryPanelOutput::NodeSelected {
            kind: node.kind() as u32,
            key: node.key(),
            secondary: node.secondary(),
            depth: tree_row.depth(),
        });
    });
}

/// Right-click context menu on the tree (mirrors the god-object browser menu):
/// group nodes get Play (enqueue all); file leaves get the full track menu
/// (play / queue / add-to-playlist / show / edit tags / info).
fn wire_context_menu(
    list_view: &gtk::ListView,
    leaf_tracks: &LeafTracks,
    sender: &ComponentSender<LibraryPanel>,
) {
    let context_click = gtk::GestureClick::builder().button(3).build();
    // Weak: this controller lives ON the list view — a strong clone is a
    // controller⇄widget self-cycle that pins the whole tree after close.
    let list_view_weak = list_view.downgrade();
    let leaves = leaf_tracks.clone();
    let out = sender.clone();
    context_click.connect_released(move |gesture, _click_count, x, y| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        let Some(list_view) = list_view_weak.upgrade() else {
            return;
        };
        let Some(position) = find_clicked_position(list_view.upcast_ref::<gtk::Widget>(), x, y)
        else {
            return;
        };
        let Some(model) = list_view.model() else {
            return;
        };
        let Some(tree_row) = model.item(position).and_downcast::<gtk::TreeListRow>() else {
            return;
        };
        let Some(node) = tree_row.item().and_downcast::<BrowserNode>() else {
            return;
        };
        if node.kind() == BrowserNodeKind::Divider {
            return;
        }
        let menu = BigContextPopover::new(false);
        if node.kind() == BrowserNodeKind::File {
            let Some(track) = leaves.borrow().get(&node.key()).cloned() else {
                return;
            };
            add_leaf_menu_items(&menu, &track, &out);
        } else {
            // Group node: a single Play that enqueues all of its tracks.
            let output_sender = out.clone();
            let (kind, key, secondary, depth) = (
                node.kind() as u32,
                node.key(),
                node.secondary(),
                tree_row.depth(),
            );
            menu.append_button(i18n("Play"), &[], move || {
                let _ = output_sender.output(LibraryPanelOutput::NodeSelected {
                    kind,
                    key: key.clone(),
                    secondary: secondary.clone(),
                    depth,
                });
            });
        }
        menu.popup_at(list_view.upcast_ref::<gtk::Widget>(), x, y);
    });
    list_view.add_controller(context_click);
}

/// Append the per-track menu items for a file leaf.
fn add_leaf_menu_items(
    menu: &BigContextPopover,
    track: &Track,
    sender: &ComponentSender<LibraryPanel>,
) {
    {
        let (output_sender, track) = (sender.clone(), track.clone());
        menu.append_button(i18n("Play"), &[], move || {
            let _ = output_sender.output(LibraryPanelOutput::PlayTrack(track.clone()));
        });
    }
    {
        let (output_sender, track) = (sender.clone(), track.clone());
        menu.append_button(i18n("Add to Queue"), &[], move || {
            let _ = output_sender.output(LibraryPanelOutput::EnqueueTrack(track.clone()));
        });
    }
    menu.append_separator();
    {
        let (output_sender, track) = (sender.clone(), track.clone());
        menu.append_button(i18n("Add to Playlist…"), &[], move || {
            let _ = output_sender.output(LibraryPanelOutput::ShowAddToPlaylist(track.clone()));
        });
    }
    {
        let (output_sender, file_path) = (sender.clone(), track.file_path.clone());
        menu.append_button(i18n("Show in File Manager"), &[], move || {
            let _ =
                output_sender.output(LibraryPanelOutput::RevealInFileManager(file_path.clone()));
        });
    }
    {
        // "Track Info" opens the panel-owned tag editor (opened via an Input).
        let (input_sender, track) = (sender.clone(), track.clone());
        menu.append_button(i18n("Track Info"), &[], move || {
            input_sender.input(LibraryPanelInput::EditTags(track.clone()));
        });
    }
}

/// DragSource (COPY): exports the selected leaves' file paths, newline-joined,
/// for a drop onto the queue or an external target. DB-free — paths are read
/// straight off the selected `BrowserNode` keys (leaf nodes only).
fn build_leaf_drag_source(
    selection: &gtk::MultiSelection,
    leaf_tracks: &LeafTracks,
) -> gtk::DragSource {
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::COPY);
    let selection_model = selection.clone();
    let leaves = leaf_tracks.clone();
    drag.connect_prepare(move |_, _, _| {
        let bitset = selection_model.selection();
        if bitset.is_empty() {
            return None;
        }
        let known = leaves.borrow();
        let mut paths = Vec::with_capacity(bitset.size() as usize);
        for selected_index in 0..bitset.size() {
            let Some(tree_row) = selection_model
                .item(bitset.nth(selected_index as u32))
                .and_downcast::<gtk::TreeListRow>()
            else {
                continue;
            };
            let Some(node) = tree_row.item().and_downcast::<BrowserNode>() else {
                continue;
            };
            if node.kind() == BrowserNodeKind::File && known.contains_key(&node.key()) {
                paths.push(node.key());
            }
        }
        if paths.is_empty() {
            return None;
        }
        Some(gdk::ContentProvider::for_value(
            &paths.join("\n").to_value(),
        ))
    });
    drag.connect_drag_begin(|drag_source, _| {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let icon = gtk::IconTheme::for_display(&display).lookup_icon(
            "audio-x-generic",
            &[],
            32,
            1,
            gtk::TextDirection::Ltr,
            gtk::IconLookupFlags::empty(),
        );
        drag_source.set_icon(Some(&icon), 16, 16);
    });
    drag
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library_tree_node(
        kind: BrowserNodeKind,
        label: &str,
        key: &str,
        count: i32,
        secondary: &str,
    ) -> LibraryTreeNode {
        LibraryTreeNode {
            kind,
            label: label.to_string(),
            key: key.to_string(),
            count,
            secondary: secondary.to_string(),
            children: Vec::new(),
            track: None,
        }
    }

    fn child_browser_node(store: &gio::ListStore, position: u32) -> Option<BrowserNode> {
        store.item(position).and_downcast::<BrowserNode>()
    }

    #[test]
    fn divider_letter_uppercases_alpha_else_hash() {
        assert_eq!(group_divider_letter("album"), 'A');
        assert_eq!(group_divider_letter("Éclair"), 'É');
        assert_eq!(group_divider_letter("9 Lives"), '#');
        assert_eq!(group_divider_letter(""), '#');
    }

    #[test]
    fn cache_key_matches_legacy_format() {
        let node = LibraryTreeNode {
            kind: BrowserNodeKind::Artist,
            key: "Radiohead".to_string(),
            ..Default::default()
        };
        let cache_key = format!("{}:{}", node.kind as u32, node.key);
        assert_eq!(cache_key, "1:Radiohead");
    }

    #[test]
    fn browser_node_contract_covers_every_library_tree_kind() {
        let various_artists_label = i18n("Various Artists");
        let cases = [
            (
                library_tree_node(BrowserNodeKind::All, "All songs", "", 12, ""),
                BrowserNodeKind::All,
                "All songs",
                "",
                "",
                12,
            ),
            (
                library_tree_node(BrowserNodeKind::Artist, "Alice", "artist-key", 3, ""),
                BrowserNodeKind::Artist,
                "Alice",
                "artist-key",
                "",
                3,
            ),
            (
                library_tree_node(BrowserNodeKind::Album, "Debut", "album-key", 9, "Alice"),
                BrowserNodeKind::Album,
                "Debut",
                "album-key",
                "Alice",
                9,
            ),
            (
                library_tree_node(BrowserNodeKind::Genre, "Jazz", "genre-key", 4, ""),
                BrowserNodeKind::Genre,
                "Jazz",
                "genre-key",
                "",
                4,
            ),
            (
                library_tree_node(BrowserNodeKind::Year, "ignored", "1997", 6, ""),
                BrowserNodeKind::Year,
                "1997",
                "1997",
                "",
                6,
            ),
            (
                library_tree_node(
                    BrowserNodeKind::VariousArtists,
                    "ignored",
                    "various-key",
                    5,
                    "",
                ),
                BrowserNodeKind::VariousArtists,
                various_artists_label.as_str(),
                "various-key",
                "",
                5,
            ),
            (
                library_tree_node(BrowserNodeKind::Divider, "A", "", 0, ""),
                BrowserNodeKind::Divider,
                "A",
                "",
                "",
                0,
            ),
        ];

        for (
            source_node,
            expected_kind,
            expected_label,
            expected_key,
            expected_secondary,
            expected_count,
        ) in cases
        {
            let browser_node = to_browser_node(&source_node);
            assert_eq!(browser_node.kind(), expected_kind);
            assert_eq!(browser_node.label(), expected_label);
            assert_eq!(browser_node.key(), expected_key);
            assert_eq!(browser_node.secondary(), expected_secondary);
            assert_eq!(browser_node.track_count(), expected_count);
        }
    }

    #[test]
    fn year_node_with_invalid_key_preserves_raw_key_and_uses_unknown_label() {
        let source_node = library_tree_node(BrowserNodeKind::Year, "ignored", "unknown", 2, "");
        let browser_node = to_browser_node(&source_node);

        assert_eq!(browser_node.kind(), BrowserNodeKind::Year);
        assert_eq!(browser_node.label(), "");
        assert_eq!(browser_node.key(), "unknown");
        assert_eq!(browser_node.track_count(), 2);
    }

    #[test]
    fn file_node_uses_track_duration_and_secondary_override() {
        let mut track = Track::new_external_path("/music/song.flac");
        track.duration_secs = 61.0;
        let source_node = LibraryTreeNode {
            kind: BrowserNodeKind::File,
            label: "Song".to_string(),
            key: "/music/song.flac".to_string(),
            track: Some(track),
            ..Default::default()
        };
        let browser_node = to_browser_node(&source_node);

        assert_eq!(browser_node.kind(), BrowserNodeKind::File);
        assert_eq!(browser_node.label(), "Song");
        assert_eq!(browser_node.key(), "/music/song.flac");
        assert_eq!(
            browser_node.secondary(),
            big_media_components::time::playback_time(61.0)
        );

        let source_node_with_secondary = LibraryTreeNode {
            secondary: "Album\t1:01\tArtist\thash".to_string(),
            ..source_node
        };
        let browser_node_with_secondary = to_browser_node(&source_node_with_secondary);
        assert_eq!(
            browser_node_with_secondary.secondary(),
            "Album\t1:01\tArtist\thash"
        );
    }

    #[test]
    fn build_node_recursive_adds_letter_dividers_only_above_threshold(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let threshold_children = (0..DIVIDER_THRESHOLD)
            .map(|index| {
                library_tree_node(
                    BrowserNodeKind::Artist,
                    &format!("Artist {index:02}"),
                    &format!("artist-{index:02}"),
                    1,
                    "",
                )
            })
            .collect();
        let threshold_parent = LibraryTreeNode {
            kind: BrowserNodeKind::Artist,
            label: "Artists".to_string(),
            key: "threshold".to_string(),
            children: threshold_children,
            ..Default::default()
        };
        let mut threshold_stores = HashMap::new();
        let mut threshold_leaves = HashMap::new();
        let threshold_browser_node = build_node_recursive(
            &threshold_parent,
            &mut threshold_stores,
            &mut threshold_leaves,
        );
        let threshold_store = threshold_stores
            .get(&format!(
                "{}:{}",
                threshold_browser_node.kind() as u32,
                threshold_browser_node.key()
            ))
            .ok_or_else(|| std::io::Error::other("threshold child store should exist"))?;
        assert_eq!(threshold_store.n_items(), DIVIDER_THRESHOLD as u32);
        assert_eq!(
            child_browser_node(threshold_store, 0).map(|browser_node| browser_node.kind()),
            Some(BrowserNodeKind::Artist)
        );

        let divider_children = [
            ("Alpha", "alpha"),
            ("Atom", "atom"),
            ("Beta", "beta"),
            ("Delta", "delta"),
            ("Echo", "echo"),
            ("Foxtrot", "foxtrot"),
            ("Golf", "golf"),
            ("Hotel", "hotel"),
            ("India", "india"),
            ("Juliet", "juliet"),
            ("Kilo", "kilo"),
        ]
        .into_iter()
        .map(|(label, key)| library_tree_node(BrowserNodeKind::Artist, label, key, 1, ""))
        .collect();
        let divider_parent = LibraryTreeNode {
            kind: BrowserNodeKind::Artist,
            label: "Artists".to_string(),
            key: "with-dividers".to_string(),
            children: divider_children,
            ..Default::default()
        };
        let mut divider_stores = HashMap::new();
        let mut divider_leaves = HashMap::new();
        let divider_browser_node =
            build_node_recursive(&divider_parent, &mut divider_stores, &mut divider_leaves);
        let divider_store = divider_stores
            .get(&format!(
                "{}:{}",
                divider_browser_node.kind() as u32,
                divider_browser_node.key()
            ))
            .ok_or_else(|| std::io::Error::other("divider child store should exist"))?;

        assert_eq!(divider_store.n_items(), 21);
        assert_eq!(
            child_browser_node(divider_store, 0).map(|browser_node| browser_node.kind()),
            Some(BrowserNodeKind::Divider)
        );
        assert_eq!(
            child_browser_node(divider_store, 0).map(|browser_node| browser_node.label()),
            Some("A".to_string())
        );
        assert_eq!(
            child_browser_node(divider_store, 3).map(|browser_node| browser_node.label()),
            Some("B".to_string())
        );
        assert_eq!(
            child_browser_node(divider_store, 4).map(|browser_node| browser_node.label()),
            Some("Beta".to_string())
        );
        Ok(())
    }

    #[test]
    fn build_node_recursive_registers_leaf_tracks_by_file_path() {
        let track = Track::new_external_path("/music/leaf.flac");
        let source_node = LibraryTreeNode {
            kind: BrowserNodeKind::File,
            label: "Leaf".to_string(),
            key: "/music/leaf.flac".to_string(),
            track: Some(track),
            ..Default::default()
        };
        let mut child_stores = HashMap::new();
        let mut leaf_tracks = HashMap::new();
        let browser_node = build_node_recursive(&source_node, &mut child_stores, &mut leaf_tracks);

        assert_eq!(browser_node.kind(), BrowserNodeKind::File);
        assert!(child_stores.is_empty());
        assert_eq!(
            leaf_tracks
                .get("/music/leaf.flac")
                .map(|registered_track| registered_track.file_path.as_str()),
            Some("/music/leaf.flac")
        );
    }

    #[test]
    fn tree_handle_set_tree_splices_roots_and_replaces_child_state(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let root_store = gio::ListStore::new::<BrowserNode>();
        let child_stores: ChildStores = Rc::new(RefCell::new(HashMap::new()));
        let leaf_tracks: LeafTracks = Rc::new(RefCell::new(HashMap::new()));
        let tree_handle = LibraryTreeHandle {
            root_store,
            child_stores,
            leaf_tracks,
        };
        let track = Track::new_external_path("/music/track.flac");
        let tree_snapshot = LibraryTreeSnapshot {
            level0: vec![LibraryTreeNode {
                kind: BrowserNodeKind::Artist,
                label: "Alice".to_string(),
                key: "artist-key".to_string(),
                children: vec![LibraryTreeNode {
                    kind: BrowserNodeKind::File,
                    label: "Track".to_string(),
                    key: "/music/track.flac".to_string(),
                    track: Some(track),
                    ..Default::default()
                }],
                ..Default::default()
            }],
        };

        tree_handle.set_tree(tree_snapshot);

        assert_eq!(tree_handle.root_store.n_items(), 1);
        let root_browser_node = child_browser_node(&tree_handle.root_store, 0)
            .ok_or_else(|| std::io::Error::other("root browser node should exist"))?;
        assert_eq!(root_browser_node.kind(), BrowserNodeKind::Artist);
        assert_eq!(root_browser_node.key(), "artist-key");
        {
            let child_store_map = tree_handle.child_stores.borrow();
            let child_store = child_store_map
                .get("1:artist-key")
                .ok_or_else(|| std::io::Error::other("artist child store should exist"))?;
            assert_eq!(child_store.n_items(), 1);
            assert_eq!(
                child_browser_node(child_store, 0).map(|browser_node| browser_node.key()),
                Some("/music/track.flac".to_string())
            );
        }
        assert_eq!(
            tree_handle
                .leaf_tracks
                .borrow()
                .get("/music/track.flac")
                .map(|registered_track| registered_track.file_path.as_str()),
            Some("/music/track.flac")
        );

        tree_handle.set_tree(LibraryTreeSnapshot::default());

        assert_eq!(tree_handle.root_store.n_items(), 0);
        assert!(tree_handle.child_stores.borrow().is_empty());
        assert!(tree_handle.leaf_tracks.borrow().is_empty());
        Ok(())
    }
}
