use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use super::super::{BrowserState, chrome};
use super::model::{build_filter, build_store, finish_view};
use super::shared::{set_accent, tooltip_set};
use crate::media::playlist::{self, EntryKind, PlaylistEntry};
use crate::media::thumbnail;
use big_media_components::time::duration_hms as format_duration_hms;

/// Playlist (queue) view: a compact ordered list distinct from the Files
/// browser — position number, small thumbnail, title + duration, and an
/// always-visible remove button; the playing entry is marked with a ▶ and an
/// accent. No folders, chips, or select mode.
pub(crate) fn build_queue_view(state: &Rc<BrowserState>, items: &[String]) -> gtk::ScrolledWindow {
    let (store, folder_count) = build_store(items, false, None, &[]);
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
            item.set_child(Some(&build_queue_row(&state)));
        }
    });
    factory.connect_bind({
        let state = state.clone();
        move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            bind_queue_row(&state, item, generation);
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
    // Alt+Up / Alt+Down move the focused row one place. Capture phase: the
    // list's own row keeps plain arrow keys, and took these too.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    {
        let state = state.clone();
        let list = list.downgrade();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            use gtk::gdk::Key;
            let step: isize = match key {
                Key::Up if modifiers.contains(gtk::gdk::ModifierType::ALT_MASK) => -1,
                Key::Down if modifiers.contains(gtk::gdk::ModifierType::ALT_MASK) => 1,
                _ => return glib::Propagation::Proceed,
            };
            let Some(from) = list
                .upgrade()
                .and_then(|list| list.focus_child())
                .and_then(|item| item.first_child())
                .and_downcast::<gtk::Box>()
                .and_then(|row| queue_index_of(&row))
            else {
                return glib::Propagation::Proceed;
            };
            let Some(to) = from.checked_add_signed(step) else {
                return glib::Propagation::Stop;
            };
            state.move_in_queue(from, to);
            if let Some(list) = list.upgrade() {
                list.scroll_to(
                    u32::try_from(to).unwrap_or(0),
                    gtk::ListScrollFlags::FOCUS,
                    None,
                );
            }
            glib::Propagation::Stop
        });
    }
    list.add_controller(keys);
    list.add_css_class("navigation-sidebar");
    list.add_css_class("queue-list");
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

/// Build an empty queue row: position, thumbnail, title+duration, remove button.
fn build_queue_row(state: &Rc<BrowserState>) -> gtk::Box {
    let row = gtk::Box::builder()
        .spacing(8)
        .margin_start(8)
        .margin_end(8)
        .margin_top(3)
        .margin_bottom(3)
        .build();

    // Drag it by this to another place in the list.
    let handle = gtk::Image::builder()
        .icon_name("list-drag-handle-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(["dim-label", "queue-handle"])
        .build();
    handle.set_cursor_from_name(Some("grab"));
    tooltip_set(&handle, &crate::i18n::i18n("Drag to change the order"));
    row.append(&handle);

    let pos = gtk::Label::builder()
        .width_request(20)
        .valign(gtk::Align::Center)
        .css_classes(["dim-label", "caption", "queue-pos"])
        .build();
    pos.set_widget_name("pos");
    row.append(&pos);

    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .build();
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&picture));
    frame.add_css_class("playlist-thumb-frame");
    frame.set_overflow(gtk::Overflow::Hidden);
    frame.set_size_request(56, 32);
    frame.set_valign(gtk::Align::Center);
    row.append(&frame);

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let title = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    title.set_widget_name("title");
    let dur = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .css_classes(["dim-label", "caption"])
        .build();
    dur.set_widget_name("dur");
    text.append(&title);
    text.append(&dur);
    row.append(&text);

    let remove = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(["flat", "circular", "queue-remove"])
        .build();
    tooltip_set(&remove, &crate::i18n::i18n("Remove from playlist"));
    remove.connect_clicked({
        let state = state.clone();
        let row_weak = row.downgrade();
        move |_| {
            let Some(row) = row_weak.upgrade() else {
                return;
            };
            if let Some(path) = queue_path_of(&state, &row) {
                chrome::remove_from_queue(&state, &path);
            }
        }
    });
    row.append(&remove);
    wire_reorder(state, &row);
    row
}

/// The row's queue position, from its `q:{index}` widget name.
fn queue_index_of(row: &gtk::Box) -> Option<usize> {
    row.widget_name().strip_prefix("q:")?.parse().ok()
}

/// Drag the row onto another to put it there: above that row when dropped
/// on its upper half, below on its lower half.
fn wire_reorder(state: &Rc<BrowserState>, row: &gtk::Box) {
    let drag = gtk::DragSource::builder()
        .actions(gtk::gdk::DragAction::MOVE)
        .build();
    {
        let row = row.downgrade();
        drag.connect_prepare(move |_, _, _| {
            let index = queue_index_of(&row.upgrade()?)?;
            Some(gtk::gdk::ContentProvider::for_value(
                &(index as u64).to_value(),
            ))
        });
    }
    row.add_controller(drag);

    let drop = gtk::DropTarget::new(u64::static_type(), gtk::gdk::DragAction::MOVE);
    {
        let state = state.clone();
        let row = row.downgrade();
        drop.connect_drop(move |_, value, _, y| {
            let (Some(row), Ok(from)) = (row.upgrade(), value.get::<u64>()) else {
                return false;
            };
            let Some(onto) = queue_index_of(&row) else {
                return false;
            };
            let from = from as usize;
            let below = y > f64::from(row.height()) / 2.0;
            let mut to = onto + usize::from(below);
            if from < to {
                to -= 1;
            }
            state.move_in_queue(from, to);
            true
        });
    }
    row.add_controller(drop);
}

/// Resolve a queue row's path from its `q:{index}` widget name.
fn queue_path_of(state: &Rc<BrowserState>, row: &gtk::Box) -> Option<String> {
    let name = row.widget_name();
    let idx = name.strip_prefix("q:")?.parse::<usize>().ok()?;
    state.active_items().get(idx).cloned()
}

fn bind_queue_row(state: &Rc<BrowserState>, queue_cell: &gtk::ListItem, generation: u64) {
    let Some(obj) = queue_cell.item().and_downcast::<glib::BoxedAnyObject>() else {
        return;
    };
    let entry: std::cell::Ref<PlaylistEntry> = obj.borrow();
    if entry.kind != EntryKind::Video {
        return;
    }
    let Some(row) = queue_cell.child().and_downcast::<gtk::Box>() else {
        return;
    };
    // First the drag handle, then the position.
    let Some(pos) = row
        .first_child()
        .and_then(|handle| handle.next_sibling())
        .and_downcast::<gtk::Label>()
    else {
        return;
    };
    let Some(frame) = pos.next_sibling().and_downcast::<gtk::Frame>() else {
        return;
    };
    let Some(picture) = frame.child().and_downcast::<gtk::Picture>() else {
        return;
    };
    let Some(text) = frame.next_sibling().and_downcast::<gtk::Box>() else {
        return;
    };
    let Some(title) = text.first_child().and_downcast::<gtk::Label>() else {
        return;
    };
    let Some(dur) = title.next_sibling().and_downcast::<gtk::Label>() else {
        return;
    };

    let is_current = entry.index as i64 == state.pos.get();
    if is_current {
        pos.set_label("▶");
        row.add_css_class("playlist-current");
    } else {
        pos.set_label(&(entry.index + 1).to_string());
        row.remove_css_class("playlist-current");
    }
    set_accent(&pos, is_current);
    set_accent(&title, is_current);
    title.set_label(&entry.display_name);
    tooltip_set(&title, &entry.path);
    dur.set_label("");
    picture.set_paintable(gdk::Paintable::NONE);

    let token = format!("q:{}", entry.index);
    row.set_widget_name(&token);
    if state.scan_generation.get() != generation {
        return;
    }
    spawn_queue_media(
        entry.path.clone(),
        token,
        row.downgrade(),
        picture.downgrade(),
        dur.downgrade(),
    );
}

/// Load a queue row's thumbnail + duration off-thread, guarded by the bind token.
fn spawn_queue_media(
    path: String,
    token: String,
    row_weak: glib::WeakRef<gtk::Box>,
    pic_weak: glib::WeakRef<gtk::Picture>,
    dur_weak: glib::WeakRef<gtk::Label>,
) {
    let token: glib::GString = token.into();
    let path_meta = path.clone();
    glib::spawn_future_local(async move {
        let decoded = gio::spawn_blocking(move || {
            let thumb = thumbnail::get_or_generate_thumbnail(&path)?;
            let texture = gdk::Texture::from_filename(&thumb).ok()?;
            Some((thumb, texture))
        })
        .await
        .ok()
        .flatten();
        if row_weak.upgrade().map(|r| r.widget_name()) != Some(token.clone()) {
            return;
        }
        if let (Some((thumb, texture)), Some(picture)) = (decoded, pic_weak.upgrade()) {
            let texture = thumbnail::intern_thumbnail_texture(thumb, texture);
            picture.set_paintable(Some(&texture));
        }
        let meta = gio::spawn_blocking(move || playlist::probe_metadata_cached(&path_meta))
            .await
            .ok()
            .flatten();
        if row_weak.upgrade().map(|r| r.widget_name()) != Some(token) {
            return;
        }
        if let Some(dur) = meta.and_then(|m| m.duration_secs).filter(|d| *d > 0.0)
            && let Some(label) = dur_weak.upgrade()
        {
            label.set_label(&format_duration_hms(dur));
        }
    });
}
