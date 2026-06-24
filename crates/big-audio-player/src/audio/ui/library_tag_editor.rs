//! Multi-field tag-editor dialog for the `LibraryPanel` (push-model).
//!
//! Ported from `audio::window::detail_dialogs::{tag_editor,fields}`. A two-pane
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

use adw::prelude::*;
use gtk::glib;
use relm4::ComponentSender;

use big_media_components::player_chrome::spawn_blocking_result;

use crate::db::models::Track;
use crate::i18n::i18n;

use super::library_panel::{LibraryPanel, LibraryPanelOutput};

/// Editable tag values collected from the dialog and shipped to the shell.
///
/// Strings are pre-trimmed; `None` means "clear this tag". `year` stays a string
/// (the writer parses it); numeric fields are parsed into `Option<u32>`/`<i32>`.
/// Mirrors `crate::library::metadata::TagEdits` so the shell can map 1:1.
#[derive(Debug, Clone, Default)]
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
        let (Some(button), Some(genre_row), Some(year_row), Some(track_row)) = (
            self.lookup_button.upgrade(),
            self.genre_row.upgrade(),
            self.year_row.upgrade(),
            self.track_row.upgrade(),
        ) else {
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
    const DEFAULT_W: i32 = 900;
    const DEFAULT_H: i32 = 750;

    let dialog = adw::Window::builder()
        .title(i18n("Track Info"))
        .default_width(DEFAULT_W)
        .default_height(DEFAULT_H)
        .modal(true)
        .build();
    if let Some(parent) = parent {
        dialog.set_transient_for(Some(parent));
    }

    let split = adw::NavigationSplitView::new();
    split.set_min_sidebar_width(340.0);
    split.set_max_sidebar_width(420.0);
    split.set_sidebar(Some(&build_sidebar_page(track)));

    let rows = EditRows::new(track);
    let (cancel_btn, save_btn) = build_dialog_actions();
    let (content_page, editor_handle) =
        build_content_page(track, &rows, &cancel_btn, &save_btn, sender);
    split.set_content(Some(&content_page));
    dialog.set_content(Some(&split));

    {
        // WEAK: the cancel button lives in the dialog window — a strong clone is
        // a window ⇄ button ⇄ closure cycle that leaks the dialog after close.
        let dialog = dialog.downgrade();
        cancel_btn.connect_clicked(move |_| {
            if let Some(dialog) = dialog.upgrade() {
                dialog.close();
            }
        });
    }
    wire_save(&dialog, &save_btn, track, &rows, sender);

    dialog.present();
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
        let year_str = self.year.text().trim().to_string();
        TagEditFields {
            title: optional_entry_text(self.title.text().as_str()),
            artist: optional_entry_text(self.artist.text().as_str()),
            album: optional_entry_text(self.album.text().as_str()),
            album_artist: optional_entry_text(self.album_artist.text().as_str()),
            genre: optional_entry_text(self.genre.text().as_str()),
            comment: optional_entry_text(self.comment.text().as_str()),
            year: if year_str.is_empty() {
                None
            } else {
                Some(year_str)
            },
            track_number: self.track_number.text().trim().parse().ok(),
            disc_number: self.disc_number.text().trim().parse().ok(),
        }
    }
}

/// Trim and collapse blank entry text to `None` (a cleared tag).
fn optional_entry_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Cancel + Save header buttons (hidden until a field goes dirty).
fn build_dialog_actions() -> (gtk::Button, gtk::Button) {
    let cancel_btn = gtk::Button::builder()
        .label(i18n("Cancel"))
        .visible(false)
        .build();
    let save_btn = gtk::Button::builder()
        .label(i18n("Save"))
        .visible(false)
        .build();
    save_btn.add_css_class("suggested-action");
    (cancel_btn, save_btn)
}

/// Sidebar page: album art (off-thread) + read-only file info.
fn build_sidebar_page(track: &Track) -> adw::NavigationPage {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::builder()
        .show_end_title_buttons(false)
        .build();
    header.set_title_widget(Some(&adw::WindowTitle::new(&i18n("File Info"), "")));
    toolbar.add_top_bar(&header);

    let sidebar_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    sidebar_box.append(&build_art_box(&track.file_path));
    append_read_only_info(&sidebar_box, track);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&sidebar_box)
        .build();
    toolbar.set_content(Some(&scroll));

    adw::NavigationPage::builder()
        .title(i18n("File Info"))
        .child(&toolbar)
        .build()
}

/// Centered album-art picture, filled off-thread from the embedded cover art.
fn build_art_box(file_path: &str) -> gtk::Box {
    let art_box = gtk::Box::builder()
        .halign(gtk::Align::Center)
        .margin_bottom(4)
        .build();
    let picture = gtk::Picture::builder()
        .width_request(120)
        .height_request(120)
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
            if let Some(texture) = big_media_components::media::images::bytes_to_texture(&data) {
                pic.set_paintable(Some(&texture));
            }
        },
    );
    art_box.append(&picture);
    art_box
}

/// Read-only file-info rows (label/value pairs) appended to the sidebar.
fn append_read_only_info(sidebar_box: &gtk::Box, track: &Track) {
    let ro_fields: [(&str, String); 6] = [
        ("Duration", track.duration_display()),
        (
            "Bitrate",
            track.bitrate.map_or_else(
                || "-".into(),
                |bitrate| {
                    big_media_components::audio::labels::bitrate_kbps_label_from_bps(bitrate.into())
                },
            ),
        ),
        (
            "Sample Rate",
            track
                .sample_rate
                .map_or_else(|| "-".into(), |rate| format!("{rate} Hz")),
        ),
        (
            "Channels",
            track
                .channels
                .map_or_else(|| "-".into(), |channels| channels.to_string()),
        ),
        ("Format", track.format.as_deref().unwrap_or("-").to_string()),
        (
            "File Size",
            track.file_size.map_or_else(
                || "-".into(),
                |size| big_media_components::bytes::decimal_compact(size.max(0) as u64),
            ),
        ),
    ];

    for (label, value) in &ro_fields {
        append_info_row(sidebar_box, &i18n(label), value, false);
    }
    append_info_row(sidebar_box, &i18n("File"), &track.file_path, true);
}

/// One read-only "caption header + value" pair; `path` makes the value
/// char-wrapping + selectable (for long file paths).
fn append_info_row(sidebar_box: &gtk::Box, label: &str, value: &str, path: bool) {
    let lbl = gtk::Label::builder().label(label).xalign(0.0).build();
    lbl.add_css_class("caption");
    lbl.add_css_class("dim-label");
    sidebar_box.append(&lbl);

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
    cancel_btn: &gtk::Button,
    save_btn: &gtk::Button,
    sender: &ComponentSender<LibraryPanel>,
) -> (adw::NavigationPage, Option<TagEditorHandle>) {
    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new(&i18n("Track Info"), "")));
    header.pack_start(cancel_btn);
    header.pack_end(save_btn);
    toolbar.add_top_bar(&header);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(16)
        .build();

    let group = adw::PreferencesGroup::builder()
        .title(i18n("Metadata"))
        .build();
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

    connect_dirty_detection(save_btn, cancel_btn, rows, track);
    let editor_handle = maybe_add_musicbrainz_lookup(&content, track, rows, sender);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&content)
        .build();
    toolbar.set_content(Some(&scroll));

    let page = adw::NavigationPage::builder()
        .title(i18n("Track Info"))
        .child(&toolbar)
        .build();
    (page, editor_handle)
}

/// Show Save/Cancel only when at least one editable row differs from its
/// original value.
fn connect_dirty_detection(
    save_btn: &gtk::Button,
    cancel_btn: &gtk::Button,
    rows: &EditRows,
    track: &Track,
) {
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
        let save_btn = save_btn.clone();
        let cancel_btn = cancel_btn.clone();
        std::rc::Rc::new(move || {
            let dirty = pairs
                .iter()
                .any(|(row, orig)| row.text().as_str() != orig.as_str());
            save_btn.set_visible(dirty);
            cancel_btn.set_visible(dirty);
        })
    };
    for editable in editables {
        let recheck = std::rc::Rc::clone(&recheck);
        editable.connect_changed(move |_| recheck());
    }
}

/// Save → collect fields → emit `WriteTags` → close. The shell owns the write.
fn wire_save(
    dialog: &adw::Window,
    save_btn: &gtk::Button,
    track: &Track,
    rows: &EditRows,
    sender: &ComponentSender<LibraryPanel>,
) {
    // WEAK: the save button lives in the dialog window — a strong clone is a
    // window ⇄ button ⇄ closure cycle that leaks the dialog after close.
    let dialog = dialog.downgrade();
    let rows = rows.clone();
    let out = sender.clone();
    let track_id = track.id;
    let file_path = track.file_path.clone();
    save_btn.connect_clicked(move |btn| {
        btn.set_sensitive(false);
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
    let has_missing = track.genre.is_none() || track.year.is_none() || track.track_number.is_none();
    if !has_missing || track.artist.is_none() {
        return None;
    }

    let mb_btn = gtk::Button::builder()
        .label(i18n("Lookup from MusicBrainz"))
        .halign(gtk::Align::Center)
        .margin_top(4)
        .build();
    mb_btn.add_css_class("pill");

    let out = sender.clone();
    let track_id = track.id;
    let track_for_lookup = track.clone();
    mb_btn.connect_clicked(move |btn| {
        let _ = out.output(LibraryPanelOutput::MusicBrainzLookup(
            track_for_lookup.clone(),
        ));
        btn.set_sensitive(false);
        btn.set_label(&i18n("Searching…"));
    });
    content.append(&mb_btn);
    Some(TagEditorHandle {
        track_id,
        lookup_button: mb_btn.downgrade(),
        genre_row: rows.genre.downgrade(),
        year_row: rows.year.downgrade(),
        track_row: rows.track_number.downgrade(),
    })
}

/// Fill only the empty MusicBrainz-eligible rows; update the button to report
/// what (if anything) was found.
fn apply_musicbrainz_result(
    btn: &gtk::Button,
    genre_row: &adw::EntryRow,
    year_row: &adw::EntryRow,
    track_row: &adw::EntryRow,
    meta: Option<crate::library::musicbrainz::MbMetadata>,
) {
    let Some(meta) = meta else {
        btn.set_label(&i18n("Not found"));
        return;
    };
    let mut filled: Vec<&str> = Vec::new();
    if genre_row.text().is_empty() {
        if let Some(ref genre) = meta.genre {
            genre_row.set_text(genre);
            filled.push("Genre");
        }
    }
    if year_row.text().is_empty() {
        if let Some(year) = meta.year {
            year_row.set_text(&year.to_string());
            filled.push("Year");
        }
    }
    if track_row.text().is_empty() {
        if let Some(track_number) = meta.track_number {
            track_row.set_text(&track_number.to_string());
            filled.push("Track #");
        }
    }
    if filled.is_empty() {
        btn.set_label(&i18n("No new data found"));
    } else {
        btn.set_label(&format!("✓ {}", filled.join(", ")));
    }
}

#[cfg(test)]
mod tests {
    use super::optional_entry_text;

    #[test]
    fn optional_entry_text_discards_blank_values() {
        assert_eq!(optional_entry_text(""), None);
        assert_eq!(optional_entry_text("   "), None);
    }

    #[test]
    fn optional_entry_text_trims_real_values() {
        assert_eq!(optional_entry_text("  album  "), Some("album".to_string()));
    }
}
