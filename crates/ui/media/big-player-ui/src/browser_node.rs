//! `BigMediaBrowserNode`: generic GObject node type for a media library /
//! file-browser tree — covers folder/file/divider plus the music-library
//! flavours (artist/album/genre/year/station/playlist). Reusable by any media
//! app; the constructors are convenience factories, not a fixed taxonomy.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::cell::{Cell, RefCell};

use big_media_data::i18n::i18n;

/// Category of a [`BrowserNode`] — what the row represents in the library/file tree.
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
        label: RefCell<String>,
        /// Symbolic icon name shown beside the label.
        #[property(get, set)]
        icon_name: RefCell<String>,
        /// Which [`BrowserNodeKind`] this row represents.
        #[property(get, set, builder(BrowserNodeKind::default()))]
        kind: Cell<BrowserNodeKind>,
        /// Number of tracks under this node (0 when not applicable).
        #[property(get, set)]
        track_count: Cell<i32>,
        /// Stable identity key (path / url / name) used for selection + lookup.
        #[property(get, set)]
        key: RefCell<String>,
        /// Secondary text (album artist, station url, track duration, …).
        #[property(get, set)]
        secondary: RefCell<String>,
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
    /// "All tracks" root node with a track count.
    pub fn all(label: &str, count: i32) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        browser_node.set_label(label);
        browser_node.set_icon_name("emblem-music-symbolic");
        browser_node.set_kind(BrowserNodeKind::All);
        browser_node.set_track_count(count);
        browser_node
    }

    /// Artist node; `key` is the artist name.
    pub fn artist(name: &str, count: i32) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        browser_node.set_label(name);
        browser_node.set_icon_name("avatar-default-symbolic");
        browser_node.set_kind(BrowserNodeKind::Artist);
        browser_node.set_track_count(count);
        browser_node.set_key(name);
        browser_node
    }

    /// Album node; `secondary` carries the album artist.
    pub fn album(name: &str, artist: &str, count: i32) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        browser_node.set_label(name);
        browser_node.set_icon_name("media-optical-symbolic");
        browser_node.set_kind(BrowserNodeKind::Album);
        browser_node.set_track_count(count);
        browser_node.set_key(name);
        browser_node.set_secondary(artist);
        browser_node
    }

    /// Genre node; `key` is the genre name.
    pub fn genre(name: &str, count: i32) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        browser_node.set_label(name);
        browser_node.set_icon_name("audio-x-generic-symbolic");
        browser_node.set_kind(BrowserNodeKind::Genre);
        browser_node.set_track_count(count);
        browser_node.set_key(name);
        browser_node
    }

    /// Release-year node; `year <= 0` renders a blank label (unknown year).
    pub fn year(year: i32, count: i32) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        let label = if year > 0 {
            year.to_string()
        } else {
            String::new()
        };
        browser_node.set_label(label);
        browser_node.set_icon_name("x-office-calendar-symbolic");
        browser_node.set_kind(BrowserNodeKind::Year);
        browser_node.set_track_count(count);
        browser_node.set_key(year.to_string());
        browser_node
    }

    /// Filesystem folder node; `key` is the folder path.
    pub fn folder(name: &str, path: &str) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        browser_node.set_label(name);
        browser_node.set_icon_name("folder-symbolic");
        browser_node.set_kind(BrowserNodeKind::Folder);
        browser_node.set_key(path);
        browser_node
    }

    /// Radio-station node; `key` and `secondary` both carry the stream url.
    pub fn station(name: &str, url: &str) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        browser_node.set_label(name);
        browser_node.set_icon_name("icon_radio");
        browser_node.set_kind(BrowserNodeKind::Station);
        browser_node.set_key(url);
        browser_node.set_secondary(url);
        browser_node
    }

    /// User-playlist node; `key` is the playlist name.
    pub fn playlist(name: &str, count: i32) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        browser_node.set_label(name);
        browser_node.set_icon_name("playlist-symbolic");
        browser_node.set_kind(BrowserNodeKind::Playlist);
        browser_node.set_track_count(count);
        browser_node.set_key(name);
        browser_node
    }

    /// Plain file node; `key` is the file path.
    pub fn file_node(name: &str, path: &str) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        browser_node.set_label(name);
        browser_node.set_icon_name("audio-x-generic-symbolic");
        browser_node.set_kind(BrowserNodeKind::File);
        browser_node.set_key(path);
        browser_node
    }

    /// Track node (a [`BrowserNodeKind::File`]) whose `secondary` shows the
    /// formatted playback duration.
    pub fn track_node(title: &str, path: &str, duration_secs: f64) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        browser_node.set_label(title);
        browser_node.set_icon_name("audio-x-generic-symbolic");
        browser_node.set_kind(BrowserNodeKind::File);
        browser_node.set_key(path);
        browser_node.set_secondary(big_media_components::time::playback_time(duration_secs));
        browser_node
    }

    /// Non-selectable alphabetical divider showing `letter`.
    pub fn divider(letter: &str) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        browser_node.set_label(letter);
        browser_node.set_kind(BrowserNodeKind::Divider);
        browser_node
    }

    /// "Various Artists" compilation grouping node.
    pub fn various_artists(count: i32) -> Self {
        let browser_node: Self = glib::Object::builder().build();
        browser_node.set_label(i18n("Various Artists"));
        browser_node.set_icon_name("system-users-symbolic");
        browser_node.set_kind(BrowserNodeKind::VariousArtists);
        browser_node.set_track_count(count);
        browser_node.set_key("__various_artists__");
        browser_node
    }
}
