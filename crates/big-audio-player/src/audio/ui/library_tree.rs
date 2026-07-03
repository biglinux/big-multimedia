//! Left-pane library tree for the `LibraryPanel` (push-model).
//!
//! A `gtk::ListView` over a `gtk::TreeListModel` whose root is a
//! `gio::ListStore` of [`BrowserNode`]. The shell runs the full group+preload
//! query once and pushes the ENTIRE tree as pure (non-GTK) [`LibraryTreeSnapshot`]
//! via `SetLibraryTree`; the `TreeListModel` create-func then builds each child
//! `gio::ListStore` SYNCHRONOUSLY from the preloaded [`LibraryTreeNode`]
//! children (never a DB hit on expand — atomic preload is the generation guard).
//!
//! Row activation emits `LibraryPanelOutput::NodeSelected`; the shell resolves
//! the node into tracks and pushes them back via `PopulateDetail`. Letter
//! dividers are inserted when a group level has more than ten real entries
//! (mirrors `window::library::group_nodes::group_divider_letter`). Per-row
//! gestures are recycled-safe (signal id stashed on bind, disconnected on
//! unbind); the tree data changes via `splice`, never a full view rebuild.

#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::rc::Rc;

#[cfg(test)]
use crate::audio::browser_node::{BrowserNode, BrowserNodeKind};
#[cfg(test)]
use crate::db::models::Track;
#[cfg(test)]
use crate::i18n::i18n;

#[cfg(test)]
use super::library_panel::LibraryPanelOutput;

mod interaction;
mod model;
mod presentation;
mod view;

#[cfg(test)]
use gtk::{gio, prelude::*};
#[cfg(test)]
use interaction::{
    library_panel_node_selected_output, library_tree_context_target, library_tree_selection,
    selected_leaf_drag_paths, LibraryTreeContextTarget, LibraryTreeSelection,
};
pub(super) use model::LibraryTreeHandle;
#[cfg(test)]
use model::{
    browser_node_contract_for_tree_node, build_node_recursive, group_divider_letter,
    to_browser_node, BrowserNodeContract, ChildStores, LeafTracks, DIVIDER_THRESHOLD,
};
pub use model::{LibraryTreeNode, LibraryTreeSnapshot};
#[cfg(test)]
use presentation::{
    file_leaf_presentation, library_tree_row_presentation, FileLeafPresentation,
    LibraryTreeAvatarPresentation,
};
pub(super) use view::build_library_tree;

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

    #[cfg(not(miri))]
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
    fn browser_node_contract_for_tree_node_covers_core_kinds_without_gobject() {
        let mut track = Track::new_external_path("/music/song.flac");
        track.duration_secs = 61.0;
        let various_artists_label = i18n("Various Artists");
        let cases = [
            (
                library_tree_node(BrowserNodeKind::All, "All songs", "", 12, ""),
                BrowserNodeContract {
                    kind: BrowserNodeKind::All,
                    label: "All songs".to_string(),
                    key: String::new(),
                    secondary: String::new(),
                    track_count: 12,
                },
            ),
            (
                library_tree_node(BrowserNodeKind::Album, "Debut", "album-key", 9, "Alice"),
                BrowserNodeContract {
                    kind: BrowserNodeKind::Album,
                    label: "Debut".to_string(),
                    key: "album-key".to_string(),
                    secondary: "Alice".to_string(),
                    track_count: 9,
                },
            ),
            (
                library_tree_node(BrowserNodeKind::Year, "ignored", "1997", 6, ""),
                BrowserNodeContract {
                    kind: BrowserNodeKind::Year,
                    label: "1997".to_string(),
                    key: "1997".to_string(),
                    secondary: String::new(),
                    track_count: 6,
                },
            ),
            (
                library_tree_node(
                    BrowserNodeKind::VariousArtists,
                    "ignored",
                    "various-key",
                    5,
                    "",
                ),
                BrowserNodeContract {
                    kind: BrowserNodeKind::VariousArtists,
                    label: various_artists_label,
                    key: "various-key".to_string(),
                    secondary: String::new(),
                    track_count: 5,
                },
            ),
            (
                LibraryTreeNode {
                    kind: BrowserNodeKind::File,
                    label: "Song".to_string(),
                    key: "/music/song.flac".to_string(),
                    track: Some(track),
                    ..Default::default()
                },
                BrowserNodeContract {
                    kind: BrowserNodeKind::File,
                    label: "Song".to_string(),
                    key: "/music/song.flac".to_string(),
                    secondary: big_media_components::time::playback_time(61.0),
                    track_count: 0,
                },
            ),
        ];

        for (source_node, expected_contract) in cases {
            assert_eq!(
                browser_node_contract_for_tree_node(&source_node),
                expected_contract
            );
        }
    }

    #[test]
    fn browser_node_contract_preserves_invalid_year_key_and_file_secondary_override() {
        let invalid_year_node =
            library_tree_node(BrowserNodeKind::Year, "ignored", "unknown", 2, "");
        assert_eq!(
            browser_node_contract_for_tree_node(&invalid_year_node),
            BrowserNodeContract {
                kind: BrowserNodeKind::Year,
                label: String::new(),
                key: "unknown".to_string(),
                secondary: String::new(),
                track_count: 2,
            }
        );
        let zero_year_node = library_tree_node(BrowserNodeKind::Year, "ignored", "0", 1, "");
        assert_eq!(
            browser_node_contract_for_tree_node(&zero_year_node).label,
            String::new()
        );

        let file_node = LibraryTreeNode {
            kind: BrowserNodeKind::File,
            label: "Song".to_string(),
            key: "/music/song.flac".to_string(),
            secondary: "Album\t1:01\tArtist\thash".to_string(),
            track: Some(Track::new_external_path("/music/song.flac")),
            ..Default::default()
        };
        assert_eq!(
            browser_node_contract_for_tree_node(&file_node).secondary,
            "Album\t1:01\tArtist\thash"
        );
    }

    #[cfg(not(miri))]
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

    #[cfg(not(miri))]
    #[test]
    fn year_node_with_invalid_key_preserves_raw_key_and_uses_unknown_label() {
        let source_node = library_tree_node(BrowserNodeKind::Year, "ignored", "unknown", 2, "");
        let browser_node = to_browser_node(&source_node);

        assert_eq!(browser_node.kind(), BrowserNodeKind::Year);
        assert_eq!(browser_node.label(), "");
        assert_eq!(browser_node.key(), "unknown");
        assert_eq!(browser_node.track_count(), 2);
    }

    #[cfg(not(miri))]
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

    #[cfg(not(miri))]
    #[test]
    fn row_presentation_makes_dividers_inert_headings() {
        let divider_node = BrowserNode::divider("B");
        let presentation = library_tree_row_presentation(&divider_node);

        assert_eq!(presentation.label, "B");
        assert!(presentation.is_heading);
        assert!(!presentation.is_file_leaf);
        assert!(!presentation.is_activatable);
        assert!(!presentation.is_selectable);
        assert!(!presentation.show_icon);
        assert!(!presentation.show_tracks_button);
        assert_eq!(presentation.track_count_label, None);
        assert_eq!(presentation.avatar, None);
    }

    #[cfg(not(miri))]
    #[test]
    fn row_presentation_keeps_file_leaves_for_leaf_binder() {
        let file_node = BrowserNode::track_node("Song", "/music/song.flac", 65.0);
        let presentation = library_tree_row_presentation(&file_node);

        assert_eq!(presentation.label, "Song");
        assert_eq!(presentation.icon_name, "audio-x-generic-symbolic");
        assert!(presentation.is_file_leaf);
        assert!(presentation.is_activatable);
        assert!(presentation.is_selectable);
        assert!(presentation.show_icon);
        assert!(!presentation.show_tracks_button);
    }

    #[cfg(not(miri))]
    #[test]
    fn row_presentation_uses_count_badge_and_icon_for_regular_groups() {
        let artist_node = BrowserNode::artist("Alice", 7);
        let presentation = library_tree_row_presentation(&artist_node);

        assert_eq!(presentation.label, "Alice");
        assert_eq!(presentation.icon_name, "avatar-default-symbolic");
        assert_eq!(presentation.track_count_label, Some("7".to_string()));
        assert_eq!(presentation.avatar, None);
        assert!(presentation.show_icon);
        assert!(presentation.show_tracks_button);

        let empty_genre_node = BrowserNode::genre("Ambient", 0);
        let empty_genre_presentation = library_tree_row_presentation(&empty_genre_node);
        assert_eq!(empty_genre_presentation.track_count_label, None);
        assert!(empty_genre_presentation.show_icon);
    }

    #[cfg(not(miri))]
    #[test]
    fn row_presentation_uses_avatar_for_album_and_various_artists() {
        let album_node = BrowserNode::album("Debut", "Alice", 3);
        let album_presentation = library_tree_row_presentation(&album_node);

        assert_eq!(
            album_presentation.avatar,
            Some(LibraryTreeAvatarPresentation {
                text: "Debut".to_string(),
                icon_name: None,
            })
        );
        assert!(!album_presentation.show_icon);
        assert_eq!(album_presentation.track_count_label, Some("3".to_string()));

        let various_artists_node = BrowserNode::various_artists(4);
        let various_artists_presentation = library_tree_row_presentation(&various_artists_node);
        assert_eq!(
            various_artists_presentation.avatar,
            Some(LibraryTreeAvatarPresentation {
                text: i18n("Various Artists"),
                icon_name: Some("system-users-symbolic"),
            })
        );
        assert!(!various_artists_presentation.show_icon);
    }

    #[cfg(not(miri))]
    #[test]
    fn file_leaf_presentation_handles_badge_fallback_and_detailed_metadata() {
        let fallback_node = BrowserNode::file_node("Loose", "/music/loose.flac");
        fallback_node.set_secondary("3:04");
        assert_eq!(
            file_leaf_presentation(&fallback_node, None),
            FileLeafPresentation {
                duration_label: None,
                artist_label: None,
                fallback_badge_label: Some("3:04".to_string()),
                avatar_text: None,
                has_embedded_art: false,
            }
        );

        let empty_secondary_node = BrowserNode::file_node("Plain", "/music/plain.flac");
        assert_eq!(
            file_leaf_presentation(&empty_secondary_node, None),
            FileLeafPresentation {
                duration_label: None,
                artist_label: None,
                fallback_badge_label: None,
                avatar_text: None,
                has_embedded_art: false,
            }
        );

        let detailed_node = BrowserNode::file_node("Detailed", "/music/detailed.flac");
        detailed_node.set_secondary("Album\t4:20\tAlice\tcover-hash");
        let mut track_with_art = Track::new_external_path("/music/detailed.flac");
        track_with_art.art_hash = Some("cover-hash".to_string());
        assert_eq!(
            file_leaf_presentation(&detailed_node, Some(&track_with_art)),
            FileLeafPresentation {
                duration_label: Some("4:20".to_string()),
                artist_label: Some("Alice".to_string()),
                fallback_badge_label: None,
                avatar_text: Some("Alice".to_string()),
                has_embedded_art: true,
            }
        );

        let minimal_detailed_node = BrowserNode::file_node("Short", "/music/short.flac");
        minimal_detailed_node.set_secondary("Album\t0:59");
        assert_eq!(
            file_leaf_presentation(&minimal_detailed_node, None),
            FileLeafPresentation {
                duration_label: Some("0:59".to_string()),
                artist_label: None,
                fallback_badge_label: None,
                avatar_text: Some("Short".to_string()),
                has_embedded_art: false,
            }
        );

        let no_artist_node = BrowserNode::file_node("Instrumental", "/music/instrumental.flac");
        no_artist_node.set_secondary("Album\t2:10\t\t");
        assert_eq!(
            file_leaf_presentation(&no_artist_node, None),
            FileLeafPresentation {
                duration_label: Some("2:10".to_string()),
                artist_label: None,
                fallback_badge_label: None,
                avatar_text: Some("Instrumental".to_string()),
                has_embedded_art: false,
            }
        );
    }

    #[cfg(not(miri))]
    #[test]
    fn selected_leaf_drag_paths_exports_only_registered_file_leaves() {
        let registered_leaf_node = BrowserNode::file_node("Ready", "/music/ready.flac");
        let missing_leaf_node = BrowserNode::file_node("Missing", "/music/missing.flac");
        let artist_node = BrowserNode::artist("Alice", 1);
        let selected_browser_nodes = [registered_leaf_node, missing_leaf_node, artist_node];
        let mut registered_leaf_tracks = HashMap::new();
        registered_leaf_tracks.insert(
            "/music/ready.flac".to_string(),
            Track::new_external_path("/music/ready.flac"),
        );

        assert_eq!(
            selected_leaf_drag_paths(selected_browser_nodes.iter(), &registered_leaf_tracks),
            vec!["/music/ready.flac".to_string()]
        );
    }

    #[cfg(not(miri))]
    #[test]
    fn library_tree_selection_ignores_dividers_and_preserves_node_identity() {
        let divider_node = BrowserNode::divider("C");
        assert_eq!(library_tree_selection(&divider_node, 2), None);

        let album_node = BrowserNode::album("Debut", "Alice", 8);
        assert_eq!(
            library_tree_selection(&album_node, 3),
            Some(LibraryTreeSelection {
                kind: BrowserNodeKind::Album as u32,
                key: "Debut".to_string(),
                secondary: "Alice".to_string(),
                depth: 3,
            })
        );
    }

    #[test]
    fn node_selected_output_preserves_tree_selection_payload() {
        let output = library_panel_node_selected_output(&LibraryTreeSelection {
            kind: BrowserNodeKind::Album as u32,
            key: "Debut".to_string(),
            secondary: "Alice".to_string(),
            depth: 3,
        });

        let LibraryPanelOutput::NodeSelected {
            kind,
            key,
            secondary,
            depth,
        } = output
        else {
            panic!("expected node selection output");
        };
        assert_eq!(kind, BrowserNodeKind::Album as u32);
        assert_eq!(key, "Debut");
        assert_eq!(secondary, "Alice");
        assert_eq!(depth, 3);
    }

    #[cfg(not(miri))]
    #[test]
    fn context_target_distinguishes_inert_files_and_groups() {
        let divider_node = BrowserNode::divider("D");
        let registered_file_node = BrowserNode::file_node("Ready", "/music/ready.flac");
        let missing_file_node = BrowserNode::file_node("Missing", "/music/missing.flac");
        let genre_node = BrowserNode::genre("Jazz", 5);
        let mut registered_leaf_tracks = HashMap::new();
        registered_leaf_tracks.insert(
            "/music/ready.flac".to_string(),
            Track::new_external_path("/music/ready.flac"),
        );

        assert_eq!(
            library_tree_context_target(&divider_node, 0, &registered_leaf_tracks),
            LibraryTreeContextTarget::Inert
        );
        assert_eq!(
            library_tree_context_target(&missing_file_node, 1, &registered_leaf_tracks),
            LibraryTreeContextTarget::Inert
        );
        assert_eq!(
            library_tree_context_target(&registered_file_node, 1, &registered_leaf_tracks),
            LibraryTreeContextTarget::FileLeaf {
                file_path: "/music/ready.flac".to_string(),
            }
        );
        assert_eq!(
            library_tree_context_target(&genre_node, 2, &registered_leaf_tracks),
            LibraryTreeContextTarget::Group {
                selection: LibraryTreeSelection {
                    kind: BrowserNodeKind::Genre as u32,
                    key: "Jazz".to_string(),
                    secondary: String::new(),
                    depth: 2,
                },
            }
        );
    }

    #[cfg(not(miri))]
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

    #[cfg(not(miri))]
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

    #[cfg(not(miri))]
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
