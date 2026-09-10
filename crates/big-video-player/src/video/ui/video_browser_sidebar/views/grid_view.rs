use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use super::super::BrowserState;
use super::model::{build_filter, build_store, finish_view};
use super::shared::{attach_context_menu, centered_icon, set_accent, tooltip_set};
use crate::media::playlist::{self, EntryKind, PlaylistEntry};
use crate::media::thumbnail;
use big_media_components::time::duration_hms as format_duration_hms;

/// GridView factory: thumbnail cards with duration badge + play overlay.
pub(crate) fn build_grid_view(
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
            item.set_child(Some(&build_card(&state)));
        }
    });
    factory.connect_bind({
        let state = state.clone();
        move |_, item| {
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            bind_card(&state, item, generation);
        }
    });
    factory.connect_unbind(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        if let Some(card) = item.child().and_downcast::<gtk::Box>() {
            card.set_widget_name("unbound");
        }
    });

    let grid = gtk::GridView::new(Some(selection.clone()), Some(factory));
    grid.set_min_columns(2);
    grid.set_max_columns(3);
    grid.set_single_click_activate(true);
    grid.add_css_class("playlist-grid");
    grid.connect_activate({
        let state = state.clone();
        move |gv, pos| {
            if let Some(model) = gv.model() {
                state.activate(&model, pos);
            }
        }
    });

    let grid_weak = grid.downgrade();
    finish_view(
        state,
        &grid,
        &selection,
        store,
        filter,
        folder_count,
        move |target| {
            if let Some(gv) = grid_weak.upgrade() {
                gv.scroll_to(target, gtk::ListScrollFlags::FOCUS, None);
            }
        },
    )
}

/// Widgets pulled back out of a recycled grid card for (re)binding.
struct CardWidgets {
    card: gtk::Box,
    picture: gtk::Picture,
    frame: gtk::Frame,
    placeholder: gtk::Image,
    badge: gtk::Label,
    play_icon: gtk::Image,
    folder_icon: gtk::Image,
    select_check: gtk::Image,
    name: gtk::Label,
}

/// Build an empty grid card (overlay: frame+picture, placeholder, badge, play,
/// folder icon; name label below).
fn build_card(state: &Rc<BrowserState>) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("playlist-card");

    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .build();
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&picture));
    frame.add_css_class("playlist-thumb-frame");
    frame.set_overflow(gtk::Overflow::Hidden);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&frame));
    // ~16:9 so previews fill the card without the old short letterbox crop;
    // grows with the (resizable) sidebar width.
    overlay.set_height_request(132);

    let placeholder = centered_icon("video-x-generic-symbolic", 32);
    placeholder.set_widget_name("placeholder-icon");
    overlay.add_overlay(&placeholder);

    let badge = gtk::Label::builder()
        .halign(gtk::Align::End)
        .valign(gtk::Align::End)
        .margin_end(4)
        .margin_bottom(4)
        .visible(false)
        .css_classes(["caption", "playlist-badge"])
        .build();
    badge.set_widget_name("badge");
    overlay.add_overlay(&badge);

    let play_icon = centered_icon("media-playback-start-symbolic", 28);
    play_icon.set_visible(false);
    play_icon.add_css_class("card-play-icon");
    play_icon.set_widget_name("play-icon");
    overlay.add_overlay(&play_icon);

    let folder_icon = gtk::Image::builder()
        .pixel_size(48)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .visible(false)
        .build();
    folder_icon.set_widget_name("folder-icon");
    overlay.add_overlay(&folder_icon);

    // Triage select indicator (top-left), shown only in select mode.
    let select_check = gtk::Image::builder()
        .pixel_size(20)
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_start(4)
        .margin_top(4)
        .visible(false)
        .build();
    select_check.add_css_class("card-select-check");
    select_check.set_widget_name("select-check");
    overlay.add_overlay(&select_check);

    card.append(&overlay);

    let name = gtk::Label::builder()
        .halign(gtk::Align::Start)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .lines(2)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::WordChar)
        .css_classes(["caption"])
        .build();
    name.set_widget_name("name");
    card.append(&name);

    attach_context_menu(state, &card);
    super::super::scrub::wire_card_hover(state, &card, &picture);
    card
}

fn extract_card(grid_cell: &gtk::ListItem) -> Option<CardWidgets> {
    let card = grid_cell.child().and_downcast::<gtk::Box>()?;
    let overlay = card.first_child().and_downcast::<gtk::Overlay>()?;
    let frame = overlay.child().and_downcast::<gtk::Frame>()?;
    let picture = frame.child().and_downcast::<gtk::Picture>()?;

    let mut placeholder = None;
    let mut badge = None;
    let mut play_icon = None;
    let mut folder_icon = None;
    let mut select_check = None;
    let mut child = overlay.first_child();
    while let Some(w) = child {
        match w.widget_name().as_str() {
            "placeholder-icon" => placeholder = w.clone().downcast::<gtk::Image>().ok(),
            "badge" => badge = w.clone().downcast::<gtk::Label>().ok(),
            "play-icon" => play_icon = w.clone().downcast::<gtk::Image>().ok(),
            "folder-icon" => folder_icon = w.clone().downcast::<gtk::Image>().ok(),
            "select-check" => select_check = w.clone().downcast::<gtk::Image>().ok(),
            _ => {}
        }
        child = w.next_sibling();
    }
    let name = overlay.next_sibling().and_downcast::<gtk::Label>()?;

    Some(CardWidgets {
        card,
        picture,
        frame,
        placeholder: placeholder?,
        badge: badge?,
        play_icon: play_icon?,
        folder_icon: folder_icon?,
        select_check: select_check?,
        name,
    })
}

fn bind_card(state: &Rc<BrowserState>, grid_cell: &gtk::ListItem, generation: u64) {
    let Some(obj) = grid_cell.item().and_downcast::<glib::BoxedAnyObject>() else {
        return;
    };
    let entry: std::cell::Ref<PlaylistEntry> = obj.borrow();
    let Some(w) = extract_card(grid_cell) else {
        return;
    };

    match entry.kind {
        EntryKind::ParentFolder => bind_folder_card(&w, &entry, "go-up-symbolic"),
        EntryKind::Folder => bind_folder_card(&w, &entry, "folder-symbolic"),
        EntryKind::Video => bind_video_card(state, &w, &entry, generation),
    }
}

fn bind_folder_card(w: &CardWidgets, entry: &PlaylistEntry, icon: &str) {
    w.picture.set_paintable(gdk::Paintable::NONE);
    w.picture.set_visible(false);
    w.frame.set_visible(true);
    w.frame.remove_css_class("accent");
    w.placeholder.set_visible(false);
    w.folder_icon.set_icon_name(Some(icon));
    w.folder_icon.set_visible(true);
    w.play_icon.set_visible(false);
    w.badge.set_visible(false);
    w.select_check.set_visible(false);
    w.name.set_label(&entry.display_name);
    tooltip_set(&w.name, &entry.path);
    w.name.remove_css_class("accent");
    w.card.remove_css_class("playlist-card-active");
    w.card.remove_css_class("selected");
    w.card.set_widget_name(&format!("folder:{}", entry.path));
    w.card
        .update_property(&[gtk::accessible::Property::Label(&entry.display_name)]);
}

fn bind_video_card(
    state: &Rc<BrowserState>,
    w: &CardWidgets,
    entry: &PlaylistEntry,
    generation: u64,
) {
    let is_current = entry.index as i64 == state.pos.get();
    w.picture.set_paintable(gdk::Paintable::NONE);
    w.picture.set_visible(true);
    w.frame.set_visible(true);
    w.placeholder.set_visible(true);
    w.folder_icon.set_visible(false);
    w.play_icon.set_visible(is_current);
    set_accent(&w.play_icon, is_current);
    set_accent(&w.frame, is_current);
    set_accent(&w.name, is_current);
    if is_current {
        w.card.add_css_class("playlist-card-active");
    } else {
        w.card.remove_css_class("playlist-card-active");
    }
    w.badge.set_visible(false);
    w.badge.set_label("");
    w.name.set_label(&entry.display_name);
    tooltip_set(&w.name, &entry.path);

    // Triage select indicator (Files select mode only).
    if state.select_mode.get() {
        let selected = state.is_selected(&entry.path);
        w.select_check.set_icon_name(Some(if selected {
            "checkbox-checked-symbolic"
        } else {
            "checkbox-symbolic"
        }));
        w.select_check.set_visible(true);
        if selected {
            w.card.add_css_class("selected");
        } else {
            w.card.remove_css_class("selected");
        }
    } else {
        w.select_check.set_visible(false);
        w.card.remove_css_class("selected");
    }

    let token = format!("video:{}", entry.index);
    w.card.set_widget_name(&token);
    w.card
        .update_property(&[gtk::accessible::Property::Label(&entry.display_name)]);

    if state.scan_generation.get() != generation {
        return;
    }

    // Thumbnail + duration badge are filled off-thread (no main-loop file I/O
    // at bind time, even for the first paint of a cold cache).
    spawn_thumbnail_load(
        entry.path.clone(),
        token,
        w.card.downgrade(),
        w.picture.downgrade(),
        w.placeholder.downgrade(),
        w.badge.downgrade(),
    );
}

/// Load a card's thumbnail (then metadata) off-thread, dropping the result if
/// the card was recycled for a different entry meanwhile.
fn spawn_thumbnail_load(
    path: String,
    token: String,
    card_weak: glib::WeakRef<gtk::Box>,
    pic_weak: glib::WeakRef<gtk::Picture>,
    placeholder_weak: glib::WeakRef<gtk::Image>,
    badge_weak: glib::WeakRef<gtk::Label>,
) {
    let token_quark: glib::GString = token.into();
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

        if card_weak.upgrade().map(|c| c.widget_name()) != Some(token_quark.clone()) {
            return;
        }
        if let (Some((thumb, texture)), Some(picture)) = (decoded, pic_weak.upgrade()) {
            let texture = thumbnail::intern_thumbnail_texture(thumb, texture);
            picture.set_paintable(Some(&texture));
            if let Some(ph) = placeholder_weak.upgrade() {
                ph.set_visible(false);
            }
        }

        let meta = gio::spawn_blocking(move || playlist::probe_metadata_cached(&path_meta))
            .await
            .ok()
            .flatten();
        if card_weak.upgrade().map(|c| c.widget_name()) != Some(token_quark) {
            return;
        }
        if let Some(dur) = meta.and_then(|m| m.duration_secs).filter(|d| *d > 0.0)
            && let Some(badge) = badge_weak.upgrade()
        {
            badge.set_label(&format_duration_hms(dur));
            badge.set_visible(true);
        }
    });
}
