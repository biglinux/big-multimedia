//! The audio library tree: a drawn `ItemView` over a `GtkTreeListModel`.
//!
//! Groups open in place under their expander; a song shows its cover, artist
//! and length. A group's button (and a double click) shows its tracks.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::rc::Rc;

use adw::prelude::*;
use big_item_view::{
    Column, ColumnShows, Geometry, IconFrame, IconSource, ItemContent, ItemPresenter, ItemView,
};
use gtk::{gdk, gio, glib};
use relm4::ComponentSender;

use big_relm4_components::feedback::context_popover::BigContextPopover;

use crate::audio::browser_node::{BrowserNode, BrowserNodeKind};
use crate::db::models::Track;
use crate::i18n::i18n;

use super::super::library_panel::{LibraryPanel, LibraryPanelInput, LibraryPanelOutput};
use super::interaction::{
    LibraryTreeContextTarget, library_panel_node_selected_output, library_tree_context_target,
    library_tree_selection, selected_leaf_drag_paths,
};
use super::model::{ChildStores, LeafTracks, LibraryTreeHandle, child_store, child_store_key};
use super::presentation::{file_leaf_presentation, library_tree_row_presentation};

/// The picture's edge in a row; covers and initials are made at twice that.
const PICTURE_PX: i32 = 32;
/// Column ids: the name, the count or length, the show-tracks button.
const NAME: u32 = 0;
const COUNT: u32 = 1;
const SHOW_TRACKS: u32 = 2;

/// Pictures by cover hash or initials: rows are drawn again on every scroll,
/// and each cover costs an ffmpeg run.
type Pictures = RefCell<lru::LruCache<String, gdk::Texture>>;

/// What the tree's view asks about a node.
struct TreePresenter {
    leaves: LeafTracks,
    pictures: Pictures,
    /// Covers being decoded, so a row drawn again does not start another.
    pending: Rc<RefCell<HashSet<String>>>,
    covers: Rc<Pictures>,
    view: Rc<RefCell<glib::WeakRef<ItemView>>>,
}

impl TreePresenter {
    fn initials(&self, text: &str) -> Option<gdk::Texture> {
        let key = format!("initials:{text}");
        if let Some(texture) = self.pictures.borrow_mut().get(&key) {
            return Some(texture.clone());
        }
        let texture = big_media_components::initials_avatar::initials_texture(text, PICTURE_PX)?;
        self.pictures.borrow_mut().put(key, texture.clone());
        Some(texture)
    }

    /// The cover for `file_path` if decoded; else its decode is started, and
    /// the view is drawn again when it lands.
    fn cover(&self, hash: &str, file_path: &str) -> Option<gdk::Texture> {
        if let Some(texture) = self.covers.borrow_mut().get(hash) {
            return Some(texture.clone());
        }
        if self.pending.borrow_mut().insert(hash.to_owned()) {
            let (hash, wanted) = (hash.to_owned(), file_path.to_owned());
            let (covers, pending, view) = (
                Rc::clone(&self.covers),
                Rc::clone(&self.pending),
                Rc::clone(&self.view),
            );
            big_media_components::player_chrome::spawn_blocking_result(
                move || {
                    let data = crate::library::metadata::extract_album_art(&wanted)?;
                    big_media_components::media::images::bytes_to_thumbnail(&data, 2 * PICTURE_PX)
                },
                move |texture| {
                    pending.borrow_mut().remove(&hash);
                    if let Some(texture) = texture {
                        covers.borrow_mut().put(hash, texture);
                        if let Some(view) = view.borrow().upgrade() {
                            view.refresh_all();
                        }
                    }
                },
            );
        }
        None
    }
}

impl ItemPresenter for TreePresenter {
    fn content(&self, _position: u32, item: &glib::Object, out: &mut ItemContent) {
        let Some(node) = item.downcast_ref::<BrowserNode>() else {
            return;
        };
        let row = library_tree_row_presentation(node);
        out.title.clone_from(&row.label);
        out.hides_button = !row.show_tracks_button;
        if row.is_heading {
            // A letter over the names under it.
            out.emphasized = true;
            return;
        }
        let themed = || IconSource::Gicon(gio::ThemedIcon::new(&row.icon_name).upcast());
        let mut said = Vec::new();
        if row.is_file_leaf {
            let file_path = node.key();
            let (leaf, art_hash) = {
                let leaves = self.leaves.borrow();
                let track = leaves.get(&file_path);
                (
                    file_leaf_presentation(node, track),
                    track.and_then(|track| track.art_hash.clone()),
                )
            };
            out.icon = themed();
            out.frame = IconFrame::Small;
            if let Some(badge) = leaf.fallback_badge_label {
                out.cells = vec![badge];
                return;
            }
            let Some(duration) = leaf.duration_label else {
                return;
            };
            let picture = art_hash
                .filter(|hash| leaf.has_embedded_art && !hash.is_empty())
                .and_then(|hash| self.cover(&hash, &file_path))
                .or_else(|| self.initials(leaf.avatar_text.as_deref()?));
            if let Some(texture) = picture {
                out.icon = IconSource::Texture(texture);
                out.frame = IconFrame::Cover;
            }
            if let Some(artist) = leaf.artist_label {
                said.push(artist.clone());
                out.subtitle = Some(artist);
            }
            said.push(duration.clone());
            out.cells = vec![duration];
        } else {
            let initials = row
                .avatar
                .as_ref()
                .filter(|avatar| avatar.icon_name.is_none())
                .and_then(|avatar| self.initials(&avatar.text));
            let named = row.avatar.as_ref().and_then(|avatar| avatar.icon_name);
            (out.icon, out.frame) = match (initials, named) {
                (Some(texture), _) => (IconSource::Texture(texture), IconFrame::Cover),
                (None, Some(name)) => (
                    IconSource::Gicon(gio::ThemedIcon::new(name).upcast()),
                    IconFrame::Small,
                ),
                (None, None) => (themed(), IconFrame::Small),
            };
            if let Some(count) = row.track_count_label {
                let unit = if node.track_count() == 1 {
                    i18n("track")
                } else {
                    i18n("tracks")
                };
                said.push(format!("{count} {unit}"));
                out.cells = vec![count];
            }
        }
        out.accessible_description = (!said.is_empty()).then(|| said.join(", "));
    }

    fn tooltip(&self, _position: u32, item: &glib::Object) -> Option<String> {
        let node = item.downcast_ref::<BrowserNode>()?;
        (node.kind() != BrowserNodeKind::Divider).then(|| node.label())
    }

    fn drag_content(&self, positions: &[u32]) -> Option<gdk::ContentProvider> {
        let view = self.view.borrow().upgrade()?;
        let model = view.model()?;
        let nodes: Vec<BrowserNode> = positions
            .iter()
            .filter_map(|position| model.item(*position).and_downcast::<gtk::TreeListRow>())
            .filter_map(|row| row.item().and_downcast::<BrowserNode>())
            .collect();
        let paths = selected_leaf_drag_paths(nodes.iter(), &self.leaves.borrow());
        (!paths.is_empty()).then(|| gdk::ContentProvider::for_value(&paths.join("\n").to_value()))
    }
}

/// Build the library tree widget. Returns the view (the panel mounts it in its
/// left-pane `gtk::ScrolledWindow`) and a [`LibraryTreeHandle`] the panel keeps
/// to apply `SetLibraryTree`.
pub(in crate::audio::ui) fn build_library_tree(
    sender: &ComponentSender<LibraryPanel>,
) -> (ItemView, LibraryTreeHandle) {
    let root_store = gio::ListStore::new::<BrowserNode>();
    let child_stores: ChildStores = Rc::new(RefCell::new(HashMap::new()));
    let leaf_tracks: LeafTracks = Rc::new(RefCell::new(HashMap::new()));

    let tree_model = build_tree_model(&root_store, &child_stores, &leaf_tracks);
    let selection = gtk::MultiSelection::new(Some(tree_model));
    let shown: Rc<RefCell<glib::WeakRef<ItemView>>> = Rc::default();
    let lru = |n| {
        RefCell::new(lru::LruCache::new(
            NonZeroUsize::new(n).unwrap_or(NonZeroUsize::MIN),
        ))
    };
    let presenter = Rc::new(TreePresenter {
        leaves: Rc::clone(&leaf_tracks),
        pictures: lru(256),
        pending: Rc::default(),
        covers: Rc::new(lru(256)),
        view: Rc::clone(&shown),
    });
    let view = ItemView::new(presenter, tree_geometry());
    shown.replace(view.downgrade());
    view.set_model(Some(&selection));
    view.set_vexpand(true);
    view.add_css_class("navigation-sidebar");
    view.update_property(&[gtk::accessible::Property::Label(&i18n("Music library"))]);

    wire_activation(&view, sender);
    wire_context_menu(&view, &leaf_tracks, sender);

    let handle = LibraryTreeHandle {
        root_store,
        child_stores,
        leaf_tracks,
    };
    (view, handle)
}

/// A row per node: its picture and name, its count or length, and the button
/// that shows a group's tracks.
fn tree_geometry() -> Geometry {
    let mut geometry = Geometry::rows(PICTURE_PX);
    geometry.subtitle = true;
    geometry.columns = vec![
        Column {
            id: NAME,
            title: String::new(),
            shows: ColumnShows::Primary,
            width: 120,
            expand: true,
            xalign: 0.0,
            sort: None,
        },
        Column {
            id: COUNT,
            title: String::new(),
            shows: ColumnShows::Cell(0),
            width: 56,
            expand: false,
            xalign: 1.0,
            sort: None,
        },
        Column {
            id: SHOW_TRACKS,
            title: String::new(),
            shows: ColumnShows::Button("go-next-symbolic"),
            width: 32,
            expand: false,
            xalign: 0.5,
            sort: None,
        },
    ];
    geometry
}

/// `TreeListModel` whose create-func mounts the preloaded children of a row
/// (synchronous HashMap lookup, built on first expand; `None` for
/// leaves/dividers).
fn build_tree_model(
    root_store: &gio::ListStore,
    child_stores: &ChildStores,
    leaf_tracks: &LeafTracks,
) -> gtk::TreeListModel {
    let stores = child_stores.clone();
    let leaves = leaf_tracks.clone();
    gtk::TreeListModel::new(root_store.clone(), false, false, move |model_item| {
        let node = model_item.downcast_ref::<BrowserNode>()?;
        if matches!(
            node.kind(),
            BrowserNodeKind::Divider | BrowserNodeKind::File
        ) {
            return None;
        }
        let store = child_store(
            &child_store_key(node.kind(), &node.key()),
            &mut stores.borrow_mut(),
            &mut leaves.borrow_mut(),
        )?;
        Some(store.upcast::<gio::ListModel>())
    })
}

/// The tree row and node at `position`.
fn node_at(view: &ItemView, position: u32) -> Option<(gtk::TreeListRow, BrowserNode)> {
    let row = view
        .model()?
        .item(position)
        .and_downcast::<gtk::TreeListRow>()?;
    let node = row.item().and_downcast::<BrowserNode>()?;
    Some((row, node))
}

/// Activation (double click, Enter) and a group's button → `NodeSelected`:
/// the shell queries its tracks → `PopulateDetail`. Dividers are inert.
fn wire_activation(view: &ItemView, sender: &ComponentSender<LibraryPanel>) {
    let select = {
        let out = sender.clone();
        move |view: &ItemView, position: u32| {
            if let Some((row, node)) = node_at(view, position)
                && let Some(selection) = library_tree_selection(&node, row.depth())
            {
                let _ = out.output(library_panel_node_selected_output(&selection));
            }
        }
    };
    let on_activate = select.clone();
    view.connect_activate(move |view, position| on_activate(view, position));
    view.connect_cell_pressed(move |view, position, column, _| {
        if column == SHOW_TRACKS {
            select(view, position);
        }
    });
}

/// Right-click context menu on the tree: group nodes get Play (enqueue all);
/// file leaves get the full track menu.
fn wire_context_menu(
    view: &ItemView,
    leaf_tracks: &LeafTracks,
    sender: &ComponentSender<LibraryPanel>,
) {
    let leaves = leaf_tracks.clone();
    let out = sender.clone();
    view.connect_context_menu(move |view, position, x, y| {
        let Some((row, node)) = node_at(view, position) else {
            return;
        };
        let context_target = {
            let registered_leaf_tracks = leaves.borrow();
            library_tree_context_target(&node, row.depth(), &registered_leaf_tracks)
        };
        let menu = BigContextPopover::new(false);
        match context_target {
            LibraryTreeContextTarget::Inert => return,
            LibraryTreeContextTarget::FileLeaf { file_path } => {
                let Some(track) = leaves.borrow().get(&file_path).cloned() else {
                    return;
                };
                add_leaf_menu_items(&menu, &track, &out);
            }
            LibraryTreeContextTarget::Group { selection } => {
                // Group node: a single Play that enqueues all of its tracks.
                let output_sender = out.clone();
                menu.append_button(i18n("Play"), &[], move || {
                    let _ = output_sender.output(library_panel_node_selected_output(&selection));
                });
            }
        }
        menu.popup_at(view, x, y);
    });
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
