//! The Files tab's library: the person's video folders (Videos, Downloads and
//! the home folder unless chosen otherwise, plus removable drives while they
//! are plugged in), a search across them, and the page to choose them.
//!
//! BigSearch's index daemon answers when it runs: a search is one name query
//! and a folder's count one `list` query. Folders it does not index, or no
//! daemon at all, are scanned here instead, once per folder per session.
//!
//! The protocol is one JSON line each way over the user's own Unix socket
//! (`$XDG_RUNTIME_DIR/biglinux/indexd.sock`, as `big-indexd-client` defines
//! it); only the requests and reply fields used here are spoken, and replies
//! are data from another process: only absolute paths to videos that are
//! there now are shown.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use super::{BrowserState, VideoBrowserSidebarOutput};
use crate::i18n::{i18n, ni18n};
use crate::media::{playlist, thumbnail};
use crate::settings::keys;

/// Search results listed; a search that wants more is better narrowed.
const SHOWN: usize = 40;
/// `.ts` files read to tell a transport stream from TypeScript, per folder.
const TS_READ: usize = 200;
/// A daemon that does not answer quickly is left out, not waited on.
const TIMEOUT: Duration = Duration::from_secs(3);

// ── folders ────────────────────────────────────────────────────────────────

/// The folders the library shows and searches: the chosen ones, else Videos,
/// Downloads and the home folder (those that exist).
pub(super) fn folders(settings: &crate::settings::Settings) -> Vec<String> {
    // A JSON list: the settings file keeps one line per key.
    serde_json::from_str::<Vec<String>>(&settings.get_string(keys::VIDEO_LIBRARY_FOLDERS))
        .ok()
        .filter(|chosen| !chosen.is_empty())
        .unwrap_or_else(default_folders)
}

fn default_folders() -> Vec<String> {
    let mut found = Vec::new();
    for dir in [
        glib::user_special_dir(glib::UserDirectory::Videos),
        glib::user_special_dir(glib::UserDirectory::Downloads),
        Some(glib::home_dir()),
    ]
    .into_iter()
    .flatten()
    {
        let dir = dir.to_string_lossy().into_owned();
        if Path::new(&dir).is_dir() && !found.contains(&dir) {
            found.push(dir);
        }
    }
    found
}

fn save_folders(state: &Rc<BrowserState>, folders: &[String]) {
    // The defaults are stored as nothing, so a changed Videos folder follows.
    let value = if folders == default_folders().as_slice() {
        String::new()
    } else {
        serde_json::to_string(folders).unwrap_or_default()
    };
    let _ = state
        .settings
        .put_string(keys::VIDEO_LIBRARY_FOLDERS, &value);
    state.scanned.borrow_mut().clear();
    state.counts.borrow_mut().clear();
}

/// Removable drives mounted for this user now (`/run/media/$USER/*`).
fn drives() -> Vec<String> {
    let Some(user) = std::env::var_os("USER") else {
        return Vec::new();
    };
    let base = Path::new("/run/media").join(user);
    let Ok(entries) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    let mut drives: Vec<String> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    drives.sort();
    drives
}

/// Whether `dir` is one of the places: a library folder or a drive.
pub(super) fn is_place(settings: &crate::settings::Settings, dir: &str) -> bool {
    folders(settings)
        .iter()
        .chain(drives().iter())
        .any(|place| place == dir)
}

fn is_under(path: &str, dir: &str) -> bool {
    let dir = dir.trim_end_matches('/');
    path.strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn place_name(dir: &str) -> String {
    if Path::new(dir) == glib::home_dir() {
        return i18n("Home folder");
    }
    Path::new(dir).file_name().map_or_else(
        || dir.to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn place_icon(dir: &str) -> &'static str {
    let path = Path::new(dir);
    if path == glib::home_dir() {
        "user-home-symbolic"
    } else if glib::user_special_dir(glib::UserDirectory::Videos).as_deref() == Some(path) {
        "folder-videos-symbolic"
    } else if glib::user_special_dir(glib::UserDirectory::Downloads).as_deref() == Some(path) {
        "folder-download-symbolic"
    } else if dir.starts_with("/run/media/") {
        "drive-removable-media-symbolic"
    } else {
        "folder-symbolic"
    }
}

// ── the daemon ─────────────────────────────────────────────────────────────

/// The socket, as `big_indexd_client::default_socket`: never a shared
/// directory, where another account could answer in the daemon's place.
fn socket() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|runtime| runtime.join("biglinux").join("indexd.sock"))
}

#[derive(serde::Deserialize, Default)]
struct Reply {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    hits: Vec<Hit>,
    #[serde(default)]
    total: usize,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    roots: Vec<Root>,
}

#[derive(serde::Deserialize)]
struct Hit {
    path: String,
    #[serde(default)]
    available: bool,
    /// Its media type, from a daemon that has `mime-filter`.
    #[serde(default)]
    mime: String,
}

#[derive(serde::Deserialize)]
struct Root {
    path: String,
}

fn ask(socket: &Path, request: &serde_json::Value) -> Option<Reply> {
    let stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(TIMEOUT)).ok()?;
    let mut writer = stream.try_clone().ok()?;
    writeln!(writer, "{request}").ok()?;
    let mut line = String::new();
    // The reply is one line; a runaway one is not read to the end.
    BufReader::new(stream.take(8 * 1024 * 1024))
        .read_line(&mut line)
        .ok()?;
    serde_json::from_str(line.trim()).ok()
}

/// What the daemon offers this session: where it indexes, whether its
/// `list` mode filters inside the index (an older one answers empty), and
/// whether it knows each file's media type.
struct Daemon {
    socket: PathBuf,
    roots: Vec<String>,
    lists: bool,
    types: bool,
}

impl Daemon {
    fn find(socket: &Path) -> Option<Self> {
        let status = ask(socket, &serde_json::json!({ "v": 2, "cmd": "status" }))?;
        let roots = ask(socket, &serde_json::json!({ "v": 2, "cmd": "sources" }))
            .map(|reply| reply.roots.into_iter().map(|root| root.path).collect())
            .unwrap_or_default();
        Some(Self {
            socket: socket.to_path_buf(),
            roots,
            lists: status.capabilities.iter().any(|c| c == "list-filtered"),
            types: status.capabilities.iter().any(|c| c == "mime-filter"),
        })
    }

    fn indexes(&self, dir: &str) -> bool {
        self.roots
            .iter()
            .any(|root| dir == root || is_under(dir, root))
    }

    fn query(&self, q: &str, mode: &str, filter: serde_json::Value, limit: usize) -> Option<Reply> {
        let reply = ask(
            &self.socket,
            &serde_json::json!({
                "v": 2,
                "cmd": "query",
                "q": q,
                "mode": mode,
                "filter": filter,
                "page": { "offset": 0, "limit": limit },
            }),
        )?;
        (reply.kind == "query").then_some(reply)
    }
}

/// A hit the player can open: an absolute path to a video that is there now
/// (a hit on an unplugged disk is not).
fn openable(hit: &Hit) -> bool {
    hit.available && Path::new(&hit.path).is_absolute() && playlist::is_video_file(&hit.path)
}

/// Videos under `dir` whose names hold `q` (every one, in the `list` mode):
/// the first `limit` the player can open, and how many there are.
///
/// A daemon that knows media types is asked for `video/*`: it has already
/// told a transport stream from TypeScript, and finds a video with no
/// extension. An older one is asked by extension, `.ts` apart and each one
/// read here: asked together, TypeScript filled the answer and left one video
/// of hundreds.
fn videos(
    daemon: &Daemon,
    q: &str,
    mode: &str,
    dir: &str,
    limit: usize,
) -> Option<(Vec<String>, usize)> {
    if daemon.types {
        let filter = serde_json::json!({ "mime": ["video/*"], "under": dir });
        let reply = daemon.query(q, mode, filter, limit)?;
        let paths = reply
            .hits
            .into_iter()
            .filter(|hit| {
                hit.available
                    && Path::new(&hit.path).is_absolute()
                    && hit.mime.starts_with("video/")
            })
            .map(|hit| hit.path)
            .collect();
        return Some((paths, reply.total));
    }
    let others: Vec<&str> = playlist::VIDEO_EXTENSIONS
        .iter()
        .copied()
        .filter(|ext| *ext != "ts")
        .collect();
    let filter = serde_json::json!({ "ext": others, "under": dir });
    let reply = daemon.query(q, mode, filter, limit)?;
    let mut total = reply.total;
    let mut paths: Vec<String> = reply
        .hits
        .into_iter()
        .filter(openable)
        .map(|hit| hit.path)
        .collect();
    let filter = serde_json::json!({ "ext": ["ts"], "under": dir });
    if let Some(reply) = daemon.query(q, mode, filter, TS_READ) {
        let streams: Vec<String> = reply
            .hits
            .into_iter()
            .filter(openable)
            .map(|hit| hit.path)
            .collect();
        total += streams.len();
        paths.extend(streams);
    }
    paths.truncate(limit);
    Some((paths, total))
}

// ── searching ──────────────────────────────────────────────────────────────

/// What a search found: the first [`SHOWN`] videos, and how many there are.
#[derive(Default)]
struct Found {
    paths: Vec<String>,
    total: usize,
}

/// Videos in `folders` whose name matches `query`: the daemon for what it
/// indexes, a scan (kept in `scanned`) for the rest.
fn search(
    daemon: Option<&Daemon>,
    folders: &[String],
    query: &str,
    scanned: &mut HashMap<String, Vec<String>>,
) -> Found {
    // A folder inside another chosen one would only count its videos twice.
    let outer = folders
        .iter()
        .filter(|dir| !folders.iter().any(|other| is_under(dir, other)));
    let (indexed, rest): (Vec<&String>, Vec<&String>) =
        outer.partition(|dir| daemon.is_some_and(|daemon| daemon.indexes(dir)));
    let mut found = Found::default();
    if let Some(daemon) = daemon {
        for dir in indexed {
            if let Some((paths, total)) = videos(daemon, query, "name", dir, SHOWN) {
                found.paths.extend(paths);
                found.total += total;
            }
        }
    }
    let query = query.to_lowercase();
    for dir in rest {
        let files = scanned.entry(dir.clone()).or_insert_with(|| {
            playlist::scan_folder_recursive(Path::new(dir))
                .files
                .into_iter()
                .filter(|path| playlist::is_video_file(path))
                .collect()
        });
        let matches: Vec<&String> = files
            .iter()
            .filter(|path| {
                let name = playlist::display_name(path).to_lowercase();
                name.contains(&query) || playlist::fuzzy_match(&query, &name).is_some()
            })
            .collect();
        found.total += matches.len();
        found.paths.extend(matches.into_iter().cloned());
    }
    found.paths.truncate(SHOWN);
    found
}

/// Videos in each folder, when the daemon can count them.
fn count(daemon: &Daemon, dir: &str) -> Option<usize> {
    (daemon.lists && daemon.indexes(dir))
        .then(|| videos(daemon, "", "list", dir, 1))
        .flatten()
        .map(|(_, total)| total)
}

// ── the places page ────────────────────────────────────────────────────────

/// With no folder open: the library's folders and the drives plugged in,
/// then "Choose folders…". Each opens in the browser.
pub(super) fn build_places(state: &Rc<BrowserState>) {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(8)
        .margin_end(8)
        .margin_bottom(8)
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let chosen = folders(&state.settings);
    let mut counts: Vec<(String, adw::ActionRow)> = Vec::new();
    for dir in chosen
        .iter()
        .chain(drives().iter().filter(|drive| !chosen.contains(drive)))
    {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&place_name(dir)))
            .subtitle(glib::markup_escape_text(dir))
            .subtitle_lines(1)
            .activatable(true)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(place_icon(dir)));
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        {
            let state = state.clone();
            let dir = dir.clone();
            row.connect_activated(move |_| state.navigate_to_folder(&dir));
        }
        list.append(&row);
        counts.push((dir.clone(), row));
    }
    let choose = adw::ActionRow::builder()
        .title(i18n("Choose folders…"))
        .activatable(true)
        .build();
    choose.add_prefix(&gtk::Image::from_icon_name("folder-new-symbolic"));
    {
        let state = state.clone();
        choose.connect_activated(move |_| {
            state.customizing.set(true);
            state.rebuild();
        });
    }
    list.append(&choose);
    page.append(&list);
    state.places.replace(Some(page.downgrade()));
    if let Some(container) = state.container() {
        container.append(&page);
    }
    fill_counts(state, counts);
}

/// "35 videos" under each place the daemon can count, off the main loop.
fn fill_counts(state: &Rc<BrowserState>, rows: Vec<(String, adw::ActionRow)>) {
    let known = state.counts.borrow().clone();
    let show = |row: &adw::ActionRow, dir: &str, count: usize| {
        let text = ni18n("{} video", "{} videos", count as u64).replace("{}", &count.to_string());
        row.set_subtitle(&glib::markup_escape_text(&format!("{text} · {dir}")));
    };
    let mut wanted = Vec::new();
    for (dir, row) in &rows {
        match known.get(dir) {
            Some(count) => show(row, dir, *count),
            None => wanted.push(dir.clone()),
        }
    }
    if wanted.is_empty() {
        return;
    }
    let state = Rc::downgrade(state);
    let rows: Vec<(String, glib::WeakRef<adw::ActionRow>)> = rows
        .into_iter()
        .map(|(dir, row)| (dir, row.downgrade()))
        .collect();
    glib::spawn_future_local(async move {
        let counted = gio::spawn_blocking(move || {
            let Some(daemon) = socket().and_then(|socket| Daemon::find(&socket)) else {
                return Vec::new();
            };
            wanted
                .into_iter()
                .filter_map(|dir| count(&daemon, &dir).map(|count| (dir, count)))
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let Some(state) = state.upgrade() else {
            return;
        };
        state.counts.borrow_mut().extend(counted.iter().cloned());
        for (dir, count) in counted {
            if let Some(row) = rows
                .iter()
                .find(|(row_dir, _)| *row_dir == dir)
                .and_then(|(_, row)| row.upgrade())
            {
                show(&row, &dir, count);
            }
        }
    });
}

// ── the search results ─────────────────────────────────────────────────────

/// The section of search results across the library: hidden until a search.
pub(super) fn build_results(state: &Rc<BrowserState>) -> gtk::Box {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_start(8)
        .margin_end(8)
        .margin_bottom(8)
        .visible(false)
        .build();
    state.results.replace(Some(section.downgrade()));
    section
}

/// Search the library for `query` (already lowercased) and fill the results,
/// leaving out what the open folder lists already. With a folder open they
/// are "in your other folders"; on the places page they replace the places.
/// Stale answers, for a query typed over since, are dropped.
pub(super) fn refresh(state: &Rc<BrowserState>, query_text: String) {
    let section = state
        .results
        .borrow()
        .as_ref()
        .and_then(glib::WeakRef::upgrade);
    let places = state
        .places
        .borrow()
        .as_ref()
        .and_then(glib::WeakRef::upgrade);
    let searching = query_text.chars().count() >= 2;
    if let Some(places) = &places {
        places.set_visible(!searching);
    }
    let Some(section) = section else {
        return;
    };
    if !searching {
        section.set_visible(false);
        return;
    }
    let generation = state.search_gen.get();
    let folders = folders(&state.settings);
    let mut scanned = std::mem::take(&mut *state.scanned.borrow_mut());
    let state = state.clone();
    glib::spawn_future_local(async move {
        let wanted = query_text.clone();
        let (mut found, scanned) = gio::spawn_blocking(move || {
            let daemon = socket().and_then(|socket| Daemon::find(&socket));
            let found = search(daemon.as_ref(), &folders, &wanted, &mut scanned);
            (found, scanned)
        })
        .await
        .unwrap_or_default();
        state.scanned.borrow_mut().extend(scanned);
        if state.search_gen.get() != generation {
            return;
        }
        let local: std::collections::HashSet<String> =
            state.entries.borrow().iter().cloned().collect();
        let in_folder = state.folder.borrow().is_some();
        let listed = found.paths.len();
        found.paths.retain(|path| !local.contains(path));
        found.total -= listed - found.paths.len();
        fill(&state, &section, &found, in_folder);
    });
}

fn fill(state: &Rc<BrowserState>, section: &gtk::Box, found: &Found, in_folder: bool) {
    let hits = &found.paths;
    while let Some(child) = section.first_child() {
        section.remove(&child);
    }
    if hits.is_empty() {
        // On the places page the answer is said; under a folder's own
        // matches an empty section would only repeat the folder's.
        section.set_visible(!in_folder);
        if !in_folder {
            section.append(
                &gtk::Label::builder()
                    .label(i18n("No video in your folders has that name"))
                    .css_classes(["dim-label"])
                    .margin_top(12)
                    .build(),
            );
        }
        return;
    }
    section.set_visible(true);
    let total = found.total.max(hits.len());
    let heading = if in_folder {
        ni18n(
            "{} more in your folders",
            "{} more in your folders",
            total as u64,
        )
    } else {
        ni18n("{} video found", "{} videos found", total as u64)
    }
    .replace("{}", &total.to_string());
    section.append(
        &gtk::Label::builder()
            .label(&heading)
            .xalign(0.0)
            .css_classes(["heading"])
            .margin_top(6)
            .build(),
    );
    if total > hits.len() {
        section.append(
            &gtk::Label::builder()
                .label(
                    i18n("Showing the first {}. Type more to narrow the search.")
                        .replace("{}", &hits.len().to_string()),
                )
                .xalign(0.0)
                .wrap(true)
                .max_width_chars(30)
                .css_classes(["dim-label", "caption"])
                .build(),
        );
    }
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    for path in hits {
        list.append(&result_row(state, path));
    }
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(if in_folder { 240 } else { 520 })
        .child(&list)
        .build();
    section.append(&scroll);
}

/// A found video: its picture, name and folder. A click plays it right after
/// the current one; + adds it to the end of the playlist.
fn result_row(state: &Rc<BrowserState>, path: &str) -> adw::ActionRow {
    let folder = Path::new(path)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default();
    let row = adw::ActionRow::builder()
        .title(glib::markup_escape_text(&playlist::display_name(path)))
        .subtitle(glib::markup_escape_text(&folder))
        .subtitle_lines(1)
        .activatable(true)
        .build();
    row.set_tooltip_text(Some(path));
    let picture = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .build();
    let frame = gtk::Overlay::builder()
        .child(
            &gtk::Box::builder()
                .width_request(64)
                .height_request(36)
                .build(),
        )
        .valign(gtk::Align::Center)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["playlist-thumb-frame"])
        .build();
    frame.add_overlay(&picture);
    frame.set_measure_overlay(&picture, false);
    row.add_prefix(&frame);
    let add = gtk::Button::builder()
        .icon_name("list-add-symbolic")
        .tooltip_text(i18n("Add to playlist"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    add.update_property(&[gtk::accessible::Property::Label(&i18n("Add to playlist"))]);
    row.add_suffix(&add);
    {
        let state = state.clone();
        let path = path.to_string();
        row.connect_activated(move |_| {
            let _ = state.sender.output(VideoBrowserSidebarOutput::PlayNext {
                path: path.clone(),
                now: true,
            });
        });
    }
    {
        let state = state.clone();
        let path = path.to_string();
        add.connect_clicked(move |_| {
            let _ = state
                .sender
                .output(VideoBrowserSidebarOutput::Enqueue(vec![path.clone()]));
        });
    }
    let picture = picture.downgrade();
    let path = path.to_string();
    glib::spawn_future_local(async move {
        let texture = gio::spawn_blocking(move || {
            let file = thumbnail::get_or_generate_thumbnail(&path)?;
            gdk::Texture::from_filename(file).ok()
        })
        .await
        .ok()
        .flatten();
        if let (Some(texture), Some(picture)) = (texture, picture.upgrade()) {
            picture.set_paintable(Some(&texture));
        }
    });
    row
}

// ── choosing the folders ───────────────────────────────────────────────────

/// The folders the library shows and searches: remove one, add one, or go
/// back to Videos, Downloads and the home folder.
pub(super) fn build_customize(state: &Rc<BrowserState>) {
    let page = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_start(8)
        .margin_end(8)
        .margin_bottom(8)
        .build();
    let top = gtk::Box::builder().spacing(6).build();
    let back = gtk::Button::builder()
        .icon_name("go-previous-symbolic")
        .tooltip_text(i18n("Back"))
        .css_classes(["flat", "circular"])
        .build();
    back.update_property(&[gtk::accessible::Property::Label(&i18n("Back"))]);
    {
        let state = state.clone();
        back.connect_clicked(move |_| {
            state.customizing.set(false);
            state.rebuild();
        });
    }
    top.append(&back);
    top.append(
        &gtk::Label::builder()
            .label(i18n("Video folders"))
            .css_classes(["heading"])
            .xalign(0.0)
            .hexpand(true)
            .build(),
    );
    page.append(&top);
    page.append(
        &gtk::Label::builder()
            .label(i18n(
                "Their videos are listed and searched here. Folders BigSearch does not index are searched by the player, more slowly.",
            ))
            .wrap(true)
            // Natural width short of the card's: it wraps inside, not widens it.
            .max_width_chars(30)
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build(),
    );

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let chosen = folders(&state.settings);
    for dir in &chosen {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&place_name(dir)))
            .subtitle(glib::markup_escape_text(dir))
            .subtitle_lines(1)
            .build();
        row.add_prefix(&gtk::Image::from_icon_name(place_icon(dir)));
        let remove = gtk::Button::builder()
            .icon_name("list-remove-symbolic")
            .tooltip_text(i18n("Remove"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        remove.update_property(&[gtk::accessible::Property::Label(
            &i18n("Remove {}").replace("{}", &place_name(dir)),
        )]);
        // The last folder stays: a library of nothing would search nothing.
        remove.set_sensitive(chosen.len() > 1);
        {
            let state = state.clone();
            let dir = dir.clone();
            remove.connect_clicked(move |_| {
                let mut kept = folders(&state.settings);
                kept.retain(|kept| *kept != dir);
                save_folders(&state, &kept);
                state.rebuild();
            });
        }
        row.add_suffix(&remove);
        list.append(&row);
    }
    let add = adw::ActionRow::builder()
        .title(i18n("Add a folder…"))
        .activatable(true)
        .build();
    add.add_prefix(&gtk::Image::from_icon_name("list-add-symbolic"));
    {
        let state = state.clone();
        add.connect_activated(move |_| {
            let Some(window) = state.window() else {
                return;
            };
            let state = state.clone();
            big_app_kit::file_dialogs::FilePicker::new(i18n("Add a Video Folder")).select_folder(
                &window,
                move |folder| {
                    let folder = folder.to_string_lossy().into_owned();
                    let mut kept = folders(&state.settings);
                    if !kept.contains(&folder) {
                        kept.push(folder);
                        save_folders(&state, &kept);
                    }
                    state.rebuild();
                },
            );
        });
    }
    list.append(&add);
    page.append(&list);

    let defaults = gtk::Button::builder()
        .label(i18n("Use the standard folders"))
        .tooltip_text(i18n("Videos, Downloads and the home folder"))
        .halign(gtk::Align::Start)
        .css_classes(["flat"])
        .sensitive(chosen != default_folders())
        .build();
    {
        let state = state.clone();
        defaults.connect_clicked(move |_| {
            save_folders(&state, &default_folders());
            state.rebuild();
        });
    }
    page.append(&defaults);

    if let Some(container) = state.container() {
        container.append(&page);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A fake daemon answering each request in turn with `replies`; returns
    /// the requests it got.
    fn serve(socket: &Path, replies: Vec<String>) -> std::thread::JoinHandle<Vec<String>> {
        let listener = UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            let mut requests = Vec::new();
            for reply in replies {
                let (stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                let mut writer = stream;
                writer.write_all(reply.as_bytes()).unwrap();
                writer.write_all(b"\n").unwrap();
                requests.push(request);
            }
            requests
        })
    }

    #[test]
    fn no_daemon_is_no_daemon() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Daemon::find(&dir.path().join("indexd.sock")).is_none());
    }

    #[test]
    fn a_search_asks_each_outer_folder_and_reads_its_ts_files() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("indexd.sock");
        // A transport stream syncs on 0x47 every 188 bytes; TypeScript does not.
        let stream = dir.path().join("recording.ts");
        let mut packets = vec![0_u8; 188 * 3];
        packets
            .iter_mut()
            .step_by(188)
            .for_each(|byte| *byte = 0x47);
        std::fs::write(&stream, packets).unwrap();
        let code = dir.path().join("prompt.ts");
        std::fs::write(&code, "export const x = 1;\n").unwrap();
        let server = serve(
            &socket,
            vec![
                r#"{"kind":"status","v":2,"capabilities":["sources","list-filtered"]}"#.into(),
                r#"{"kind":"sources","v":2,"roots":[{"path":"/home/u"}]}"#.into(),
                concat!(
                    r#"{"kind":"query","v":2,"total":5,"hits":["#,
                    r#"{"path":"/home/u/Videos/show.mkv","available":true},"#,
                    r#"{"path":"/home/u/Videos/offline.mp4","available":false},"#,
                    r#"{"path":"relative.mp4","available":true},"#,
                    r#"{"path":"/home/u/Videos/notes.txt","available":true}"#,
                    r#"]}"#,
                )
                .into(),
                format!(
                    r#"{{"kind":"query","v":2,"total":2,"hits":[{{"path":{:?},"available":true}},{{"path":{:?},"available":true}}]}}"#,
                    code.to_str().unwrap(),
                    stream.to_str().unwrap(),
                ),
            ],
        );
        let daemon = Daemon::find(&socket).unwrap();
        assert!(daemon.lists);
        assert!(daemon.indexes("/home/u/Videos"));
        assert!(!daemon.indexes("/run/media/u/disk"));
        // Videos is inside the home folder: asked once, as part of it.
        let found = search(
            Some(&daemon),
            &["/home/u/Videos".to_string(), "/home/u".to_string()],
            "show",
            &mut HashMap::new(),
        );
        let requests: Vec<serde_json::Value> = server
            .join()
            .unwrap()
            .iter()
            .map(|request| serde_json::from_str(request).unwrap())
            .collect();
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[2]["q"], "show");
        assert_eq!(requests[2]["mode"], "name");
        assert_eq!(requests[2]["filter"]["under"], "/home/u");
        let asked = requests[2]["filter"]["ext"].as_array().unwrap();
        assert!(asked.contains(&"mkv".into()) && !asked.contains(&"ts".into()));
        assert_eq!(requests[3]["filter"]["ext"], serde_json::json!(["ts"]));
        assert_eq!(
            found.paths,
            ["/home/u/Videos/show.mkv", stream.to_str().unwrap()]
        );
        assert_eq!(found.total, 6);
    }

    #[test]
    fn a_daemon_that_knows_types_is_asked_for_videos_once() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("indexd.sock");
        let server = serve(
            &socket,
            vec![
                r#"{"kind":"status","v":2,"capabilities":["sources","list-filtered","mime-filter"]}"#.into(),
                r#"{"kind":"sources","v":2,"roots":[{"path":"/home/u"}]}"#.into(),
                concat!(
                    r#"{"kind":"query","v":2,"total":3,"hits":["#,
                    r#"{"path":"/home/u/recording","available":true,"mime":"video/mp2t"},"#,
                    r#"{"path":"/home/u/clip.ts","available":true,"mime":"video/mp2t"},"#,
                    r#"{"path":"/home/u/away.mkv","available":false,"mime":"video/matroska"}"#,
                    r#"]}"#,
                )
                .into(),
            ],
        );
        let daemon = Daemon::find(&socket).unwrap();
        assert!(daemon.types);
        let found = search(
            Some(&daemon),
            &["/home/u".to_string()],
            "re",
            &mut HashMap::new(),
        );
        let requests: Vec<serde_json::Value> = server
            .join()
            .unwrap()
            .iter()
            .map(|request| serde_json::from_str(request).unwrap())
            .collect();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests[2]["filter"],
            serde_json::json!({ "mime": ["video/*"], "under": "/home/u" })
        );
        assert_eq!(found.paths, ["/home/u/recording", "/home/u/clip.ts"]);
        assert_eq!(found.total, 3);
    }

    #[test]
    fn a_folder_the_daemon_does_not_index_is_scanned() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Holiday Trip.mp4"), b"").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"").unwrap();
        let folder = dir.path().to_string_lossy().into_owned();
        let found = search(None, &[folder], "holiday", &mut HashMap::new());
        assert_eq!(found.total, 1);
        assert_eq!(found.paths.len(), 1, "{:?}", found.paths);
        assert!(found.paths[0].ends_with("Holiday Trip.mp4"));
    }

    #[test]
    fn under_means_inside() {
        assert!(is_under("/home/u/Videos/a.mp4", "/home/u/Videos"));
        assert!(is_under("/home/u/Videos/a.mp4", "/home/u/Videos/"));
        assert!(!is_under("/home/u/Videos2/a.mp4", "/home/u/Videos"));
    }
}
