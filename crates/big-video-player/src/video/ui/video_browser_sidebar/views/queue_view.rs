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

    // The picture, its length on it, and how far it was watched under it.
    // The box sets the size; the picture, not measured, fills it.
    let size = gtk::Box::builder()
        .width_request(96)
        .height_request(54)
        .build();
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .build();
    let length = gtk::Label::builder()
        .halign(gtk::Align::End)
        .valign(gtk::Align::End)
        .margin_end(3)
        .margin_bottom(5)
        .visible(false)
        .css_classes(["playlist-badge", "caption"])
        .build();
    let watched = gtk::ProgressBar::builder()
        .valign(gtk::Align::End)
        .visible(false)
        .css_classes(["queue-watched"])
        .build();
    let frame = gtk::Overlay::builder()
        .child(&size)
        .valign(gtk::Align::Center)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["playlist-thumb-frame"])
        .build();
    frame.add_overlay(&picture);
    frame.set_measure_overlay(&picture, false);
    frame.add_overlay(&watched);
    frame.add_overlay(&length);
    row.append(&frame);

    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let title = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .lines(2)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    let detail = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .visible(false)
        .css_classes(["dim-label", "caption"])
        .build();
    text.append(&title);
    text.append(&detail);
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
    state.item_at(idx)
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
    let parts = QueueRow::of(&row);
    let Some(QueueRow {
        pos,
        picture,
        watched,
        length,
        title,
        detail,
    }) = parts
    else {
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
    title.set_label(&entry.display_name);
    tooltip_set(&title, &entry.path);
    length.set_visible(false);
    watched.set_visible(false);
    detail.set_visible(false);
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
        QueueRow {
            pos,
            picture,
            watched,
            length,
            title,
            detail,
        },
    );
}

/// A queue row's parts, found from its widget tree (built by
/// [`build_queue_row`]).
struct QueueRow {
    pos: gtk::Label,
    picture: gtk::Picture,
    watched: gtk::ProgressBar,
    length: gtk::Label,
    title: gtk::Label,
    detail: gtk::Label,
}

impl QueueRow {
    fn of(row: &gtk::Box) -> Option<Self> {
        // handle, position, the picture's overlay, the text.
        let pos = row.first_child()?.next_sibling()?;
        let frame = pos.next_sibling()?.downcast::<gtk::Overlay>().ok()?;
        let text = frame.next_sibling()?;
        let picture = frame.child()?.next_sibling()?;
        let watched = picture.next_sibling()?;
        let length = watched.next_sibling()?;
        let title = text.first_child()?;
        let detail = title.next_sibling()?;
        Some(Self {
            pos: pos.downcast().ok()?,
            picture: picture.downcast().ok()?,
            watched: watched.downcast().ok()?,
            length: length.downcast().ok()?,
            title: title.downcast().ok()?,
            detail: detail.downcast().ok()?,
        })
    }
}

/// Where mpv would resume `path` (seconds), from its watch-later file: the
/// MD5 of the path, under the player's own directory.
fn resume_seconds(path: &str) -> Option<f64> {
    let name =
        glib::compute_checksum_for_string(glib::ChecksumType::Md5, path)?.to_ascii_uppercase();
    let file = glib::user_data_dir()
        .join(crate::config::video::WATCH_LATER_RELATIVE)
        .join(name);
    std::fs::read_to_string(file)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("start=")?.trim().parse::<f64>().ok())
        .filter(|seconds| *seconds > 0.0)
}

/// Load a queue row's picture, length and resume point off-thread, guarded
/// by the bind token.
fn spawn_queue_media(
    path: String,
    token: String,
    row_weak: glib::WeakRef<gtk::Box>,
    parts: QueueRow,
) {
    let token: glib::GString = token.into();
    let path_meta = path.clone();
    let parts = Rc::new(parts);
    glib::spawn_future_local(async move {
        let decoded = gio::spawn_blocking(move || thumbnail::load_thumbnail(&path))
            .await
            .ok()
            .flatten();
        if row_weak.upgrade().map(|r| r.widget_name()) != Some(token.clone()) {
            return;
        }
        if let Some((thumb, image)) = decoded {
            let texture = thumbnail::thumbnail_texture(thumb, image);
            parts.picture.set_paintable(Some(&texture));
        }
        let (meta, resume) = gio::spawn_blocking(move || {
            (
                playlist::probe_metadata_cached(&path_meta),
                resume_seconds(&path_meta),
            )
        })
        .await
        .unwrap_or((None, None));
        if row_weak.upgrade().map(|r| r.widget_name()) != Some(token) {
            return;
        }
        let duration = meta.and_then(|m| m.duration_secs).filter(|d| *d > 0.0);
        if let Some(duration) = duration {
            parts.length.set_label(&format_duration_hms(duration));
            parts.length.set_visible(true);
        }
        if let Some(resume) = resume {
            parts.detail.set_label(
                &crate::i18n::i18n("Stopped at {}")
                    .replace("{}", &big_media_components::time::playback_time(resume)),
            );
            parts.detail.set_visible(true);
            if let Some(duration) = duration {
                parts
                    .watched
                    .set_fraction((resume / duration).clamp(0.0, 1.0));
                parts.watched.set_visible(true);
            }
        }
    });
}
