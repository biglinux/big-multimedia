//! Virtualized media list shared by the radio and podcast panels.
//!
//! Uses the GTK4 recycling view (the same idiom as the big-shell menu's
//! `AppView`) so the panels never build hundreds of row widgets up front or
//! fetch artwork for rows that are off screen: a `GListModel` of lightweight
//! `MediaListRowModel` row models feeds a `ListView` through a
//! `SignalListItemFactory`, so only the ~visible rows ever become widgets and
//! only their images are fetched (lazily, in `bind`, cancelled on recycle).
//! Replacing the row model is a cheap model swap, never a widget rebuild; the
//! now-playing highlight touches only the realized rows.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib};

use big_media_components::initials_avatar::initials_texture;

use crate::favicon_cache::FaviconLruCache;

/// Blocking, disk-cached byte fetcher for a remote row image. Run off-thread by
/// the view for visible rows only; `Send + Sync` so it can cross to a worker and
/// a closure so callers can capture per-request state (e.g. provider headers).
pub type ImageFetcher = Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

/// How a row's leading image is sourced.
#[derive(Clone)]
pub enum RowImage {
    /// A generated initials avatar (no network).
    Initials {
        /// Text the initials are derived from.
        text: String,
        /// Pixel size of the avatar.
        size: i32,
    },
    /// A remote image with an initials fallback shown until it loads. `fetch`
    /// is the (blocking, disk-cached) byte fetcher run off-thread for VISIBLE
    /// rows only.
    Remote {
        /// Image URL.
        url: String,
        /// Initials fallback shown until the image loads.
        initials: String,
        /// Pixel size of the image.
        size: i32,
        /// Blocking byte fetcher (run off-thread), e.g. disk-cached favicon. A
        /// closure (not a bare `fn`) so callers can capture per-request state
        /// such as provider HTTP headers; `Send + Sync` because the view runs it
        /// on a worker thread.
        fetch: ImageFetcher,
    },
    /// A themed icon name.
    Icon {
        /// Themed icon name.
        name: String,
        /// Pixel size of the icon.
        size: i32,
    },
}

impl RowImage {
    fn size(&self) -> i32 {
        match self {
            Self::Initials { size, .. } | Self::Remote { size, .. } | Self::Icon { size, .. } => {
                *size
            }
        }
    }
}

/// One row's display and action payload. A `header` row renders as a
/// non-activatable section heading (inline grouping); everything else is an item
/// row.
#[derive(Clone)]
pub struct MediaRow {
    /// Stable key (stream/feed URL): activation payload + now-playing match.
    pub id: String,
    /// Primary row label.
    pub title: String,
    /// Optional secondary line.
    pub subtitle: Option<String>,
    /// Leading image source.
    pub image: RowImage,
    /// `Some(text)` → this is a section header, not an item.
    pub header: Option<String>,
    /// Favicon URL carried for actions (radio "add to my stations"); else empty.
    pub favicon: String,
    /// Group name carried for actions; else empty.
    pub group: String,
    /// Panel-defined discriminator so one `build_suffix` can tell row kinds
    /// apart (e.g. saved vs explore, subscribed vs search result).
    pub tag: u32,
}

impl MediaRow {
    /// A section-header row.
    #[must_use]
    pub fn header(text: impl Into<String>) -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            subtitle: None,
            image: RowImage::Icon {
                name: String::new(),
                size: 0,
            },
            header: Some(text.into()),
            favicon: String::new(),
            group: String::new(),
            tag: 0,
        }
    }

    fn is_header(&self) -> bool {
        self.header.is_some()
    }
}

mod imp {
    use std::cell::RefCell;

    use gtk::glib;
    use gtk::subclass::prelude::*;

    use super::MediaRow;

    #[derive(Default)]
    pub struct MediaListRowModel {
        pub row: RefCell<Option<MediaRow>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MediaListRowModel {
        const NAME: &'static str = "BigPlayerMediaRowModel";
        type Type = super::MediaListRowModel;
    }

    impl ObjectImpl for MediaListRowModel {}
}

glib::wrapper! {
    /// GObject wrapper holding one [`MediaRow`] for the list model.
    pub struct MediaListRowModel(ObjectSubclass<imp::MediaListRowModel>);
}

impl MediaListRowModel {
    fn new(row: MediaRow) -> Self {
        let media_row_model: Self = glib::Object::new();
        *media_row_model.imp().row.borrow_mut() = Some(row);
        media_row_model
    }

    fn row(&self) -> MediaRow {
        self.imp()
            .row
            .borrow()
            .clone()
            .expect("MediaListRowModel initialized")
    }
}

/// CSS class marking a realized item row (used to find rows for highlight).
const ROW_CLASS: &str = "media-list-row";
/// CSS class applied to the currently-playing row.
const NOW_PLAYING_CLASS: &str = "radio-now-playing";

/// A recycling media list. Replace its contents with [`set_items`](Self::set_items);
/// update the highlight with [`set_now_playing`](Self::set_now_playing).
pub struct MediaListView {
    /// The `ListView`; the caller parks it in a `ScrolledWindow`.
    pub widget: gtk::Widget,
    store: gtk::gio::ListStore,
    now_playing: Rc<RefCell<String>>,
}

impl MediaListView {
    /// Build a virtualized list. `on_activate(id)` fires on row activation (never
    /// for headers); `build_suffix(row)` returns the per-row action buttons,
    /// rebuilt on each (visible) bind.
    pub fn new(
        on_activate: impl Fn(&str) + 'static,
        build_suffix: impl Fn(&MediaRow) -> Vec<gtk::Widget> + 'static,
    ) -> Self {
        let store = gtk::gio::ListStore::new::<MediaListRowModel>();
        let selection = gtk::NoSelection::new(Some(store.clone()));
        let now_playing = Rc::new(RefCell::new(String::new()));
        let image_cache = Rc::new(RefCell::new(FaviconLruCache::default()));
        let factory = gtk::SignalListItemFactory::new();
        wire_factory(&factory, &now_playing, &image_cache, Rc::new(build_suffix));

        let view = gtk::ListView::builder()
            .model(&selection)
            .factory(&factory)
            .single_click_activate(true)
            .css_classes(["navigation-sidebar", "media-list"])
            .build();
        let store_for_activate = store.clone();
        view.connect_activate(move |_, position| {
            if let Some(media_row_model) = store_for_activate
                .item(position)
                .and_downcast::<MediaListRowModel>()
            {
                let row = media_row_model.row();
                if !row.is_header() {
                    on_activate(&row.id);
                }
            }
        });

        Self {
            widget: view.upcast(),
            store,
            now_playing,
        }
    }

    /// Replace the list contents (a cheap model swap; only visible rows rebind).
    pub fn set_items(&self, rows: Vec<MediaRow>) {
        let media_row_models: Vec<MediaListRowModel> =
            rows.into_iter().map(MediaListRowModel::new).collect();
        self.store
            .splice(0, self.store.n_items(), &media_row_models);
    }

    /// Number of rows currently in the model.
    #[must_use]
    pub fn len(&self) -> u32 {
        self.store.n_items()
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.store.n_items() == 0
    }

    /// Set the now-playing id and refresh the highlight on the realized rows
    /// only (no rebuild, no full-tree walk).
    pub fn set_now_playing(&self, id: &str) {
        *self.now_playing.borrow_mut() = id.to_owned();
        for row in realized_rows(&self.widget) {
            let playing = !id.is_empty() && row.widget_name() == id;
            toggle_class(&row, NOW_PLAYING_CLASS, playing);
        }
    }
}

/// Wire the factory's setup/bind/unbind. Setup builds the row shell once; bind
/// fills it from the bound `MediaListRowModel` and starts the image fetch for
/// visible rows only; unbind cancels the fetch and clears per-bind widgets.
fn wire_factory(
    factory: &gtk::SignalListItemFactory,
    now_playing: &Rc<RefCell<String>>,
    image_cache: &Rc<RefCell<FaviconLruCache>>,
    build_suffix: Rc<dyn Fn(&MediaRow) -> Vec<gtk::Widget>>,
) {
    factory.connect_setup(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let image = gtk::Image::builder()
            .pixel_size(24)
            .valign(gtk::Align::Center)
            .build();
        let title = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let subtitle = gtk::Label::builder()
            .xalign(0.0)
            .visible(false)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["dim-label", "caption"])
            .build();
        let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
        text.set_hexpand(true);
        text.set_valign(gtk::Align::Center);
        text.append(&title);
        text.append(&subtitle);
        let suffix = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        suffix.set_valign(gtk::Align::Center);
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(10)
            .css_classes([ROW_CLASS])
            .build();
        row.append(&image);
        row.append(&text);
        row.append(&suffix);
        list_item.set_child(Some(&row));
    });

    factory.connect_bind({
        let now_playing = now_playing.clone();
        let image_cache = image_cache.clone();
        move |_, list_item| {
            let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(media_row_model) = list_item.item().and_downcast::<MediaListRowModel>() else {
                return;
            };
            let Some(row_box) = list_item.child().and_downcast::<gtk::Box>() else {
                return;
            };
            let media_row = media_row_model.row();
            let Some((image, text, suffix)) = row_parts(&row_box) else {
                return;
            };
            let Some(title) = text.first_child().and_downcast::<gtk::Label>() else {
                return;
            };
            let Some(subtitle) = text.last_child().and_downcast::<gtk::Label>() else {
                return;
            };

            // Section header: a heading, not activatable, no image/suffix.
            if let Some(heading) = &media_row.header {
                row_box.set_widget_name("");
                list_item.set_activatable(false);
                image.set_visible(false);
                suffix.set_visible(false);
                title.set_label(heading);
                title.set_css_classes(&["heading"]);
                subtitle.set_visible(false);
                toggle_class(&row_box, NOW_PLAYING_CLASS, false);
                return;
            }

            list_item.set_activatable(true);
            row_box.set_widget_name(&media_row.id);
            row_box.update_property(&[gtk::accessible::Property::Label(&media_row.title)]);
            image.set_visible(true);
            suffix.set_visible(true);
            title.set_css_classes(&[]);
            title.set_label(&media_row.title);
            match &media_row.subtitle {
                Some(text) if !text.is_empty() => {
                    subtitle.set_label(text);
                    subtitle.set_visible(true);
                }
                _ => subtitle.set_visible(false),
            }
            bind_image(&image, &media_row.image, &image_cache, list_item);

            // Rebuild the action buttons for this bind (visible rows only).
            while let Some(child) = suffix.first_child() {
                suffix.remove(&child);
            }
            for widget in build_suffix(&media_row) {
                suffix.append(&widget);
            }

            let playing = {
                let current = now_playing.borrow();
                !current.is_empty() && *current == media_row.id
            };
            toggle_class(&row_box, NOW_PLAYING_CLASS, playing);
        }
    });

    factory.connect_unbind(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        // An in-flight image fetch for this slot is dropped at completion (its
        // bound-row check fails once the slot rebinds); just clear per-bind
        // widgets here.
        if let Some(row_box) = list_item.child().and_downcast::<gtk::Box>() {
            let Some((_, _, suffix)) = row_parts(&row_box) else {
                return;
            };
            while let Some(child) = suffix.first_child() {
                suffix.remove(&child);
            }
        }
    });
}

/// (image, text-box, suffix-box) from a row built in `setup`.
fn row_parts(row_box: &gtk::Box) -> Option<(gtk::Image, gtk::Box, gtk::Box)> {
    let image = row_box.first_child().and_downcast::<gtk::Image>()?;
    let text = image.next_sibling().and_downcast::<gtk::Box>()?;
    let suffix = row_box.last_child().and_downcast::<gtk::Box>()?;
    Some((image, text, suffix))
}

/// Set the row image: initials/icon are instant; a remote image shows its
/// initials fallback immediately, serves the mem cache, and otherwise fetches
/// off-thread for this VISIBLE row, guarded by a per-slot token so a recycle
/// drops the stale result.
fn bind_image(
    image: &gtk::Image,
    source: &RowImage,
    cache: &Rc<RefCell<FaviconLruCache>>,
    list_item: &gtk::ListItem,
) {
    image.set_pixel_size(source.size().max(1));
    match source {
        RowImage::Initials { text, size } => {
            set_initials(image, text, *size);
        }
        RowImage::Icon { name, .. } => {
            image.set_icon_name(Some(name));
        }
        RowImage::Remote {
            url,
            initials,
            size,
            fetch,
        } => {
            if let Some(texture) = cache.borrow_mut().0.get(url) {
                image.set_paintable(Some(texture));
                return;
            }
            set_initials(image, initials, *size);
            if url.is_empty() {
                return;
            }
            let image_weak = image.downgrade();
            let cache = cache.clone();
            let cache_key = url.clone();
            let fetch_url = url.clone();
            let size = *size;
            let fetch = fetch.clone();
            let list_item_weak = list_item.downgrade();
            big_media_components::player_chrome::spawn_blocking_result(
                move || {
                    fetch(&fetch_url).and_then(|image_bytes| bytes_to_texture(&image_bytes, size))
                },
                move |texture| {
                    let Some(texture) = texture else { return };
                    // Cache under the fetched URL regardless of recycling.
                    cache.borrow_mut().0.put(cache_key.clone(), texture.clone());
                    // Drop the visible set if this slot has since recycled to a
                    // different row (no per-slot token needed: the bound
                    // MediaListRowModel changes on rebind).
                    let still_bound = list_item_weak
                        .upgrade()
                        .and_then(|item| item.item())
                        .and_downcast::<MediaListRowModel>()
                        .is_some_and(|media_row_model| media_row_model.row().id == cache_key);
                    if !still_bound {
                        return;
                    }
                    if let Some(image) = image_weak.upgrade() {
                        image.set_paintable(Some(&texture));
                    }
                },
            );
        }
    }
}

fn set_initials(image: &gtk::Image, text: &str, size: i32) {
    if let Some(texture) = initials_texture(text, size.max(1)) {
        image.set_paintable(Some(&texture));
    } else {
        image.set_icon_name(Some("audio-x-generic-symbolic"));
    }
}

fn bytes_to_texture(image_bytes: &[u8], size: i32) -> Option<gdk::Texture> {
    big_media_components::media::images::bytes_to_thumbnail(image_bytes, size.max(1))
}

/// Realized item rows under a `ListView` (only the ~visible ones exist).
fn realized_rows(view: &gtk::Widget) -> Vec<gtk::Box> {
    let mut rows = Vec::new();
    collect_rows(view, &mut rows);
    rows
}

fn collect_rows(widget: &gtk::Widget, rows: &mut Vec<gtk::Box>) {
    if let Some(row) = widget.downcast_ref::<gtk::Box>()
        && row.has_css_class(ROW_CLASS)
    {
        rows.push(row.clone());
        return;
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        collect_rows(&current, rows);
        child = current.next_sibling();
    }
}

fn toggle_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}
