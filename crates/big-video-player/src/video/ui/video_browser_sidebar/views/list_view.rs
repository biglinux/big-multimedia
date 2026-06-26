use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::super::BrowserState;
use super::model::{build_filter, build_store, finish_view};
use super::shared::{attach_context_menu, set_accent, tooltip_set};
use crate::media::playlist::{self, EntryKind, PlaylistEntry};

/// ListView factory: icon + name + a metadata caption line per row.
pub(crate) fn build_list_view(
    state: &Rc<BrowserState>,
    items: &[String],
    recursive_on: bool,
    folder: Option<&String>,
) -> gtk::ScrolledWindow {
    let (store, folder_count) =
        build_store(items, recursive_on, folder, &state.subfolders.borrow());
    let filter = build_filter(state);
    let model = gtk::FilterListModel::new(Some(store.clone()), Some(filter.clone()));
    let selection = gtk::SingleSelection::builder()
        .model(&model)
        .autoselect(false)
        .can_unselect(true)
        .build();

    let factory = gtk::SignalListItemFactory::new();
    let generation = state.scan_generation.get();

    factory.connect_setup({
        let state = state.clone();
        move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let row = gtk::Box::new(gtk::Orientation::Vertical, 0);

            let hbox = gtk::Box::builder()
                .spacing(8)
                .margin_start(8)
                .margin_end(8)
                .margin_top(6)
                .margin_bottom(2)
                .build();
            let icon = gtk::Image::builder()
                .icon_name("video-x-generic-symbolic")
                .pixel_size(16)
                .build();
            hbox.append(&icon);
            let label = gtk::Label::builder()
                .halign(gtk::Align::Start)
                .hexpand(true)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build();
            hbox.append(&label);
            row.append(&hbox);

            let meta = gtk::Label::builder()
                .halign(gtk::Align::Start)
                .margin_start(32)
                .margin_end(8)
                .margin_bottom(4)
                .css_classes(["dim-label", "caption"])
                .build();
            row.append(&meta);

            attach_context_menu(&state, &row);
            item.set_child(Some(&row));
        }
    });

    factory.connect_bind({
        let state = state.clone();
        move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(obj) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                return;
            };
            let entry: std::cell::Ref<PlaylistEntry> = obj.borrow();
            let Some(row) = item.child().and_downcast::<gtk::Box>() else {
                return;
            };
            let Some(hbox) = row.first_child().and_downcast::<gtk::Box>() else {
                return;
            };
            let Some(icon) = hbox.first_child().and_downcast::<gtk::Image>() else {
                return;
            };
            let Some(label) = icon.next_sibling().and_downcast::<gtk::Label>() else {
                return;
            };
            let Some(meta) = hbox.next_sibling().and_downcast::<gtk::Label>() else {
                return;
            };

            match entry.kind {
                EntryKind::ParentFolder | EntryKind::Folder => {
                    let folder_icon = if entry.kind == EntryKind::ParentFolder {
                        "go-up-symbolic"
                    } else {
                        "folder-symbolic"
                    };
                    icon.set_icon_name(Some(folder_icon));
                    icon.remove_css_class("accent");
                    label.set_label(&entry.display_name);
                    label.remove_css_class("accent");
                    tooltip_set(&label, &entry.path);
                    meta.set_visible(false);
                    row.remove_css_class("playlist-current");
                    row.set_widget_name(&format!("folder:{}", entry.path));
                    row.update_property(&[gtk::accessible::Property::Label(&entry.display_name)]);
                }
                EntryKind::Video => {
                    let is_current = entry.index as i64 == state.pos.get();
                    let selecting = state.select_mode.get();
                    let selected = selecting && state.is_selected(&entry.path);
                    // In select mode the leading icon becomes a checkbox.
                    icon.set_icon_name(Some(if selecting {
                        if selected {
                            "checkbox-checked-symbolic"
                        } else {
                            "checkbox-symbolic"
                        }
                    } else if is_current {
                        "media-playback-start-symbolic"
                    } else {
                        "video-x-generic-symbolic"
                    }));
                    set_accent(&icon, is_current || selected);
                    set_accent(&label, is_current);
                    if is_current {
                        row.add_css_class("playlist-current");
                    } else {
                        row.remove_css_class("playlist-current");
                    }
                    if selected {
                        row.add_css_class("selected");
                    } else {
                        row.remove_css_class("selected");
                    }
                    label.set_label(&entry.display_name);
                    tooltip_set(&label, &entry.path);
                    row.update_property(&[gtk::accessible::Property::Label(&entry.display_name)]);
                    meta.set_visible(true);
                    meta.set_label("");

                    let token = format!("video:{}", entry.index);
                    row.set_widget_name(&token);
                    if state.scan_generation.get() != generation {
                        return;
                    }
                    spawn_meta_probe(entry.path.clone(), token, row.downgrade(), meta.downgrade());
                }
            }
        }
    });

    factory.connect_unbind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        if let Some(row) = item.child().and_downcast::<gtk::Box>() {
            row.set_widget_name("unbound");
        }
    });

    let list = gtk::ListView::new(Some(selection.clone()), Some(factory));
    list.set_single_click_activate(true);
    list.add_css_class("navigation-sidebar");
    list.connect_activate({
        let state = state.clone();
        move |lv, pos| {
            if let Some(model) = lv.model() {
                state.activate(&model, pos);
            }
        }
    });

    let list_weak = list.downgrade();
    finish_view(
        state,
        &list,
        &selection,
        store,
        filter,
        folder_count,
        move |target| {
            if let Some(lv) = list_weak.upgrade() {
                lv.scroll_to(target, gtk::ListScrollFlags::FOCUS, None);
            }
        },
    )
}

/// Probe a video's metadata off-thread; paint the caption only if the row still
/// holds the same bind token.
fn spawn_meta_probe(
    path: String,
    token: String,
    row_weak: glib::WeakRef<gtk::Box>,
    meta_weak: glib::WeakRef<gtk::Label>,
) {
    glib::spawn_future_local(async move {
        let meta = gio::spawn_blocking(move || playlist::probe_metadata_cached(&path))
            .await
            .ok()
            .flatten();
        if row_weak.upgrade().map(|r| r.widget_name()) != Some(token.into()) {
            return;
        }
        if let (Some(meta), Some(label)) = (meta, meta_weak.upgrade()) {
            let summary = playlist::meta_summary(&meta);
            if !summary.is_empty() {
                label.set_label(&summary);
            }
        }
    });
}
