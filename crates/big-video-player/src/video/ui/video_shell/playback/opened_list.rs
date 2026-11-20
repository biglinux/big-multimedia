//! What an open request resolves to: a playlist file read, a single file's
//! folder scanned, streams and IPTV channels with their played URLs.

use std::sync::atomic::AtomicBool;

use big_media_runtime::player::mpv_backend::TargetSource;
use big_os_kit::keyring::SecretService;
use big_os_kit::url::ListOrigin;

use crate::media::iptv::IptvProvider;
use crate::video::ui::iptv::fetcher::{FetchError, played_channels};

/// What an open request resolved to.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OpenedList {
    pub entries: Vec<String>,
    pub start: usize,
    /// The folder the sidebar browses, for a single opened file.
    pub folder: Option<String>,
    /// The opened playlist file listed more entries than were kept.
    pub truncated: bool,
    /// Entries of the opened playlist file the player does not open.
    pub skipped: usize,
    pub source: TargetSource,
    /// `(stored, played)` of the IPTV channels among the entries.
    pub channels: Vec<(String, String)>,
    /// Why the IPTV channels among the entries cannot play.
    pub channel_error: Option<FetchError>,
}

impl OpenedList {
    fn from_entries(entries: Vec<String>) -> Self {
        Self {
            entries,
            ..Self::default()
        }
    }
}

/// Resolve an open request off the main thread. Pure + `Send` (captures only
/// `String`s); the blocking folder scan / playlist parse run here. Returns
/// `None` when nothing playable was found.
pub(super) fn resolve_open_files(
    paths: Vec<String>,
    (sort_key, sort_desc): &(String, bool),
) -> Option<OpenedList> {
    use big_media_runtime::media::playlist;

    if paths.is_empty() {
        return None;
    }

    // A playlist file on disk; an HLS stream ends in .m3u8 too, and it is
    // played, not read as a file: every IPTV channel was one.
    if paths.len() == 1 && !is_remote_uri(&paths[0]) && playlist::is_playlist_file(&paths[0]) {
        let parsed = playlist::parse_playlist_file(std::path::Path::new(&paths[0])).ok()?;
        if parsed.entries.is_empty() && parsed.skipped == 0 {
            return None;
        }
        return Some(OpenedList {
            truncated: parsed.truncated,
            skipped: parsed.skipped,
            source: TargetSource::List(ListOrigin::LocalFile),
            ..OpenedList::from_entries(parsed.entries)
        });
    }

    if paths.len() == 1 && !is_remote_uri(&paths[0]) {
        // Single local file → scan its folder, start at this file, and show that
        // folder as the sidebar's browse context.
        let alone = OpenedList::from_entries(vec![paths[0].clone()]);
        return match std::path::Path::new(&paths[0]).parent() {
            Some(parent) => {
                let mut folder = playlist::scan_folder_flat(parent);
                playlist::sort_paths(&mut folder, sort_key, *sort_desc);
                // A file the scan does not list (an extension it lacks) plays
                // alone, never the folder's first video in its place.
                match folder.iter().position(|p| p == &paths[0]) {
                    Some(start) => Some(OpenedList {
                        entries: folder,
                        start,
                        folder: Some(parent.to_string_lossy().into_owned()),
                        ..OpenedList::default()
                    }),
                    None => Some(alone),
                }
            }
            None => Some(alone),
        };
    }

    if paths.len() == 1 {
        // Single remote URI / stream → just that entry.
        return Some(OpenedList::from_entries(paths));
    }

    let media: Vec<String> = paths
        .iter()
        .filter(|p| playlist::is_media_file(p))
        .cloned()
        .collect();
    if media.is_empty() {
        return None;
    }
    Some(OpenedList::from_entries(media))
}

/// `list` with the played URLs of the IPTV channels it names by their stored
/// URL. Blocking, reads the keyring.
pub(super) fn with_played_channels(mut list: OpenedList, providers: &[IptvProvider]) -> OpenedList {
    let cancelled = AtomicBool::new(false);
    let vault = SecretService {
        cancelled: &cancelled,
    };
    match played_channels(&list.entries, providers, &vault) {
        Ok(channels) => list.channels = channels,
        Err(error) => list.channel_error = Some(error),
    }
    list
}

/// Streams and discs: what mpv opens by URL, never a path on disk. Schemes
/// are case-insensitive; IPTV lists use the UDP, RTP, SRT and MMS ones too.
pub(crate) fn is_remote_uri(s: &str) -> bool {
    let s = s.trim();
    big_os_kit::url::is_stream_uri(s)
        || s.split_once("://").is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("dvd") || scheme.eq_ignore_ascii_case("bd")
        })
}
