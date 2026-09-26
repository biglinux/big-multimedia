//! Model/store contracts for the audio library tree.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;

use crate::audio::browser_node::{BrowserNode, BrowserNodeKind};
use crate::db::models::Track;
use crate::i18n::i18n;

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

/// Maps a node's identity to its preloaded child `gio::ListStore`. Keyed exactly
/// as `"{kind as u32}:{key}"` so the create-func is a pure
/// HashMap lookup with no fallback DB path.
pub(super) type ChildStores = Rc<RefCell<HashMap<String, gio::ListStore>>>;

/// Resolved-track lookup for drag-export, keyed by file path. Kept off the
/// GObject so the drag-prepare closure stays DB-free (push-model).
pub(super) type LeafTracks = Rc<RefCell<HashMap<String, Track>>>;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct BrowserNodeContract {
    pub(super) kind: BrowserNodeKind,
    pub(super) label: String,
    pub(super) key: String,
    pub(super) secondary: String,
    pub(super) track_count: i32,
}

/// Handle the panel keeps to push tree data after construction. Owns the root
/// store + the preloaded child stores; `set_tree` splices both atomically.
pub(in crate::audio::ui) struct LibraryTreeHandle {
    pub(super) root_store: gio::ListStore,
    pub(super) child_stores: ChildStores,
    pub(super) leaf_tracks: LeafTracks,
}

impl LibraryTreeHandle {
    /// Replace the whole tree from freshly-preloaded data. Rebuilds the child-
    /// store map, then splices the root store in place (the `ListView`/
    /// `TreeListModel`/selection are never rebuilt — the RSS-leak invariant).
    pub(in crate::audio::ui) fn set_tree(&self, tree_snapshot: LibraryTreeSnapshot) {
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
pub(super) fn group_divider_letter(value: &str) -> char {
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
pub(super) fn build_node_recursive(
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
pub(super) fn to_browser_node(node: &LibraryTreeNode) -> BrowserNode {
    let contract = browser_node_contract_for_tree_node(node);
    let gnode = match contract.kind {
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
    if !contract.key.is_empty() {
        gnode.set_key(contract.key);
    }
    gnode
}

pub(super) fn browser_node_contract_for_tree_node(node: &LibraryTreeNode) -> BrowserNodeContract {
    let duration = node.track.as_ref().map_or(0.0, |track| track.duration_secs);
    let (label, secondary) = match node.kind {
        BrowserNodeKind::Year => (
            node.key
                .parse::<i32>()
                .ok()
                .filter(|year| *year > 0)
                .map_or_else(String::new, |year| year.to_string()),
            String::new(),
        ),
        BrowserNodeKind::VariousArtists => (i18n("Various Artists"), String::new()),
        BrowserNodeKind::File => (
            node.label.clone(),
            if node.secondary.is_empty() {
                big_media_components::time::playback_time(duration)
            } else {
                node.secondary.clone()
            },
        ),
        _ => (node.label.clone(), node.secondary.clone()),
    };
    BrowserNodeContract {
        kind: node.kind,
        label,
        key: node.key.clone(),
        secondary,
        track_count: node.count,
    }
}
