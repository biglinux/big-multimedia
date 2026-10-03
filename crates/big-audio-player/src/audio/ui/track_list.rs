//! The track lists of the queue and the library: one drawn `ItemView` each.
//!
//! A `GtkColumnView` of 5 000 tracks kept 2 903 widgets alive and cost
//! +10 MiB; the drawn view keeps 44 and +1.6 MiB. Both lists show the same
//! columns, so the columns, their words, their order and what a press on a
//! heading means live here; the panels say only what differs (the first
//! column, the button at the end of a row, how the playing track is known).

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::rc::Rc;

use adw::prelude::*;
use big_item_view::{
    Column as ViewColumn, ColumnHeader, ColumnShows, Geometry, IconSource, ItemContent,
    ItemPresenter, ItemView,
};
use gtk::{gdk, gio, glib};

use crate::db::models::Track;
use crate::i18n::i18n;

/// A column either list can show. The order is each column's id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Column {
    /// The row's place in the queue.
    Position,
    /// The track number from the tags.
    TrackNumber,
    Title,
    Artist,
    Album,
    Duration,
    Genre,
    Year,
    Bitrate,
    Format,
    Rating,
    /// The panel's button at the end of a row.
    Action,
}

impl Column {
    pub(super) const ALL: [Self; 12] = [
        Self::Position,
        Self::TrackNumber,
        Self::Title,
        Self::Artist,
        Self::Album,
        Self::Duration,
        Self::Genre,
        Self::Year,
        Self::Bitrate,
        Self::Format,
        Self::Rating,
        Self::Action,
    ];

    pub(super) fn id(self) -> u32 {
        Self::ALL
            .iter()
            .position(|column| *column == self)
            .unwrap_or(0) as u32
    }

    pub(super) fn from_id(id: u32) -> Option<Self> {
        Self::ALL.get(id as usize).copied()
    }

    /// The heading; also the column's name in the saved widths.
    pub(super) fn title(self) -> String {
        match self {
            Self::Position | Self::TrackNumber => "#".to_owned(),
            Self::Title => i18n("Title"),
            Self::Artist => i18n("Artist"),
            Self::Album => i18n("Album"),
            Self::Duration => i18n("Duration"),
            Self::Genre => i18n("Genre"),
            Self::Year => i18n("Year"),
            Self::Bitrate => i18n("Bitrate"),
            Self::Format => i18n("Format"),
            Self::Rating => i18n("Rating"),
            Self::Action => String::new(),
        }
    }

    fn default_width(self) -> i32 {
        match self {
            Self::Position | Self::TrackNumber => 44,
            Self::Title => 200,
            Self::Artist | Self::Album => 150,
            Self::Duration => 84,
            Self::Format => 72,
            Self::Genre => 100,
            Self::Year => 60,
            Self::Bitrate | Self::Rating => 90,
            Self::Action => 36,
        }
    }

    fn xalign(self) -> f32 {
        match self {
            Self::Position | Self::TrackNumber | Self::Duration | Self::Year | Self::Bitrate => 1.0,
            Self::Action => 0.5,
            _ => 0.0,
        }
    }

    /// Whether the list can be sorted by it, and the person can hide it.
    fn is_data(self) -> bool {
        !matches!(self, Self::Position | Self::Action)
    }

    /// The words the column shows for `track` at queue `position`.
    pub(super) fn cell(self, track: &Track, position: u32) -> String {
        match self {
            Self::Position => (position + 1).to_string(),
            Self::TrackNumber => track
                .track_number
                .map(|n| n.to_string())
                .unwrap_or_default(),
            Self::Title => track.display_title().to_owned(),
            Self::Artist => track.display_artist().to_owned(),
            Self::Album => track.display_album().to_owned(),
            Self::Duration if track.duration_secs > 0.0 => track.duration_display(),
            Self::Genre => track.genre.clone().unwrap_or_default(),
            Self::Year => track.year.map(|year| year.to_string()).unwrap_or_default(),
            Self::Bitrate => track
                .bitrate
                .map(|bps| {
                    big_media_components::audio::labels::bitrate_kbps_label_from_bps(bps.into())
                })
                .unwrap_or_default(),
            Self::Format => track.format.clone().unwrap_or_default(),
            Self::Duration | Self::Rating | Self::Action => String::new(),
        }
    }

    /// How two tracks compare by this column, ascending.
    pub(super) fn compare(self, a: &Track, b: &Track) -> Ordering {
        let text = |key: fn(&Track) -> &str| key(a).to_lowercase().cmp(&key(b).to_lowercase());
        match self {
            Self::TrackNumber => a
                .track_number
                .unwrap_or(0)
                .cmp(&b.track_number.unwrap_or(0)),
            Self::Title => text(Track::display_title),
            Self::Artist => text(Track::display_artist),
            Self::Album => text(Track::display_album),
            Self::Duration => a.duration_secs.total_cmp(&b.duration_secs),
            Self::Genre => text(|t| t.genre.as_deref().unwrap_or("")),
            Self::Year => a.year.unwrap_or(0).cmp(&b.year.unwrap_or(0)),
            Self::Bitrate => a.bitrate.unwrap_or(0).cmp(&b.bitrate.unwrap_or(0)),
            Self::Format => text(|t| t.format.as_deref().unwrap_or("")),
            Self::Rating => a.rating.cmp(&b.rating),
            Self::Position | Self::Action => Ordering::Equal,
        }
    }
}

/// The columns of one list: which, in what order, how wide, sorted by which.
#[derive(Clone, Debug)]
pub(super) struct Columns {
    order: Vec<Column>,
    hidden: Vec<Column>,
    widths: Vec<i32>,
    sort: Option<(Column, gtk::SortType)>,
    /// The symbolic icon of the [`Column::Action`] button.
    action_icon: &'static str,
    /// The picture's size in the title column; 0 for none.
    art: i32,
}

impl Columns {
    /// The queue: its place, title, artist, duration and a remove button.
    pub(super) fn queue() -> Self {
        Self::new(
            Column::Position,
            &[
                Column::Album,
                Column::Genre,
                Column::Year,
                Column::Bitrate,
                Column::Format,
                Column::Rating,
            ],
            "edit-delete-symbolic",
            0,
        )
    }

    /// The library: cover, title, artist, duration and an add button.
    pub(super) fn library() -> Self {
        Self::new(
            Column::TrackNumber,
            &[
                Column::TrackNumber,
                Column::Album,
                Column::Genre,
                Column::Year,
                Column::Bitrate,
                Column::Format,
                Column::Rating,
            ],
            "list-add-symbolic",
            28,
        )
    }

    fn new(first: Column, hidden: &[Column], action_icon: &'static str, art: i32) -> Self {
        let order = std::iter::once(first)
            .chain(
                Column::ALL
                    .into_iter()
                    .filter(|column| !matches!(column, Column::Position | Column::TrackNumber)),
            )
            .collect();
        Self {
            order,
            hidden: hidden.to_vec(),
            widths: Column::ALL
                .iter()
                .map(|column| column.default_width())
                .collect(),
            sort: None,
            action_icon,
            art,
        }
    }

    pub(super) fn shown(&self, column: Column) -> bool {
        !self.hidden.contains(&column)
    }

    /// The columns the person can show or hide, in order.
    pub(super) fn toggleable(&self) -> impl Iterator<Item = Column> + '_ {
        self.order
            .iter()
            .copied()
            .filter(|column| column.is_data() && *column != Column::Title)
    }

    pub(super) fn toggle(&mut self, column: Column) {
        if let Some(at) = self.hidden.iter().position(|hidden| *hidden == column) {
            self.hidden.remove(at);
        } else if column.is_data() && column != Column::Title {
            self.hidden.push(column);
        }
    }

    pub(super) fn resize(&mut self, column: Column, width: i32) {
        if let Some(slot) = self.widths.get_mut(column.id() as usize) {
            *slot = width.clamp(24, 800);
        }
    }

    /// Move `column` to `to`, counted among the shown columns.
    pub(super) fn reorder(&mut self, column: Column, to: u32) {
        let shown: Vec<Column> = self
            .order
            .iter()
            .copied()
            .filter(|c| self.shown(*c))
            .collect();
        let Some(&target) = shown.get(to as usize) else {
            return;
        };
        let (Some(from), Some(at)) = (
            self.order.iter().position(|c| *c == column),
            self.order.iter().position(|c| *c == target),
        ) else {
            return;
        };
        let moved = self.order.remove(from);
        self.order.insert(at, moved);
    }

    /// Sort by `column`: ascending, or turned round when sorted by it already.
    /// `None` for a column that does not sort.
    pub(super) fn sort_by(&mut self, column: Column) -> Option<(Column, gtk::SortType)> {
        if !column.is_data() {
            return None;
        }
        let direction = match self.sort {
            Some((sorted, gtk::SortType::Ascending)) if sorted == column => {
                gtk::SortType::Descending
            }
            _ => gtk::SortType::Ascending,
        };
        self.sort = Some((column, direction));
        self.sort
    }

    /// Saved widths, `title=width,…` as the settings keep them.
    pub(super) fn widths_spec(&self) -> Option<String> {
        let entries: Vec<(String, i32)> = self
            .order
            .iter()
            .map(|column| (column.title(), self.widths[column.id() as usize]))
            .collect();
        big_relm4_components::layout::column_persist::encode_column_width_entries(
            entries
                .iter()
                .map(|(title, width)| (title.as_str(), *width)),
        )
    }

    pub(super) fn apply_widths(&mut self, spec: &str) {
        for (name, width) in spec
            .split(',')
            .filter_map(|entry| entry.split_once('='))
            .filter(|(name, _)| !name.is_empty())
        {
            let Ok(width) = width.parse::<i32>() else {
                continue;
            };
            if let Some(column) = self.order.iter().copied().find(|c| c.title() == name) {
                self.resize(column, width);
            }
        }
    }

    /// The view's shape for these columns.
    pub(super) fn geometry(&self) -> Geometry {
        let mut geometry = Geometry::rows(self.art);
        geometry.gap = if self.art > 0 { 8 } else { 0 };
        geometry.columns = self
            .order
            .iter()
            .copied()
            .filter(|column| self.shown(*column))
            .map(|column| ViewColumn {
                id: column.id(),
                title: column.title(),
                shows: match column {
                    Column::Title => ColumnShows::Primary,
                    Column::Rating => ColumnShows::Stars(0),
                    Column::Action => ColumnShows::Button(self.action_icon),
                    other => ColumnShows::Cell(other.id() as usize),
                },
                width: self.widths[column.id() as usize],
                expand: column == Column::Title,
                xalign: column.xalign(),
                sort: self
                    .sort
                    .filter(|(sorted, _)| *sorted == column)
                    .map(|(_, way)| way),
            })
            .collect();
        geometry
    }
}

/// The track an item of either list holds.
pub(super) fn track_of(item: &glib::Object) -> Option<std::cell::Ref<'_, Track>> {
    Some(
        item.downcast_ref::<glib::BoxedAnyObject>()?
            .borrow::<Track>(),
    )
}

/// Show `rating` on the row at `position` at once; the track's id for the
/// shell to keep it.
pub(super) fn rate_in_place(store: &gio::ListStore, position: u32, rating: i32) -> Option<i64> {
    let item = store
        .item(position)?
        .downcast::<glib::BoxedAnyObject>()
        .ok()?;
    let mut track = item.borrow_mut::<Track>();
    track.rating = rating;
    Some(track.id)
}

/// The cover for a track, if known now; else asked for, and the view is
/// refreshed when it arrives.
pub(super) type Cover = Box<dyn Fn(&Track) -> Option<gdk::Texture>>;
/// What dragging these positions carries.
pub(super) type Drag = Box<dyn Fn(&[u32]) -> Option<gdk::ContentProvider>>;
/// Take a drop on a position (the end when `None`).
pub(super) type Receive = Box<dyn Fn(Option<u32>, &glib::Value) -> bool>;

/// What a list does beyond showing tracks.
pub(super) struct Behaviour {
    /// Whether `item` (holding `track`) is the one playing.
    pub(super) is_current: Box<dyn Fn(&glib::Object, &Track) -> bool>,
    pub(super) cover: Option<Cover>,
    pub(super) drag: Option<Drag>,
    pub(super) drop: Option<Receive>,
}

/// What one list's view asks about its tracks.
pub(super) struct TrackPresenter {
    behaviour: Behaviour,
    /// Read for each item: the words of every column, by id.
    columns: Rc<RefCell<Columns>>,
}

impl ItemPresenter for TrackPresenter {
    fn content(&self, position: u32, item: &glib::Object, out: &mut ItemContent) {
        let Some(track) = track_of(item) else {
            return;
        };
        track.display_title().clone_into(&mut out.title);
        out.emphasized = (self.behaviour.is_current)(item, &track);
        out.cells = Column::ALL
            .iter()
            .map(|column| column.cell(&track, position))
            .collect();
        out.ratings = vec![u8::try_from(track.rating.clamp(0, 5)).unwrap_or(0)];
        if self.columns.borrow().art > 0 {
            out.frame = big_item_view::IconFrame::Cover;
            if let Some(texture) = self
                .behaviour
                .cover
                .as_ref()
                .and_then(|cover| cover(&track))
            {
                out.icon = IconSource::Texture(texture);
            }
        }
        let mut said = vec![track.display_artist().to_owned()];
        if track.duration_secs > 0.0 {
            said.push(track.duration_display());
        }
        if track.rating > 0 {
            said.push(i18n("{} of 5 stars").replace("{}", &track.rating.to_string()));
        }
        if out.emphasized {
            said.push(i18n("Playing"));
        }
        out.accessible_description = Some(said.join(", "));
    }

    fn title(&self, _position: u32, item: &glib::Object) -> String {
        track_of(item)
            .map(|track| track.display_title().to_owned())
            .unwrap_or_default()
    }

    fn tooltip(&self, _position: u32, item: &glib::Object) -> Option<String> {
        track_of(item).map(|track| track.file_path.clone())
    }

    fn drag_content(&self, positions: &[u32]) -> Option<gdk::ContentProvider> {
        self.behaviour.drag.as_ref()?(positions)
    }

    fn accepts_drop(&self, _target: Option<u32>) -> bool {
        self.behaviour.drop.is_some()
    }

    fn receive_drop(&self, target: Option<u32>, value: &glib::Value, _want_move: bool) -> bool {
        self.behaviour
            .drop
            .as_ref()
            .is_some_and(|drop| drop(target, value))
    }
}

/// One list: the view, its headings, and the columns behind them.
pub(super) struct TrackList {
    pub(super) view: ItemView,
    pub(super) header: ColumnHeader,
    pub(super) columns: Rc<RefCell<Columns>>,
    /// Set while saved widths are applied, so they are not saved back.
    applying: Rc<Cell<bool>>,
}

/// What a list tells its panel.
pub(super) enum ListEvent {
    /// Sorted by a heading: the panel reorders its tracks with this order.
    Sorted(Column, gtk::SortType),
    /// The widths changed: save them.
    Widths(String),
    /// A star pressed on a row: rate it.
    Rate(u32, i32),
    /// The row's button pressed.
    Action(u32),
}

impl TrackList {
    pub(super) fn new(
        columns: Columns,
        behaviour: Behaviour,
        accessible_label: &str,
        on_event: impl Fn(ListEvent) + 'static,
    ) -> Self {
        let columns = Rc::new(RefCell::new(columns));
        let presenter = Rc::new(TrackPresenter {
            behaviour,
            columns: Rc::clone(&columns),
        });
        let view = ItemView::new(presenter, columns.borrow().geometry());
        view.set_vexpand(true);
        view.set_enable_rubberband(false);
        view.set_single_click_activate(false);
        view.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
        let header = ColumnHeader::new(&view);
        let list = Self {
            view,
            header,
            columns,
            applying: Rc::new(Cell::new(false)),
        };
        list.wire(Rc::new(on_event));
        list
    }

    fn wire(&self, on_event: Rc<dyn Fn(ListEvent)>) {
        let (columns, view) = (Rc::clone(&self.columns), self.view.downgrade());
        let event = Rc::clone(&on_event);
        self.header.connect_sort_requested(move |_, id| {
            let sorted =
                Column::from_id(id).and_then(|column| columns.borrow_mut().sort_by(column));
            if let (Some((column, way)), Some(view)) = (sorted, view.upgrade()) {
                view.set_geometry(columns.borrow().geometry());
                event(ListEvent::Sorted(column, way));
            }
        });
        let (columns, applying) = (Rc::clone(&self.columns), Rc::clone(&self.applying));
        let event = Rc::clone(&on_event);
        self.header.connect_column_resized(move |_, id, width| {
            let Some(column) = Column::from_id(id) else {
                return;
            };
            columns.borrow_mut().resize(column, width);
            if !applying.get()
                && let Some(spec) = columns.borrow().widths_spec()
            {
                event(ListEvent::Widths(spec));
            }
        });
        let (columns, view) = (Rc::clone(&self.columns), self.view.downgrade());
        self.header.connect_column_moved(move |_, id, to| {
            if let (Some(column), Some(view)) = (Column::from_id(id), view.upgrade()) {
                columns.borrow_mut().reorder(column, to);
                view.set_geometry(columns.borrow().geometry());
            }
        });
        let (columns, view) = (Rc::clone(&self.columns), self.view.downgrade());
        self.header.connect_context_menu(move |header, _, x, y| {
            if let Some(view) = view.upgrade() {
                show_column_menu(header, &view, &columns, x, y);
            }
        });
        let event = Rc::clone(&on_event);
        self.view
            .connect_cell_pressed(move |view, position, id, value| {
                match Column::from_id(id) {
                    Some(Column::Rating) => {
                        event(ListEvent::Rate(position, i32::try_from(value).unwrap_or(0)));
                        view.refresh(position..position + 1);
                    }
                    Some(Column::Action) => event(ListEvent::Action(position)),
                    _ => {}
                }
                view.grab_focus();
            });
    }

    /// Restore saved widths without saving them back.
    pub(super) fn apply_widths(&self, spec: &str) {
        self.applying.set(true);
        self.columns.borrow_mut().apply_widths(spec);
        self.view.set_geometry(self.columns.borrow().geometry());
        self.applying.set(false);
    }

    /// The headings over the list scrolled under them.
    pub(super) fn page(&self, scroll: &gtk::ScrolledWindow) -> gtk::Box {
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.append(&self.header);
        page.append(scroll);
        page
    }
}

/// Which columns are shown, as checks under the heading pressed.
fn show_column_menu(
    header: &ColumnHeader,
    view: &ItemView,
    columns: &Rc<RefCell<Columns>>,
    x: f64,
    y: f64,
) {
    let menu = gio::Menu::new();
    let actions = gio::SimpleActionGroup::new();
    for column in columns.borrow().toggleable().collect::<Vec<_>>() {
        let name = format!("column-{}", column.id());
        let action = gio::SimpleAction::new_stateful(
            &name,
            None,
            &columns.borrow().shown(column).to_variant(),
        );
        let (columns, view) = (Rc::clone(columns), view.downgrade());
        action.connect_activate(move |action, _| {
            columns.borrow_mut().toggle(column);
            action.set_state(&columns.borrow().shown(column).to_variant());
            if let Some(view) = view.upgrade() {
                view.set_geometry(columns.borrow().geometry());
            }
        });
        actions.add_action(&action);
        menu.append(Some(&column.title()), Some(&format!("columns.{name}")));
    }
    let popover = gtk::PopoverMenu::from_model(Some(&menu));
    popover.insert_action_group("columns", Some(&actions));
    popover.set_has_arrow(false);
    popover.set_parent(header);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    big_gtk_kit::popover::auto_unparent_on_close(&popover);
    popover.popup();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str, rating: i32) -> Track {
        let mut track = Track::new_external_path_with_title("/music/a.flac", Some(title.into()));
        track.rating = rating;
        track
    }

    #[test]
    fn a_heading_sorts_up_then_down_and_skips_columns_without_order() {
        let mut columns = Columns::queue();
        assert_eq!(columns.sort_by(Column::Position), None);
        assert_eq!(
            columns.sort_by(Column::Artist),
            Some((Column::Artist, gtk::SortType::Ascending))
        );
        assert_eq!(
            columns.sort_by(Column::Artist),
            Some((Column::Artist, gtk::SortType::Descending))
        );
        assert_eq!(
            columns.sort_by(Column::Title),
            Some((Column::Title, gtk::SortType::Ascending))
        );
    }

    #[test]
    fn titles_compare_without_case_and_ratings_by_number() {
        assert_eq!(
            Column::Title.compare(&track("abc", 0), &track("ABD", 0)),
            Ordering::Less
        );
        assert_eq!(
            Column::Rating.compare(&track("a", 4), &track("b", 2)),
            Ordering::Greater
        );
    }

    #[test]
    fn widths_round_trip_and_the_title_column_cannot_be_hidden() {
        let mut columns = Columns::library();
        columns.resize(Column::Artist, 210);
        let spec = columns.widths_spec().expect("titled columns");
        let mut restored = Columns::library();
        restored.apply_widths(&spec);
        assert_eq!(restored.widths, columns.widths);
        restored.apply_widths("Artist=abc,Nothing=99,=5");
        assert_eq!(restored.widths, columns.widths);

        columns.toggle(Column::Title);
        assert!(columns.shown(Column::Title));
        columns.toggle(Column::Album);
        assert!(columns.shown(Column::Album));
        columns.toggle(Column::Album);
        assert!(!columns.shown(Column::Album));
    }

    #[test]
    fn a_moved_heading_keeps_hidden_columns_in_place() {
        let mut columns = Columns::queue();
        // Shown: #, Title, Artist, Duration, (button). Artist to the front.
        columns.reorder(Column::Artist, 0);
        let shown: Vec<Column> = columns
            .order
            .iter()
            .copied()
            .filter(|c| columns.shown(*c))
            .collect();
        assert_eq!(
            shown[..3],
            [Column::Artist, Column::Position, Column::Title]
        );
        assert_eq!(columns.order.len(), Column::ALL.len() - 1);
    }

    #[test]
    fn geometry_puts_stars_and_the_button_in_their_columns() {
        let mut columns = Columns::queue();
        columns.toggle(Column::Rating);
        let geometry = columns.geometry();
        let shows: Vec<ColumnShows> = geometry.columns.iter().map(|c| c.shows).collect();
        assert!(shows.contains(&ColumnShows::Primary));
        assert!(shows.contains(&ColumnShows::Stars(0)));
        assert!(shows.contains(&ColumnShows::Button("edit-delete-symbolic")));
        assert_eq!(geometry.icon_size, 0);
        assert_eq!(Columns::library().geometry().icon_size, 28);
    }
}
