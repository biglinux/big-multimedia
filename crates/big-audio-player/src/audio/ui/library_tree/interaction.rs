//! Pure interaction decisions for the audio library tree.

use std::collections::HashMap;

use crate::audio::browser_node::{BrowserNode, BrowserNodeKind};
use crate::db::models::Track;

use super::super::library_panel::LibraryPanelOutput;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LibraryTreeSelection {
    pub(super) kind: u32,
    pub(super) key: String,
    pub(super) secondary: String,
    pub(super) depth: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LibraryTreeContextTarget {
    Inert,
    FileLeaf { file_path: String },
    Group { selection: LibraryTreeSelection },
}

pub(super) fn selected_leaf_drag_paths<'a>(
    selected_browser_nodes: impl IntoIterator<Item = &'a BrowserNode>,
    registered_leaf_tracks: &HashMap<String, Track>,
) -> Vec<String> {
    selected_browser_nodes
        .into_iter()
        .filter_map(|node| {
            let file_path = node.key();
            (node.kind() == BrowserNodeKind::File
                && registered_leaf_tracks.contains_key(&file_path))
            .then_some(file_path)
        })
        .collect()
}

pub(super) fn library_tree_selection(
    node: &BrowserNode,
    depth: u32,
) -> Option<LibraryTreeSelection> {
    if node.kind() == BrowserNodeKind::Divider {
        return None;
    }
    Some(LibraryTreeSelection {
        kind: node.kind() as u32,
        key: node.key(),
        secondary: node.secondary(),
        depth,
    })
}

pub(super) fn library_tree_context_target(
    node: &BrowserNode,
    depth: u32,
    registered_leaf_tracks: &HashMap<String, Track>,
) -> LibraryTreeContextTarget {
    if node.kind() == BrowserNodeKind::Divider {
        return LibraryTreeContextTarget::Inert;
    }
    if node.kind() == BrowserNodeKind::File {
        let file_path = node.key();
        if registered_leaf_tracks.contains_key(&file_path) {
            return LibraryTreeContextTarget::FileLeaf { file_path };
        }
        return LibraryTreeContextTarget::Inert;
    }
    let Some(selection) = library_tree_selection(node, depth) else {
        return LibraryTreeContextTarget::Inert;
    };
    LibraryTreeContextTarget::Group { selection }
}

pub(super) fn library_panel_node_selected_output(
    selection: &LibraryTreeSelection,
) -> LibraryPanelOutput {
    LibraryPanelOutput::NodeSelected {
        kind: selection.kind,
        key: selection.key.clone(),
        secondary: selection.secondary.clone(),
        depth: selection.depth,
    }
}
