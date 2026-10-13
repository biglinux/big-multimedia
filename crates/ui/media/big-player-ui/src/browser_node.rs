//! `BigMediaBrowserNode`: generic GObject node type for a media library /
//! file-browser tree. Covers folder/file/divider plus the music-library
//! flavours (artist/album/genre/year/station/playlist). Reusable by any media
//! app; the constructors are convenience factories, not a fixed taxonomy.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::cell::{Cell, RefCell};

use big_media_data::i18n::i18n;

/// Category of a [`BrowserNode`]: what the row represents in the library/file tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, glib::Enum)]
#[enum_type(name = "BigMediaBrowserNodeKind")]
pub enum BrowserNodeKind {
    /// "All tracks" root row.
    #[default]
    All = 0,
    /// An artist grouping.
    Artist = 1,
    /// An album grouping.
    Album = 2,
    /// A genre grouping.
    Genre = 3,
    /// A release-year grouping.
    Year = 4,
    /// A filesystem folder.
    Folder = 5,
    /// A radio station.
    Station = 6,
    /// A user playlist.
    Playlist = 7,
    /// A rule-based (smart) playlist.
    SmartPlaylist = 8,
    /// A single media file / track.
    File = 9,
    /// A non-selectable alphabetical divider row.
    Divider = 10,
    /// The "Various Artists" compilation grouping.
    VariousArtists = 11,
}

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::BrowserNode)]
    pub struct BrowserNode {
        /// Display text for the row.
        #[property(get, set)]
        pub(super) label: RefCell<String>,
        /// Symbolic icon name shown beside the label.
        #[property(get, set)]
        pub(super) icon_name: RefCell<String>,
        /// Which [`BrowserNodeKind`] this row represents.
        #[property(get, set, builder(BrowserNodeKind::default()))]
        pub(super) kind: Cell<BrowserNodeKind>,
        /// Number of tracks under this node (0 when not applicable).
        #[property(get, set)]
        pub(super) track_count: Cell<i32>,
        /// Stable identity key (path / url / name) used for selection + lookup.
        #[property(get, set)]
        pub(super) key: RefCell<String>,
        /// Secondary text (album artist, station url, track duration, …).
        #[property(get, set)]
        pub(super) secondary: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BrowserNode {
        const NAME: &'static str = "BigMediaBrowserNode";
        type Type = super::BrowserNode;
        type ParentType = glib::Object;
    }

    #[glib::derived_properties]
    impl ObjectImpl for BrowserNode {}
}

glib::wrapper! {
    pub struct BrowserNode(ObjectSubclass<imp::BrowserNode>);
}

impl BrowserNode {
    /// A node with every field written at once. The setters look each
    /// property up by name and notify it; a 30 000-track Songs view built
    /// 180 000 of those on an Intel i3 before anything could bind to them.
    fn with_fields(
        kind: BrowserNodeKind,
        label: impl Into<String>,
        icon_name: &str,
        key: impl Into<String>,
        secondary: impl Into<String>,
        track_count: i32,
    ) -> Self {
        let browser_node: Self = glib::Object::new();
        let fields = browser_node.imp();
        fields.kind.set(kind);
        fields.label.replace(label.into());
        fields.icon_name.replace(icon_name.to_owned());
        fields.key.replace(key.into());
        fields.secondary.replace(secondary.into());
        fields.track_count.set(track_count);
        browser_node
    }

    /// "All tracks" root node with a track count.
    pub fn all(label: &str, count: i32) -> Self {
        Self::with_fields(
            BrowserNodeKind::All,
            label,
            "emblem-music-symbolic",
            "",
            "",
            count,
        )
    }

    /// Artist node; `key` is the artist name.
    pub fn artist(name: &str, count: i32) -> Self {
        Self::with_fields(
            BrowserNodeKind::Artist,
            name,
            "avatar-default-symbolic",
            name,
            "",
            count,
        )
    }

    /// Album node; `secondary` carries the album artist.
    pub fn album(name: &str, artist: &str, count: i32) -> Self {
        Self::with_fields(
            BrowserNodeKind::Album,
            name,
            "media-optical-symbolic",
            name,
            artist,
            count,
        )
    }

    /// Genre node; `key` is the genre name.
    pub fn genre(name: &str, count: i32) -> Self {
        Self::with_fields(
            BrowserNodeKind::Genre,
            name,
            "audio-x-generic-symbolic",
            name,
            "",
            count,
        )
    }

    /// Release-year node; `year <= 0` renders a blank label (unknown year).
    pub fn year(year: i32, count: i32) -> Self {
        let label = if year > 0 {
            year.to_string()
        } else {
            String::new()
        };
        Self::with_fields(
            BrowserNodeKind::Year,
            label,
            "x-office-calendar-symbolic",
            year.to_string(),
            "",
            count,
        )
    }

    /// Filesystem folder node; `key` is the folder path.
    pub fn folder(name: &str, path: &str) -> Self {
        Self::with_fields(
            BrowserNodeKind::Folder,
            name,
            "folder-symbolic",
            path,
            "",
            0,
        )
    }

    /// Plain file node; `key` is the file path.
    pub fn file_node(name: &str, path: &str) -> Self {
        Self::with_fields(
            BrowserNodeKind::File,
            name,
            "audio-x-generic-symbolic",
            path,
            "",
            0,
        )
    }

    /// Track node (a [`BrowserNodeKind::File`]) whose `secondary` line shows
    /// what the caller says of it (its duration, or album and artist).
    pub fn track_node(title: &str, path: &str, secondary: String) -> Self {
        Self::with_fields(
            BrowserNodeKind::File,
            title,
            "audio-x-generic-symbolic",
            path,
            secondary,
            0,
        )
    }

    /// Non-selectable alphabetical divider showing `letter`.
    pub fn divider(letter: &str) -> Self {
        Self::with_fields(BrowserNodeKind::Divider, letter, "", "", "", 0)
    }

    /// "Various Artists" compilation grouping node.
    pub fn various_artists(count: i32) -> Self {
        Self::with_fields(
            BrowserNodeKind::VariousArtists,
            i18n("Various Artists"),
            "system-users-symbolic",
            "__various_artists__",
            "",
            count,
        )
    }
}
