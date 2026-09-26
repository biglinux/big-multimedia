//! "On this computer": the Files search also asks BigSearch's index daemon,
//! when one runs, for videos anywhere on the disk, and lists them under the
//! folder's own matches. No daemon, no section: the search stays the folder's.
//!
//! The protocol is one JSON line each way over the user's own Unix socket
//! (`$XDG_RUNTIME_DIR/biglinux/indexd.sock`, as `big-indexd-client` defines
//! it); only the query request and the hit fields used here are spoken.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};

use super::{BrowserState, VideoBrowserSidebarOutput};
use crate::i18n::{i18n, ni18n};
use crate::media::playlist;

/// Results listed; a search that wants more is better narrowed.
const SHOWN: usize = 20;
/// A daemon that does not answer quickly is left out, not waited on.
const TIMEOUT: Duration = Duration::from_secs(2);

/// The socket, as `big_indexd_client::default_socket`: never a shared
/// directory, where another account could answer in the daemon's place.
fn socket() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|runtime| runtime.join("biglinux").join("indexd.sock"))
}

#[derive(serde::Deserialize)]
struct Reply {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    hits: Vec<Hit>,
}

#[derive(serde::Deserialize)]
struct Hit {
    path: String,
    #[serde(default)]
    available: bool,
}

/// Videos whose name matches `query`, anywhere the daemon indexes. `None`
/// when no daemon answers.
fn query(query: &str) -> Option<Vec<String>> {
    query_at(&socket()?, query)
}

fn query_at(socket: &Path, query: &str) -> Option<Vec<String>> {
    let stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(TIMEOUT)).ok()?;
    let request = serde_json::json!({
        "v": 2,
        "cmd": "query",
        "q": query,
        "mode": "name",
        "filter": { "ext": playlist::VIDEO_EXTENSIONS },
        "page": { "offset": 0, "limit": SHOWN * 2 },
    });
    let mut writer = stream.try_clone().ok()?;
    writeln!(writer, "{request}").ok()?;
    let mut line = String::new();
    // The reply is one line; a runaway one is not read to the end.
    BufReader::new(stream.take(4 * 1024 * 1024))
        .read_line(&mut line)
        .ok()?;
    let reply: Reply = serde_json::from_str(line.trim()).ok()?;
    (reply.kind == "query").then(|| {
        reply
            .hits
            .into_iter()
            // What the player can open: an absolute path to a video that is
            // there now (a hit on an unplugged disk is not).
            .filter(|hit| {
                hit.available
                    && Path::new(&hit.path).is_absolute()
                    && playlist::is_media_file(&hit.path)
            })
            .map(|hit| hit.path)
            .collect()
    })
}

/// The section under the folder's list: hidden until a search finds
/// something elsewhere.
pub(super) fn build_section(state: &Rc<BrowserState>) -> gtk::Box {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_start(8)
        .margin_end(8)
        .margin_bottom(8)
        .visible(false)
        .build();
    state.disk_section.replace(Some(section.downgrade()));
    section
}

/// Ask the daemon for `query` (already lowercased) and fill the section,
/// leaving out what the open folder lists already. Stale answers, for a
/// query typed over since, are dropped.
pub(super) fn refresh(state: &Rc<BrowserState>, query_text: String) {
    let Some(section) = state
        .disk_section
        .borrow()
        .as_ref()
        .and_then(glib::WeakRef::upgrade)
    else {
        return;
    };
    if query_text.chars().count() < 2 {
        section.set_visible(false);
        return;
    }
    let generation = state.search_gen.get();
    let state = state.clone();
    glib::spawn_future_local(async move {
        let wanted = query_text.clone();
        let hits = gio::spawn_blocking(move || query(&wanted))
            .await
            .ok()
            .flatten();
        if state.search_gen.get() != generation {
            return;
        }
        let local: std::collections::HashSet<String> =
            state.entries.borrow().iter().cloned().collect();
        let hits: Vec<String> = hits
            .unwrap_or_default()
            .into_iter()
            .filter(|path| !local.contains(path))
            .take(SHOWN)
            .collect();
        fill(&state, &section, &hits);
    });
}

fn fill(state: &Rc<BrowserState>, section: &gtk::Box, hits: &[String]) {
    while let Some(child) = section.first_child() {
        section.remove(&child);
    }
    section.set_visible(!hits.is_empty());
    if hits.is_empty() {
        return;
    }
    let heading = ni18n(
        "{} more on this computer",
        "{} more on this computer",
        hits.len() as u64,
    )
    .replace("{}", &hits.len().to_string());
    section.append(
        &gtk::Label::builder()
            .label(&heading)
            .xalign(0.0)
            .css_classes(["heading"])
            .margin_top(6)
            .build(),
    );
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    for path in hits {
        let name = playlist::display_name(path);
        let folder = Path::new(path)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&name))
            .subtitle(glib::markup_escape_text(&folder))
            .subtitle_lines(1)
            .activatable(true)
            .build();
        row.set_tooltip_text(Some(path));
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
            let path = path.clone();
            row.connect_activated(move |_| {
                let _ = state.sender.output(VideoBrowserSidebarOutput::PlayNext {
                    path: path.clone(),
                    now: true,
                });
            });
        }
        {
            let state = state.clone();
            let path = path.clone();
            add.connect_clicked(move |_| {
                let _ = state
                    .sender
                    .output(VideoBrowserSidebarOutput::Enqueue(vec![path.clone()]));
            });
        }
        list.append(&row);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(240)
        .child(&list)
        .build();
    section.append(&scroll);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    #[test]
    fn no_daemon_is_no_answer() {
        let dir = tempfile::tempdir().unwrap();
        assert!(query_at(&dir.path().join("indexd.sock"), "camisa").is_none());
    }

    #[test]
    fn only_videos_that_are_there_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("indexd.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut request = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut request)
                .unwrap();
            let mut writer = stream;
            writer
                .write_all(
                    concat!(
                        r#"{"kind":"query","v":2,"hits":["#,
                        r#"{"path":"/v/show.mkv","name":"show.mkv","available":true},"#,
                        r#"{"path":"/v/offline.mp4","name":"offline.mp4","available":false},"#,
                        r#"{"path":"relative.mp4","name":"relative.mp4","available":true},"#,
                        r#"{"path":"/v/notes.txt","name":"notes.txt","available":true}"#,
                        r#"],"total":4,"next_offset":null}"#,
                        "\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
            request
        });
        let hits = query_at(&socket, "show").unwrap();
        let request: serde_json::Value = serde_json::from_str(&server.join().unwrap()).unwrap();
        assert_eq!(request["cmd"], "query");
        assert_eq!(request["q"], "show");
        assert_eq!(hits, ["/v/show.mkv"]);
    }
}
