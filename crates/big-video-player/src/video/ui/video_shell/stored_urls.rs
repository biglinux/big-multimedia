//! The two forms of an IPTV channel's URL: the playlist lists and saves the
//! stored one, free of the account's credentials, while mpv opens the played
//! one. Every exchange between the playlist and mpv goes through this map.

use std::collections::HashMap;

use big_media_runtime::player::mpv_backend::TargetSource;

/// Played URLs of the channels the playlist lists by their stored URL, for
/// this run of the player only.
#[derive(Debug, Default)]
pub(super) struct StoredUrls {
    played: HashMap<String, String>,
    stored: HashMap<String, String>,
    /// Played URLs with no stored form, which no playlist may list, and the
    /// list that named each.
    unlisted: HashMap<String, TargetSource>,
}

impl StoredUrls {
    /// An older played URL of `stored` keeps mapping back to it: mpv may
    /// still list it.
    pub(super) fn insert(&mut self, stored: String, played: String) {
        self.played.insert(stored.clone(), played.clone());
        self.stored.insert(played, stored);
    }

    /// Record `played` as a channel no playlist may list, named by `source`.
    pub(super) fn insert_unlisted(&mut self, played: String, source: TargetSource) {
        self.unlisted.insert(played, source);
    }

    /// The list that named `played`, when no playlist may list it.
    pub(super) fn unlisted(&self, played: &str) -> Option<TargetSource> {
        self.unlisted.get(played).copied()
    }

    /// What mpv opens for the playlist's `entry`.
    pub(super) fn played_url<'a>(&'a self, entry: &'a str) -> &'a str {
        self.played.get(entry).map_or(entry, String::as_str)
    }

    /// What the playlist lists for the file mpv opened as `path`.
    pub(super) fn stored_url<'a>(&'a self, path: &'a str) -> &'a str {
        self.stored.get(path).map_or(path, String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use big_os_kit::url::ListOrigin;

    const STORED: &str = "http://srv/live/{credentials}/1.ts";
    const PLAYED: &str = "http://srv/live/ana/s3cret/1.ts";

    #[test]
    fn channels_map_both_ways() {
        let mut urls = StoredUrls::default();
        urls.insert(STORED.into(), PLAYED.into());
        assert_eq!(urls.played_url(STORED), PLAYED);
        assert_eq!(urls.stored_url(PLAYED), STORED);
        assert_eq!(urls.stored_url(STORED), STORED);
    }

    #[test]
    fn other_entries_stay_as_they_are() {
        let mut urls = StoredUrls::default();
        urls.insert(STORED.into(), PLAYED.into());
        for entry in ["/videos/a.mkv", "https://example.org/live.m3u8", ""] {
            assert_eq!(urls.played_url(entry), entry);
            assert_eq!(urls.stored_url(entry), entry);
        }
    }

    #[test]
    fn unlisted_channels_keep_their_list() {
        let mut urls = StoredUrls::default();
        urls.insert_unlisted(PLAYED.into(), TargetSource::List(ListOrigin::Network));
        assert_eq!(
            urls.unlisted(PLAYED),
            Some(TargetSource::List(ListOrigin::Network))
        );
        assert_eq!(urls.unlisted(STORED), None);
        assert_eq!(urls.stored_url(PLAYED), PLAYED);
    }

    #[test]
    fn a_changed_password_plays_the_new_url() {
        let mut urls = StoredUrls::default();
        urls.insert(STORED.into(), PLAYED.into());
        urls.insert(STORED.into(), "http://srv/live/ana/n3w/1.ts".into());
        assert_eq!(urls.played_url(STORED), "http://srv/live/ana/n3w/1.ts");
        assert_eq!(urls.stored_url(PLAYED), STORED);
    }
}
