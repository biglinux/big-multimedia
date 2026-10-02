//! Model/store contracts for the audio library tree.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use crate::audio::browser_node::{BrowserNode, BrowserNodeKind};
use crate::db::models::Track;

/// Threshold above which a group level gets A/B/C… letter dividers.
pub(super) const DIVIDER_THRESHOLD: usize = 10;

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

/// A group's preloaded children: plain data until the row is first expanded,
/// then the `gio::ListStore` the tree shows. Building every store up front made
/// one `BrowserNode` per library track on the main loop, expanded or not.
pub(super) enum ChildEntry {
    Pending(Vec<LibraryTreeNode>),
    Built(gio::ListStore),
}

/// Maps a node's identity to its children. Keyed exactly as
/// `"{kind as u32}:{key}"` (see [`child_store_key`]) so the create-func is a
/// pure HashMap lookup with no fallback DB path.
pub(super) type ChildStores = Rc<RefCell<HashMap<String, ChildEntry>>>;

/// Resolved-track lookup for drag-export, keyed by file path. Kept off the
/// GObject so the drag-prepare closure stays DB-free (push-model).
pub(super) type LeafTracks = Rc<RefCell<HashMap<String, Track>>>;

/// Handle the panel keeps to push tree data after construction. Owns the root
/// store + the preloaded child stores; `set_tree` splices both atomically.
pub(in crate::audio::ui) struct LibraryTreeHandle {
    pub(super) root_store: gio::ListStore,
    pub(super) child_stores: ChildStores,
    pub(super) leaf_tracks: LeafTracks,
}

impl LibraryTreeHandle {
    /// Replace the whole tree from freshly-preloaded data. Registers the
    /// children to mount on expand, then splices the root store in place (the
    /// `ListView`/`TreeListModel`/selection are never rebuilt — the RSS-leak
    /// invariant).
    pub(in crate::audio::ui) fn set_tree(&self, tree_snapshot: LibraryTreeSnapshot) {
        let mut stores = self.child_stores.borrow_mut();
        let mut leaves = self.leaf_tracks.borrow_mut();
        stores.clear();
        leaves.clear();

        let roots: Vec<BrowserNode> = tree_snapshot
            .level0
            .into_iter()
            .map(|node| build_node(node, &mut stores, &mut leaves))
            .collect();
        drop(stores);
        drop(leaves);

        self.root_store.splice(0, self.root_store.n_items(), &roots);
    }
}

/// Lowercase-insensitive first letter for a group divider; non-alpha → `#`.
pub(super) fn group_divider_letter(value: &str) -> char {
    let first = value.chars().next().unwrap_or('#');
    if first.is_alphabetic() {
        first.to_uppercase().next().unwrap_or(first)
    } else {
        '#'
    }
}

/// The `ChildStores` key of a node.
pub(super) fn child_store_key(kind: BrowserNodeKind, key: &str) -> String {
    format!("{}:{key}", kind as u32)
}

/// Convert one `LibraryTreeNode` into a `BrowserNode`, registering its track
/// for drag/export and leaving its children pending in `stores` until
/// [`child_store`] mounts them.
pub(super) fn build_node(
    mut node: LibraryTreeNode,
    stores: &mut HashMap<String, ChildEntry>,
    leaves: &mut HashMap<String, Track>,
) -> BrowserNode {
    let gnode = to_browser_node(&node);
    let children = std::mem::take(&mut node.children);
    if !children.is_empty() {
        stores.insert(
            child_store_key(node.kind, &node.key),
            ChildEntry::Pending(children),
        );
    }
    if let Some(track) = node.track {
        leaves.insert(node.key, track);
    }
    gnode
}

/// The child store for `key`, built with its letter dividers the first time a
/// row asks for it. `None` for a node without children.
pub(super) fn child_store(
    key: &str,
    stores: &mut HashMap<String, ChildEntry>,
    leaves: &mut HashMap<String, Track>,
) -> Option<gio::ListStore> {
    let children = match stores.remove(key)? {
        ChildEntry::Built(store) => {
            stores.insert(key.to_owned(), ChildEntry::Built(store.clone()));
            return Some(store);
        }
        ChildEntry::Pending(children) => children,
    };
    let child_store = gio::ListStore::new::<BrowserNode>();
    let mut last_letter: Option<char> = None;
    let use_dividers = children.len() > DIVIDER_THRESHOLD;
    let mut items = Vec::with_capacity(children.len());
    for child in children {
        if use_dividers && child.kind != BrowserNodeKind::Divider && !child.label.is_empty() {
            let letter = group_divider_letter(&child.label);
            if last_letter != Some(letter) {
                last_letter = Some(letter);
                items.push(BrowserNode::divider(&letter.to_string()));
            }
        }
        items.push(build_node(child, stores, leaves));
    }
    child_store.splice(0, 0, &items);
    stores.insert(key.to_owned(), ChildEntry::Built(child_store.clone()));
    Some(child_store)
}

/// Build the leaf-level `BrowserNode` for a tree node (no children attached —
/// expansion is resolved through the child-store map).
pub(super) fn to_browser_node(node: &LibraryTreeNode) -> BrowserNode {
    let gnode = match node.kind {
        BrowserNodeKind::All => BrowserNode::all(&node.label, node.count),
        BrowserNodeKind::Artist => BrowserNode::artist(&node.label, node.count),
        BrowserNodeKind::Album => BrowserNode::album(&node.label, &node.secondary, node.count),
        BrowserNodeKind::Genre => BrowserNode::genre(&node.label, node.count),
        BrowserNodeKind::Year => BrowserNode::year(node.key.parse().unwrap_or(0), node.count),
        BrowserNodeKind::VariousArtists => BrowserNode::various_artists(node.count),
        BrowserNodeKind::Divider => BrowserNode::divider(&node.label),
        BrowserNodeKind::File => {
            let secondary = if node.secondary.is_empty() {
                let duration = node.track.as_ref().map_or(0.0, |t| t.duration_secs);
                big_media_components::time::playback_time(duration)
            } else {
                node.secondary.clone()
            };
            BrowserNode::track_node(&node.label, &node.key, secondary)
        }
        _ => BrowserNode::file_node(&node.label, &node.key),
    };
    // `key` is authoritative for resolution (year stores the raw string).
    if !node.key.is_empty() && gnode.key() != node.key {
        gnode.set_key(node.key.as_str());
    }
    gnode
}
