//! Multi-field tag-editor dialog for the `LibraryPanel` (push-model).
//!
//! A two-pane
//! `adw::NavigationSplitView`: the sidebar shows the album art + read-only file
//! info (path/format/bitrate/sample-rate/channels/size); the content pane shows
//! the editable metadata group (title/artist/album/album_artist/genre/year/
//! track#/disc#/comment) plus an optional MusicBrainz lookup button.
//!
//! PUSH-MODEL: the dialog NEVER touches the DB, mpv, or network. Save emits
//! [`LibraryPanelOutput::WriteTags`] — the shell does the off-thread file write +
//! DB row update + queue/view refresh. The MusicBrainz button emits
//! [`LibraryPanelOutput::MusicBrainzLookup`]; the shell does the blocking lookup
//! and pushes the result back through [`LibraryPanelInput::MusicBrainzLookupResolved`].
//! The art read still runs off-thread.

mod spec;
#[cfg(test)]
mod tests;

use adw::prelude::*;
use gtk::glib;
use relm4::ComponentSender;

use big_media_components::player_chrome::spawn_blocking_result;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
};

use crate::db::models::Track;
use crate::i18n::i18n;

use super::library_panel::{LibraryPanel, LibraryPanelOutput};
use spec::{
    TagEditFieldTextRefs, collect_tag_edit_fields, is_editable_text_changed, musicbrainz_fill_plan,
    read_only_metadata_rows, should_apply_musicbrainz_result, should_offer_musicbrainz_lookup,
};

/// Editable tag values collected from the dialog and shipped to the shell.
///
/// Strings are pre-trimmed; `None` means "clear this tag". `year` stays a string
/// (the writer parses it); numeric fields are parsed into `Option<u32>`/`<i32>`.
/// Mirrors `crate::library::metadata::TagEdits` so the shell can map 1:1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagEditFields {
    /// New title (`None` = clear).
    pub title: Option<String>,
    /// New artist (`None` = clear).
    pub artist: Option<String>,
    /// New album (`None` = clear).
    pub album: Option<String>,
    /// New album artist (`None` = clear).
    pub album_artist: Option<String>,
    /// New genre (`None` = clear).
    pub genre: Option<String>,
    /// New comment (`None` = clear).
    pub comment: Option<String>,
    /// New year as a raw string (`None`/empty = clear); the writer parses it.
    pub year: Option<String>,
    /// New track number (`None` = clear).
    pub track_number: Option<u32>,
    /// New disc number (`None` = clear).
    pub disc_number: Option<u32>,
}

impl TagEditFields {
    /// Year as the library stores it; text that is not a number means no year.
    pub(crate) fn year_number(&self) -> Option<i32> {
        self.year
            .as_deref()
            .and_then(|year| year.trim().parse().ok())
    }

    /// Track number as the library stores it; a value past `i32` means none
    /// instead of wrapping negative.
    pub(crate) fn track_number_i32(&self) -> Option<i32> {
        self.track_number.and_then(|n| i32::try_from(n).ok())
    }

    /// Disc number as the library stores it (see [`Self::track_number_i32`]).
    pub(crate) fn disc_number_i32(&self) -> Option<i32> {
        self.disc_number.and_then(|n| i32::try_from(n).ok())
    }
}

/// Weak handle used by the panel to push async MusicBrainz results into the
/// currently open editor dialog.
#[derive(Clone)]
pub(super) struct TagEditorHandle {
    track_id: i64,
    lookup_button: glib::WeakRef<gtk::Button>,
    genre_row: glib::WeakRef<adw::EntryRow>,
    year_row: glib::WeakRef<adw::EntryRow>,
    track_row: glib::WeakRef<adw::EntryRow>,
}

impl TagEditorHandle {
    pub(super) fn track_id(&self) -> i64 {
        self.track_id
    }

    pub(super) fn apply_musicbrainz_result(
        &self,
        metadata: Option<crate::library::musicbrainz::MbMetadata>,
    ) -> bool {
        let lookup_button = self.lookup_button.upgrade();
        let genre_row = self.genre_row.upgrade();
        let year_row = self.year_row.upgrade();
        let track_row = self.track_row.upgrade();
        if !should_apply_musicbrainz_result(
            lookup_button.is_some(),
            genre_row.is_some(),
            year_row.is_some(),
            track_row.is_some(),
        ) {
            return false;
        }
        let (Some(button), Some(genre_row), Some(year_row), Some(track_row)) =
            (lookup_button, genre_row, year_row, track_row)
        else {
            return false;
        };
        apply_musicbrainz_result(&button, &genre_row, &year_row, &track_row, metadata);
        true
    }
}

/// Build + present the tag-editor dialog for `track`. Save emits `WriteTags`;
/// MusicBrainz lookup emits `MusicBrainzLookup` and returns a result handle.
pub(super) fn show_tag_editor(
    parent: Option<&gtk::Window>,
    track: &Track,
    sender: &ComponentSender<LibraryPanel>,
) -> Option<TagEditorHandle> {
    const CONTENT_W: i32 = 900;
    const CONTENT_H: i32 = 640;

    let dialog = adw::Dialog::builder()
        .title(i18n("Track Info"))
        .content_width(CONTENT_W)
        .content_height(CONTENT_H)
        .build();

    let page = BigDialogPage::plain();
    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Save")));
    let cancel_button = footer.cancel_button().clone();
    let save_button = footer.accept_button().clone();
    save_button.set_sensitive(false);

    let split = adw::NavigationSplitView::new();
    split.set_hexpand(true);
    split.set_vexpand(true);
    split.set_min_sidebar_width(340.0);
    split.set_max_sidebar_width(420.0);
    split.set_sidebar(Some(&build_sidebar_page(track)));

    let rows = EditRows::new(track);
    let (content_page, editor_handle) = build_content_page(track, &rows, &save_button, sender);
    split.set_content(Some(&content_page));
    page.content().append(&split);
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(&page.into_root()));

    {
        // WEAK: the cancel button lives in the dialog — a strong clone is a
        // dialog ⇄ button ⇄ closure cycle that leaks the dialog after close.
        let dialog = dialog.downgrade();
        cancel_button.connect_clicked(move |_| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        });
    }
    wire_save(&dialog, &save_button, track, &rows, sender);

    dialog.present(parent);
    editor_handle
}

/// The editable metadata rows, kept together so save + dirty detection can read
/// them without threading nine clones through every call.
#[derive(Clone)]
struct EditRows {
    title: adw::EntryRow,
    artist: adw::EntryRow,
    album: adw::EntryRow,
    album_artist: adw::EntryRow,
    genre: adw::EntryRow,
    year: adw::EntryRow,
    track_number: adw::EntryRow,
    disc_number: adw::EntryRow,
    comment: adw::EntryRow,
}

impl EditRows {
    fn new(track: &Track) -> Self {
        let entry =
            |title: String, text: String| adw::EntryRow::builder().title(title).text(text).build();
        let num = |n: Option<i32>| n.map_or_else(String::new, |n| n.to_string());
        Self {
            title: entry(i18n("Title"), track.title.clone().unwrap_or_default()),
            artist: entry(i18n("Artist"), track.artist.clone().unwrap_or_default()),
            album: entry(i18n("Album"), track.album.clone().unwrap_or_default()),
            album_artist: entry(
                i18n("Album Artist"),
                track.album_artist.clone().unwrap_or_default(),
            ),
            genre: entry(i18n("Genre"), track.genre.clone().unwrap_or_default()),
            year: entry(i18n("Year"), num(track.year)),
            track_number: entry(i18n("Track #"), num(track.track_number)),
            disc_number: entry(i18n("Disc #"), num(track.disc_number)),
            comment: entry(i18n("Comment"), String::new()),
        }
    }

    /// Rows in display order, for dirty detection.
    fn editables(&self) -> [gtk::Editable; 9] {
        [
            self.title.clone().upcast(),
            self.artist.clone().upcast(),
            self.album.clone().upcast(),
            self.album_artist.clone().upcast(),
            self.genre.clone().upcast(),
            self.year.clone().upcast(),
            self.track_number.clone().upcast(),
            self.disc_number.clone().upcast(),
            self.comment.clone().upcast(),
        ]
    }

    /// Collect the current values into a shell-bound `TagEditFields`.
    fn collect(&self) -> TagEditFields {
        let title = self.title.text();
        let artist = self.artist.text();
        let album = self.album.text();
        let album_artist = self.album_artist.text();
        let genre = self.genre.text();
        let comment = self.comment.text();
        let year = self.year.text();
        let track_number = self.track_number.text();
        let disc_number = self.disc_number.text();
        collect_tag_edit_fields(TagEditFieldTextRefs {
            title: title.as_str(),
            artist: artist.as_str(),
            album: album.as_str(),
            album_artist: album_artist.as_str(),
            genre: genre.as_str(),
            comment: comment.as_str(),
            year: year.as_str(),
            track_number: track_number.as_str(),
            disc_number: disc_number.as_str(),
        })
    }
}

/// Sidebar page: album art (off-thread) + read-only file info.
fn build_sidebar_page(track: &Track) -> adw::NavigationPage {
    let sidebar_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    sidebar_box.append(&build_art_box(&track.file_path));
    append_read_only_metadata_rows(&sidebar_box, track);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .vexpand(true)
        .child(&sidebar_box)
        .build();

    adw::NavigationPage::builder()
        .title(i18n("File Info"))
        .child(&scroll)
        .build()
}

/// Edge of the album-art picture in logical points; also the decode bound.
const ART_EDGE_PX: i32 = 120;

/// Centered album-art picture, filled off-thread from the embedded cover art.
fn build_art_box(file_path: &str) -> gtk::Box {
    let art_box = gtk::Box::builder()
        .halign(gtk::Align::Center)
        .margin_bottom(4)
        .build();
    let picture = gtk::Picture::builder()
        .width_request(ART_EDGE_PX)
        .height_request(ART_EDGE_PX)
        .content_fit(gtk::ContentFit::Cover)
        .build();
    picture.add_css_class("card");
    picture.update_property(&[gtk::accessible::Property::Label(&i18n("Album art"))]);

    let path = file_path.to_string();
    let pic = picture.clone();
    spawn_blocking_result(
        move || crate::library::metadata::extract_album_art(&path),
        move |bytes| {
            let Some(data) = bytes else { return };
            // Bounded by the widget, not by the file: a 3000² embedded cover
            // decoded whole would sit in memory at ~36 MiB to fill 120 points.
            let edge = ART_EDGE_PX * pic.scale_factor().max(1);
            if let Some(texture) =
                big_media_components::media::images::bytes_to_thumbnail(&data, edge)
            {
                pic.set_paintable(Some(&texture));
            }
        },
    );
    art_box.append(&picture);
    art_box
}

/// Read-only file metadata rows (label/value pairs) appended to the sidebar.
fn append_read_only_metadata_rows(sidebar_box: &gtk::Box, track: &Track) {
    for row in read_only_metadata_rows(track) {
        append_metadata_value_row(sidebar_box, &i18n(row.label), &row.value, row.is_file_path);
    }
}

/// One read-only "caption header + value" pair; `path` makes the value
/// char-wrapping + selectable (for long file paths).
fn append_metadata_value_row(sidebar_box: &gtk::Box, label: &str, value: &str, path: bool) {
    let field_label = gtk::Label::builder().label(label).xalign(0.0).build();
    field_label.add_css_class("caption");
    field_label.add_css_class("dim-label");
    sidebar_box.append(&field_label);

    let val = gtk::Label::builder()
        .label(value)
        .xalign(0.0)
        .wrap(true)
        .build();
    if path {
        val.set_wrap_mode(gtk::pango::WrapMode::Char);
        val.set_selectable(true);
        val.add_css_class("caption");
    }
    sidebar_box.append(&val);
}

/// Content page: editable metadata group + dirty detection + MusicBrainz button.
fn build_content_page(
    track: &Track,
    rows: &EditRows,
    save_button: &gtk::Button,
    sender: &ComponentSender<LibraryPanel>,
) -> (adw::NavigationPage, Option<TagEditorHandle>) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(16)
        .build();

    let group = BigPreferencesGroupSpec::new(i18n("Metadata")).build();
    group.add(&rows.title);
    group.add(&rows.artist);
    group.add(&rows.album);
    group.add(&rows.album_artist);
    group.add(&rows.genre);
    group.add(&rows.year);
    group.add(&rows.track_number);
    group.add(&rows.disc_number);
    group.add(&rows.comment);
    content.append(&group);

    connect_dirty_detection(save_button, rows, track);
    let editor_handle = maybe_add_musicbrainz_lookup(&content, track, rows, sender);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .vexpand(true)
        .child(&content)
        .build();

    let page = adw::NavigationPage::builder()
        .title(i18n("Track Info"))
        .child(&scroll)
        .build();
    (page, editor_handle)
}

/// Enable Save only when at least one editable row differs from its original value.
fn connect_dirty_detection(save_button: &gtk::Button, rows: &EditRows, track: &Track) {
    let num = |n: Option<i32>| n.map_or_else(String::new, |n| n.to_string());
    let originals: [String; 9] = [
        track.title.clone().unwrap_or_default(),
        track.artist.clone().unwrap_or_default(),
        track.album.clone().unwrap_or_default(),
        track.album_artist.clone().unwrap_or_default(),
        track.genre.clone().unwrap_or_default(),
        num(track.year),
        num(track.track_number),
        num(track.disc_number),
        String::new(), // comment starts empty
    ];
    let editables = rows.editables();
    let pairs: [(gtk::Editable, String); 9] =
        std::array::from_fn(|i| (editables[i].clone(), originals[i].clone()));

    let recheck = {
        let save_button = save_button.clone();
        std::rc::Rc::new(move || {
            let dirty = pairs
                .iter()
                .any(|(row, orig)| is_editable_text_changed(row.text().as_str(), orig.as_str()));
            save_button.set_sensitive(dirty);
        })
    };
    for editable in editables {
        let recheck = std::rc::Rc::clone(&recheck);
        editable.connect_changed(move |_| recheck());
    }
}

/// Save → collect fields → emit `WriteTags` → close. The shell owns the write.
fn wire_save(
    dialog: &adw::Dialog,
    save_button: &gtk::Button,
    track: &Track,
    rows: &EditRows,
    sender: &ComponentSender<LibraryPanel>,
) {
    // WEAK: the save button lives in the dialog — a strong clone is a
    // dialog ⇄ button ⇄ closure cycle that leaks the dialog after close.
    let dialog = dialog.downgrade();
    let rows = rows.clone();
    let out = sender.clone();
    let track_id = track.id;
    let file_path = track.file_path.clone();
    save_button.connect_clicked(move |button| {
        button.set_sensitive(false);
        let _ = out.output(LibraryPanelOutput::WriteTags {
            track_id,
            file_path: file_path.clone(),
            fields: rows.collect(),
        });
        if let Some(dialog) = dialog.upgrade() {
            dialog.close();
        }
    });
}

/// Add the MusicBrainz lookup button only when a fillable field is missing and
/// the artist is known. Click emits `MusicBrainzLookup`; the shell replies with
/// a `MusicBrainzLookupResolved` input.
fn maybe_add_musicbrainz_lookup(
    content: &gtk::Box,
    track: &Track,
    rows: &EditRows,
    sender: &ComponentSender<LibraryPanel>,
) -> Option<TagEditorHandle> {
    let should_add_lookup = should_offer_musicbrainz_lookup(
        track.artist.is_some(),
        track.genre.is_some(),
        track.year.is_some(),
        track.track_number.is_some(),
    );
    if should_add_lookup {
        let musicbrainz_lookup_button = gtk::Button::builder()
            .label(i18n("Lookup from MusicBrainz"))
            .halign(gtk::Align::Center)
            .margin_top(4)
            .build();
        musicbrainz_lookup_button.add_css_class("pill");

        let out = sender.clone();
        let track_id = track.id;
        let track_for_lookup = track.clone();
        musicbrainz_lookup_button.connect_clicked(move |button| {
            let _ = out.output(LibraryPanelOutput::MusicBrainzLookup(
                track_for_lookup.clone(),
            ));
            button.set_sensitive(false);
            button.set_label(&i18n("Searching…"));
        });
        content.append(&musicbrainz_lookup_button);
        Some(TagEditorHandle {
            track_id,
            lookup_button: musicbrainz_lookup_button.downgrade(),
            genre_row: rows.genre.downgrade(),
            year_row: rows.year.downgrade(),
            track_row: rows.track_number.downgrade(),
        })
    } else {
        None
    }
}

/// Fill only the empty MusicBrainz-eligible rows; update the button to report
/// what (if anything) was found.
fn apply_musicbrainz_result(
    lookup_button: &gtk::Button,
    genre_row: &adw::EntryRow,
    year_row: &adw::EntryRow,
    track_row: &adw::EntryRow,
    meta: Option<crate::library::musicbrainz::MbMetadata>,
) {
    let genre_text = genre_row.text();
    let year_text = year_row.text();
    let track_number_text = track_row.text();
    let plan = musicbrainz_fill_plan(
        genre_text.as_str(),
        year_text.as_str(),
        track_number_text.as_str(),
        meta.as_ref(),
    );

    if let Some(genre) = plan.genre {
        genre_row.set_text(&genre);
    }
    if let Some(year) = plan.year {
        year_row.set_text(&year);
    }
    if let Some(track_number) = plan.track_number {
        track_row.set_text(&track_number);
    }
    lookup_button.set_label(&plan.lookup_button_label);
}
