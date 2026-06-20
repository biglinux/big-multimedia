//! `BigAudioTrackObject`: GObject wrapper around `db::models::Track` for
//! ColumnView/ListView models that need property bindings.

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use std::cell::{Cell, RefCell};

use crate::db::models::Track;
use crate::shared::ui_helpers::format_duration_hms;

mod imp {
    use super::*;

    #[derive(Default, glib::Properties)]
    #[properties(wrapper_type = super::TrackObject)]
    pub struct TrackObject {
        #[property(get, set)]
        db_id: Cell<i64>,
        #[property(get, set)]
        title: RefCell<String>,
        #[property(get, set)]
        artist: RefCell<String>,
        #[property(get, set)]
        album: RefCell<String>,
        #[property(get, set)]
        album_artist: RefCell<String>,
        #[property(get, set)]
        genre: RefCell<String>,
        #[property(get, set)]
        format: RefCell<String>,
        #[property(get, set)]
        file_path: RefCell<String>,
        #[property(get, set)]
        duration_secs: Cell<f64>,
        #[property(get, set)]
        track_number: Cell<i32>,
        #[property(get, set)]
        disc_number: Cell<i32>,
        #[property(get, set)]
        year: Cell<i32>,
        #[property(get, set)]
        bitrate: Cell<i32>,
        #[property(get, set)]
        sample_rate: Cell<i32>,
        #[property(get, set)]
        channels: Cell<i32>,
        #[property(get, set)]
        file_size: Cell<i64>,
        #[property(get, set)]
        play_count: Cell<i32>,
        #[property(get, set)]
        rating: Cell<i32>,
        #[property(get, set)]
        date_added: Cell<i64>,
        #[property(get, set)]
        is_playing: Cell<bool>,
        #[property(get, set)]
        art_hash: RefCell<String>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TrackObject {
        const NAME: &'static str = "BigAudioTrackObject";
        type Type = super::TrackObject;
        type ParentType = glib::Object;
    }

    #[glib::derived_properties]
    impl ObjectImpl for TrackObject {}
}

glib::wrapper! {
    pub struct TrackObject(ObjectSubclass<imp::TrackObject>);
}

impl TrackObject {
    pub fn new(track: &Track) -> Self {
        let obj: Self = glib::Object::builder().build();
        obj.set_db_id(track.id);
        obj.set_title(track.display_title());
        obj.set_artist(track.display_artist());
        obj.set_album(track.display_album());
        obj.set_album_artist(
            track
                .album_artist
                .as_deref()
                .unwrap_or_default(),
        );
        obj.set_genre(track.genre.as_deref().unwrap_or_default());
        obj.set_format(track.format.as_deref().unwrap_or_default());
        obj.set_file_path(&track.file_path);
        obj.set_duration_secs(track.duration_secs);
        obj.set_track_number(track.track_number.unwrap_or(0));
        obj.set_disc_number(track.disc_number.unwrap_or(0));
        obj.set_year(track.year.unwrap_or(0));
        obj.set_bitrate(track.bitrate.unwrap_or(0));
        obj.set_sample_rate(track.sample_rate.unwrap_or(0));
        obj.set_channels(track.channels.unwrap_or(0));
        obj.set_file_size(track.file_size.unwrap_or(0));
        obj.set_play_count(track.play_count);
        obj.set_rating(track.rating);
        obj.set_date_added(track.date_added);
        obj.set_art_hash(track.art_hash.as_deref().unwrap_or_default());
        obj
    }

    pub fn from_path(path: &str) -> Self {
        let filename = std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let obj: Self = glib::Object::builder().build();
        obj.set_title(filename);
        obj.set_file_path(path.to_string());
        obj
    }

    pub fn duration_display(&self) -> String {
        format_duration_hms(self.duration_secs())
    }

    pub fn to_track(&self) -> Track {
        Track {
            id: self.db_id(),
            file_path: self.file_path(),
            title: Some(self.title()).filter(|s| !s.is_empty()),
            artist: Some(self.artist()).filter(|s| !s.is_empty()),
            album: Some(self.album()).filter(|s| !s.is_empty()),
            album_artist: Some(self.album_artist()).filter(|s| !s.is_empty()),
            track_number: {
                let n = self.track_number();
                if n > 0 {
                    Some(n)
                } else {
                    None
                }
            },
            disc_number: {
                let n = self.disc_number();
                if n > 0 {
                    Some(n)
                } else {
                    None
                }
            },
            year: {
                let y = self.year();
                if y > 0 {
                    Some(y)
                } else {
                    None
                }
            },
            genre: Some(self.genre()).filter(|s| !s.is_empty()),
            duration_secs: self.duration_secs(),
            file_size: {
                let s = self.file_size();
                if s > 0 {
                    Some(s)
                } else {
                    None
                }
            },
            bitrate: {
                let b = self.bitrate();
                if b > 0 {
                    Some(b)
                } else {
                    None
                }
            },
            sample_rate: {
                let r = self.sample_rate();
                if r > 0 {
                    Some(r)
                } else {
                    None
                }
            },
            channels: {
                let c = self.channels();
                if c > 0 {
                    Some(c)
                } else {
                    None
                }
            },
            format: Some(self.format()).filter(|s| !s.is_empty()),
            art_hash: Some(self.art_hash()).filter(|s| !s.is_empty()),
            date_added: self.date_added(),
            last_played: None,
            play_count: self.play_count(),
            rating: self.rating(),
            modified_at: None,
        }
    }
}
