//! `GroupBy` enum: column key used by the library browser to fan tracks into nodes.
//! Knows its DB column, display name, icon, and the [`BrowserNodeKind`] it maps to.

use crate::i18n::i18n;

use crate::audio::browser_node::BrowserNodeKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupBy {
    #[default]
    None,
    Artist,
    AlbumArtist,
    Album,
    Genre,
    Year,
    Format,
}

impl GroupBy {
    /// Column name in the `tracks` table.
    #[must_use]
    pub fn column(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Artist => "artist",
            Self::AlbumArtist => "album_artist",
            Self::Album => "album",
            Self::Genre => "genre",
            Self::Year => "year",
            Self::Format => "format",
        }
    }

    #[must_use]
    pub fn display_name(self) -> String {
        match self {
            Self::None => i18n("None"),
            Self::Artist => i18n("Artist"),
            Self::AlbumArtist => i18n("Album Artist"),
            Self::Album => i18n("Album"),
            Self::Genre => i18n("Genre"),
            Self::Year => i18n("Year"),
            Self::Format => i18n("Format"),
        }
    }

    #[must_use]
    pub fn icon_name(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Artist | Self::AlbumArtist => "avatar-default-symbolic",
            Self::Album => "media-optical-symbolic",
            Self::Genre => "audio-x-generic-symbolic",
            Self::Year => "x-office-calendar-symbolic",
            Self::Format => "document-properties-symbolic",
        }
    }

    #[must_use]
    pub fn node_kind(self) -> BrowserNodeKind {
        match self {
            Self::None => BrowserNodeKind::All,
            Self::Artist | Self::AlbumArtist => BrowserNodeKind::Artist,
            Self::Album => BrowserNodeKind::Album,
            Self::Genre => BrowserNodeKind::Genre,
            Self::Year => BrowserNodeKind::Year,
            // File reused for format groups: same leaf semantics, different icon.
            Self::Format => BrowserNodeKind::File,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Artist => "artist",
            Self::AlbumArtist => "album_artist",
            Self::Album => "album",
            Self::Genre => "genre",
            Self::Year => "year",
            Self::Format => "format",
        }
    }

    #[must_use]
    pub fn from_str_token(s: &str) -> Self {
        match s {
            "artist" => Self::Artist,
            "album_artist" => Self::AlbumArtist,
            "album" => Self::Album,
            "genre" => Self::Genre,
            "year" => Self::Year,
            "format" => Self::Format,
            _ => Self::None,
        }
    }
}
