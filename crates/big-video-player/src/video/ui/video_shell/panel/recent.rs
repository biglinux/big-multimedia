//! The panel's "Recent" page.

use super::*;
use big_relm4_components::task::spawn_blocking_result;

/// The last videos opened, as cards: picture, name and length. A click
/// opens one. Pictures and lengths come off the main loop. The search above
/// them filters as it is typed, over every file the store keeps (50); with
/// nothing typed the most recent twelve show.
pub(super) fn recent_page(columns: usize, sender: &ComponentSender<VideoShell>) -> gtk::Widget {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .build();
    let weak_page = page.downgrade();
    let sender = sender.clone();
    // The history file may sit on a slow or stale home mount.
    spawn_blocking_result(
        || crate::recent::load_recent_for("video"),
        move |entries| {
            if let Some(page) = weak_page.upgrade() {
                fill_recent_page(&page, entries, columns, &sender);
            }
        },
    );
    page.upcast()
}

/// Enough to find the recent one without a list to scroll through.
const SHOWN: usize = 12;

fn fill_recent_page(
    page: &gtk::Box,
    entries: Vec<crate::recent::RecentEntry>,
    columns: usize,
    sender: &ComponentSender<VideoShell>,
) {
    if entries.is_empty() {
        page.append(
            &gtk::Label::builder()
                .label(i18n("No Recent Files"))
                .css_classes(["dim-label"])
                .margin_top(24)
                .margin_bottom(24)
                .build(),
        );
        return;
    }
    let grid = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(u32::try_from(columns.max(2)).unwrap_or(2))
        .max_children_per_line(u32::try_from(columns.max(2)).unwrap_or(2))
        .column_spacing(8)
        .row_spacing(8)
        .build();
    let names: Rc<[String]> = entries
        .iter()
        .map(|entry| crate::media::playlist::display_name(&entry.path))
        .collect();
    for entry in entries {
        grid.append(&recent_card(entry, sender));
    }
    // Tab moves between the cards, not the flow box's own cells.
    let mut cell = grid.first_child();
    while let Some(child) = cell {
        child.set_focusable(false);
        cell = child.next_sibling();
    }

    let (search, empty) = recent_search(&grid, names, sender);
    page.append(&search);
    page.append(&empty);
    page.append(&grid);
}

/// One recent file as a card that opens it; its picture and length load off
/// the main loop, except for streams.
fn recent_card(
    entry: crate::recent::RecentEntry,
    sender: &ComponentSender<VideoShell>,
) -> gtk::Button {
    // Without the extension, as the Files and Playlist lists name videos.
    let name = crate::media::playlist::display_name(&entry.path);
    let (frame, picture, placeholder, length) = card_frame(&entry.path);
    let label = gtk::Label::builder()
        .label(&name)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(1)
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    content.append(&frame);
    content.append(&label);
    let card = gtk::Button::builder()
        .child(&content)
        .tooltip_text(&entry.path)
        .css_classes(["recent-card"])
        .build();
    card.update_property(&[gtk::accessible::Property::Label(&name)]);
    {
        let sender = sender.clone();
        let path = entry.path.clone();
        card.connect_clicked(move |_| {
            sender.input(VideoShellInput::OpenFiles(vec![path.clone()]));
            sender.input(VideoShellInput::ClosePanel);
        });
    }
    if playback::is_remote_uri(&entry.path) {
        return card;
    }
    load_card_media(entry.path, &picture, &placeholder, &length);
    card
}

/// The card's picture area: a placeholder icon for the kind of file, the
/// picture once decoded, and the length in a corner.
fn card_frame(path: &str) -> (gtk::Overlay, gtk::Picture, gtk::Image, gtk::Label) {
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .css_classes(["recent-thumb"])
        .build();
    let length = gtk::Label::builder()
        .halign(gtk::Align::End)
        .valign(gtk::Align::End)
        .margin_end(6)
        .margin_bottom(6)
        .visible(false)
        .css_classes(["recent-length"])
        .build();
    // Until a picture comes, and for what has none (music opened here,
    // streams): what kind of file it is.
    let audio = gtk::gio::content_type_guess(Some(path), None)
        .0
        .starts_with("audio/");
    let placeholder = gtk::Image::builder()
        .icon_name(if audio {
            "audio-x-generic-symbolic"
        } else {
            "video-x-generic-symbolic"
        })
        .pixel_size(32)
        .css_classes(["dim-label"])
        .build();
    // The box sets the height; the picture, not measured, fills it (a
    // picture's own size would make video cards taller than music ones).
    let size = gtk::Box::builder().height_request(90).build();
    let frame = gtk::Overlay::builder().child(&size).build();
    frame.add_overlay(&picture);
    frame.set_measure_overlay(&picture, false);
    frame.add_overlay(&placeholder);
    frame.add_overlay(&length);
    (frame, picture, placeholder, length)
}

/// The card's picture and length, decoded and probed off the main loop.
fn load_card_media(
    path: String,
    picture: &gtk::Picture,
    placeholder: &gtk::Image,
    length: &gtk::Label,
) {
    let picture = picture.downgrade();
    let placeholder = placeholder.downgrade();
    let length = length.downgrade();
    glib::spawn_future_local(async move {
        let wanted = path.clone();
        let decoded =
            gtk::gio::spawn_blocking(move || crate::media::thumbnail::load_thumbnail(&wanted))
                .await
                .ok()
                .flatten();
        if let (Some((thumb, image)), Some(picture)) = (decoded, picture.upgrade()) {
            let texture = crate::media::thumbnail::thumbnail_texture(thumb, image);
            picture.set_paintable(Some(&texture));
            if let Some(placeholder) = placeholder.upgrade() {
                placeholder.set_visible(false);
            }
        }
        let seconds = gtk::gio::spawn_blocking(move || {
            crate::media::playlist::probe_metadata_cached(&path).and_then(|meta| meta.duration_secs)
        })
        .await
        .ok()
        .flatten()
        .filter(|seconds| *seconds > 0.0);
        if let (Some(seconds), Some(length)) = (seconds, length.upgrade()) {
            length.set_label(&big_media_components::time::duration_hms(seconds));
            length.set_visible(true);
        }
    });
}

/// The search over every kept name: it filters the grid as it is typed, says
/// when nothing matches, and Escape on an empty search closes the panel.
fn recent_search(
    grid: &gtk::FlowBox,
    names: Rc<[String]>,
    sender: &ComponentSender<VideoShell>,
) -> (gtk::SearchEntry, gtk::Label) {
    let search = gtk::SearchEntry::builder()
        .placeholder_text(i18n("Search recent files"))
        .search_delay(0)
        .build();
    search.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Search recent files",
    ))]);
    // Typing starts the search: focus it once the page is on screen.
    search.connect_map(|search| {
        let search = search.downgrade();
        glib::idle_add_local_once(move || {
            if let Some(search) = search.upgrade() {
                search.grab_focus();
            }
        });
    });
    let empty = gtk::Label::builder()
        .label(i18n("No recent file matches"))
        .css_classes(["dim-label"])
        .margin_top(12)
        .margin_bottom(12)
        .visible(false)
        .build();
    filter_recent(grid, &search, names, &empty);
    // A search entry keeps Escape to itself: the first clears what was
    // typed, the next closes the panel as Escape does elsewhere.
    search.connect_stop_search({
        let sender = sender.clone();
        move |search| {
            if search.text().is_empty() {
                sender.input(VideoShellInput::ClosePanel);
            } else {
                search.set_text("");
            }
        }
    });
    (search, empty)
}

/// Show the first `SHOWN` cards, or every one whose name matches the search,
/// and the empty label when none does.
fn filter_recent(
    grid: &gtk::FlowBox,
    search: &gtk::SearchEntry,
    names: Rc<[String]>,
    empty: &gtk::Label,
) {
    {
        let search = search.downgrade();
        let names = Rc::clone(&names);
        grid.set_filter_func(move |child| {
            let index = usize::try_from(child.index()).unwrap_or(usize::MAX);
            let query = search.upgrade().map(|s| s.text()).unwrap_or_default();
            if query.trim().is_empty() {
                return index < SHOWN;
            }
            names
                .get(index)
                .is_some_and(|name| recent_matches(query.trim(), name))
        });
    }
    search.connect_search_changed({
        let grid = grid.downgrade();
        let empty = empty.downgrade();
        let names = names.clone();
        move |search| {
            let Some(grid) = grid.upgrade() else {
                return;
            };
            grid.invalidate_filter();
            let query = search.text();
            let query = query.trim();
            let any = query.is_empty() || names.iter().any(|name| recent_matches(query, name));
            if let Some(empty) = empty.upgrade() {
                empty.set_visible(!any);
            }
        }
    });
}

/// As the file browser's search: the letters typed, in order.
fn recent_matches(query: &str, name: &str) -> bool {
    crate::media::playlist::fuzzy_match(query, name).is_some()
}
