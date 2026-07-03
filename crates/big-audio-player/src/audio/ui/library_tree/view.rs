//! GTK view and wiring for the audio library tree.

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

use super::super::library_panel::{LibraryPanel, LibraryPanelInput, LibraryPanelOutput};
use super::super::playlist_geom::find_clicked_position;
use super::interaction::{
    library_panel_node_selected_output, library_tree_context_target, library_tree_selection,
    selected_leaf_drag_paths, LibraryTreeContextTarget,
};
use super::model::{ChildStores, LeafTracks, LibraryTreeHandle};
use super::presentation::{file_leaf_presentation, library_tree_row_presentation};

/// Album-art thumbnail edge (px) for file leaves.
const LEAF_ART_PX: i32 = 48;

/// Build the library tree widget. Returns the `ListView` (the panel mounts it
/// into its left-pane `gtk::ScrolledWindow` via `set_child`) and a
/// [`LibraryTreeHandle`] the panel keeps to apply `SetLibraryTree`.
pub(in crate::audio::ui) fn build_library_tree(
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

    let row_presentation = library_tree_row_presentation(&node);
    icon.set_visible(row_presentation.show_icon);
    avatar.set_visible(row_presentation.avatar.is_some());
    label.set_label(&row_presentation.label);
    list_item.set_activatable(row_presentation.is_activatable);
    list_item.set_selectable(row_presentation.is_selectable);
    show_tracks_button.set_visible(row_presentation.show_tracks_button);
    if row_presentation.is_heading {
        label.add_css_class("heading");
    } else {
        label.remove_css_class("heading");
    }

    if row_presentation.is_heading {
        artist_label.set_visible(false);
        track_count_label.set_visible(false);
        return;
    }

    icon.set_icon_name(Some(&row_presentation.icon_name));
    if row_presentation.is_file_leaf {
        bind_file_leaf(
            &node,
            &icon,
            &avatar,
            &label,
            &artist_label,
            &track_count_label,
            leaves,
        );
        return;
    }

    artist_label.set_visible(false);
    label.remove_css_class("browser-title");
    big_relm4_components::feedback::tooltip::set(&text_box, node.label());

    if let Some(track_count_label_text) = &row_presentation.track_count_label {
        track_count_label.set_visible(true);
        track_count_label.set_label(track_count_label_text);
    } else {
        track_count_label.set_visible(false);
        track_count_label.set_label("");
    }

    if let Some(avatar_presentation) = &row_presentation.avatar {
        avatar.set_text(Some(&avatar_presentation.text));
        avatar.set_icon_name(avatar_presentation.icon_name);
        avatar.set_custom_image(None::<&gdk::Paintable>);
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
    let Some(selection) = library_tree_selection(node, depth) else {
        button.set_visible(false);
        return;
    };
    let click_signal_id = button.connect_clicked(move |_| {
        let _ = out.output(library_panel_node_selected_output(&selection));
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
    let file_path = node.key();
    let presentation = {
        let registered_leaf_tracks = leaves.borrow();
        file_leaf_presentation(node, registered_leaf_tracks.get(&file_path))
    };
    if let Some(fallback_badge_label) = &presentation.fallback_badge_label {
        avatar.set_visible(false);
        icon.set_visible(true);
        artist_label.set_visible(false);
        track_count_label.set_visible(true);
        track_count_label.set_label(fallback_badge_label);
        return;
    }
    if presentation.duration_label.is_none() {
        avatar.set_visible(false);
        icon.set_visible(true);
        artist_label.set_visible(false);
        track_count_label.set_visible(false);
        track_count_label.set_label("");
        return;
    }

    track_count_label.set_label(presentation.duration_label.as_deref().unwrap_or(""));
    track_count_label.set_visible(true);
    track_count_label.add_css_class("browser-duration");
    avatar.set_visible(true);
    icon.set_visible(false);
    label.add_css_class("browser-title");
    if let Some(artist_label_text) = &presentation.artist_label {
        artist_label.set_label(artist_label_text);
        artist_label.set_visible(true);
    } else {
        artist_label.set_visible(false);
    }
    avatar.set_text(presentation.avatar_text.as_deref());
    avatar.set_custom_image(None::<&gdk::Paintable>);

    // Resolve the source path from the leaf-track map (set on splice) and load
    // the embedded thumbnail off-thread. No DB hit — file IO only.
    if presentation.has_embedded_art {
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
        let Some(selection) = library_tree_selection(&node, tree_row.depth()) else {
            return;
        };
        let _ = out.output(library_panel_node_selected_output(&selection));
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
        let context_target = {
            let registered_leaf_tracks = leaves.borrow();
            library_tree_context_target(&node, tree_row.depth(), &registered_leaf_tracks)
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
        let registered_leaf_tracks = leaves.borrow();
        let mut selected_browser_nodes = Vec::with_capacity(bitset.size() as usize);
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
            selected_browser_nodes.push(node);
        }
        let paths =
            selected_leaf_drag_paths(selected_browser_nodes.iter(), &registered_leaf_tracks);
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
