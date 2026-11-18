//! Extended M3U (IPTV) parser.
//!
//! Handles the `#EXTINF:` subset used by IPTV playlists:
//!
//! ```text
//! #EXTINF:-1 tvg-id="..." tvg-name="..." tvg-logo="..." group-title="...",Display Name
//! https://stream.example.com/channel.m3u8
//! ```
//!
//! Tolerates missing attributes, unquoted spaces in names, and blank/comment
//! lines. Attribute parsing is permissive: IPTV playlists in the wild are
//! sloppy and strict parsing loses channels.

use std::path::Path;

use super::channel::IptvChannel;

/// Most entries kept from one playlist: a 64 MiB list of bare lines would
/// otherwise become tens of millions of entries and several gigabytes.
pub const MAX_PLAYLIST_ENTRIES: usize = 100_000;

/// The entries of one playlist, at most [`MAX_PLAYLIST_ENTRIES`].
#[derive(Debug, PartialEq, Eq)]
pub struct PlaylistEntries<T> {
    pub entries: Vec<T>,
    /// The playlist listed more entries than were kept.
    pub truncated: bool,
}

impl<T> Default for PlaylistEntries<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            truncated: false,
        }
    }
}

impl<T> PlaylistEntries<T> {
    /// Keeps `entry` while under the cap; false, and marked truncated, once full.
    pub fn push(&mut self, entry: T) -> bool {
        if self.entries.len() == MAX_PLAYLIST_ENTRIES {
            log::warn!("playlist over {MAX_PLAYLIST_ENTRIES} entries truncated");
            self.truncated = true;
            return false;
        }
        self.entries.push(entry);
        true
    }
}

/// Parse an in-memory M3U/M3U8 playlist string into channel entries.
#[must_use]
pub fn parse_m3u_content(content: &str) -> PlaylistEntries<IptvChannel> {
    let mut channels = PlaylistEntries::default();
    let mut pending = IptvChannel::default();
    // A byte-order mark would turn `#EXTM3U` into a channel URL.
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("#EXTINF:") {
            pending = IptvChannel {
                name: extract_display_name(line),
                group: extract_attr(line, "group-title").unwrap_or_default(),
                logo: extract_attr(line, "tvg-logo").unwrap_or_default(),
                url: String::new(),
            };
        } else if !line.is_empty() && !line.starts_with('#') {
            let mut ch = std::mem::take(&mut pending);
            if ch.name.is_empty() {
                ch.name = line.to_string();
            }
            ch.url = line.to_string();
            if !channels.push(ch) {
                break;
            }
        }
    }

    channels
}

/// Largest playlist file read from disk. IPTV provider lists run to tens of
/// megabytes; anything bigger is refused instead of loaded into memory.
pub const MAX_PLAYLIST_BYTES: u64 = 64 * 1024 * 1024;

/// Playlist text from its bytes: UTF-8, or else Latin-1, the encoding of
/// older `.m3u` lists, where every byte is a character.
#[must_use]
pub fn decode_playlist(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap_or_else(|error| error.into_bytes().into_iter().map(char::from).collect())
}

/// Read a playlist file, refusing files over [`MAX_PLAYLIST_BYTES`].
#[must_use]
pub fn read_playlist_text(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_PLAYLIST_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_PLAYLIST_BYTES {
        log::warn!("playlist over {MAX_PLAYLIST_BYTES} bytes ignored");
        return None;
    }
    Some(decode_playlist(bytes))
}

/// Read and parse an M3U/M3U8 file from disk.
///
/// Returns no entries on read error or an oversized file; callers that
/// need to distinguish "file missing" from "valid but empty" should check
/// existence first.
#[must_use]
pub fn parse_m3u_file(path: &Path) -> PlaylistEntries<IptvChannel> {
    read_playlist_text(path)
        .map(|s| parse_m3u_content(&s))
        .unwrap_or_default()
}

/// The quoted value of `attr`, matched by its whole name among the attributes
/// before the display name.
fn extract_attr(line: &str, attr: &str) -> Option<String> {
    let mut quoted = false;
    let mut previous = ' ';
    for (pos, character) in line.char_indices() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => return None,
            _ if !quoted && previous.is_whitespace() => {
                if let Some(rest) = line[pos..]
                    .strip_prefix(attr)
                    .and_then(|rest| rest.strip_prefix("=\""))
                {
                    let end = rest.find('"')?;
                    return Some(rest[..end].to_string());
                }
            }
            _ => {}
        }
        previous = character;
    }
    None
}

/// The name after the first comma outside the quoted attributes: a name may
/// hold commas of its own ("Globo, SP"), and an attribute value too.
fn extract_display_name(line: &str) -> String {
    let mut quoted = false;
    for (pos, character) in line.char_indices() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => return line[pos + 1..].trim().to_string(),
            _ => {}
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    proptest::proptest! {
        // Playlists come from disk and the network: any text parses without
        // panicking, into at most one entry per line.
        #[test]
        fn parse_never_panics(text in "\\PC{0,4096}") {
            proptest::prop_assert!(parse_m3u_content(&text).entries.len() <= text.lines().count());
        }
    }

    #[test]
    fn names_keep_their_commas() {
        let input = "#EXTINF:-1 group-title=\"News, Brazil\",Globo, SP\nhttp://a/1\n";
        let channels = parse_m3u_content(input).entries;
        assert_eq!(channels[0].name, "Globo, SP");
        assert_eq!(channels[0].group, "News, Brazil");
    }

    #[test]
    fn byte_order_mark_is_not_a_channel() {
        let input = "\u{feff}#EXTM3U\n#EXTINF:-1,One\nhttp://a/1\n";
        let channels = parse_m3u_content(input).entries;
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "One");
    }

    #[test]
    fn latin1_file_still_lists_its_channels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("list.m3u");
        std::fs::write(&path, b"#EXTINF:-1,Canal Not\xedcias\nhttp://a/1\n").unwrap();
        let channels = parse_m3u_file(&path).entries;
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Canal Notícias");
        assert_eq!(channels[0].url, "http://a/1");
    }

    #[test]
    fn parses_basic_channels() {
        let input = "\
#EXTM3U
#EXTINF:-1,Channel One
http://stream1.example.com/live
#EXTINF:-1,Channel Two
http://stream2.example.com/live
";
        let channels = parse_m3u_content(input).entries;
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "Channel One");
        assert_eq!(channels[0].url, "http://stream1.example.com/live");
        assert_eq!(channels[1].name, "Channel Two");
    }

    #[test]
    fn captures_group_title() {
        let input = "\
#EXTINF:-1 group-title=\"Sports\",ESPN
http://espn.example.com
#EXTINF:-1 group-title=\"News\",CNN
http://cnn.example.com
";
        let channels = parse_m3u_content(input).entries;
        assert_eq!(channels[0].group, "Sports");
        assert_eq!(channels[1].group, "News");
    }

    #[test]
    fn captures_tvg_logo_and_group() {
        let input = "#EXTINF:-1 tvg-id=\"espn.us\" tvg-logo=\"http://logo.png\" group-title=\"HD\",ESPN\n\
             http://espn.example.com\n";
        let channels = parse_m3u_content(input).entries;
        assert_eq!(channels[0].logo, "http://logo.png");
        assert_eq!(channels[0].group, "HD");
    }

    #[test]
    fn empty_input_yields_empty() {
        assert!(parse_m3u_content("").entries.is_empty());
    }

    #[test]
    fn header_only_yields_empty() {
        assert!(parse_m3u_content("#EXTM3U\n").entries.is_empty());
    }

    #[test]
    fn url_without_extinf_uses_url_as_name() {
        let input = "http://stream.example.com/live\n";
        let channels = parse_m3u_content(input).entries;
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "http://stream.example.com/live");
    }

    #[test]
    fn skips_comments_and_blanks() {
        let input = "\
#EXTM3U

# a comment
#EXTINF:-1,Only
http://only.example.com

";
        let channels = parse_m3u_content(input).entries;
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Only");
    }

    #[test]
    fn group_resets_between_entries() {
        let input = "\
#EXTINF:-1 group-title=\"Sports\",ESPN
http://espn.example.com
#EXTINF:-1,NoGroup
http://nogroup.example.com
";
        let channels = parse_m3u_content(input).entries;
        assert_eq!(channels[0].group, "Sports");
        assert_eq!(channels[1].group, "");
    }

    #[test]
    fn trims_whitespace_around_name_and_url() {
        let input = "  #EXTINF:-1,  Trimmed  \n  http://example.com  \n";
        let channels = parse_m3u_content(input).entries;
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Trimmed");
        assert_eq!(channels[0].url, "http://example.com");
    }

    #[test]
    fn display_name_skips_commas_inside_attributes() {
        let input = "#EXTINF:-1 tvg-id=\"x,y\",Real Name\nhttp://u.example.com\n";
        let channels = parse_m3u_content(input).entries;
        assert_eq!(channels[0].name, "Real Name");
    }

    #[test]
    fn extract_attr_finds_quoted_value() {
        let line = r#"#EXTINF:-1 tvg-id="" group-title="Sports",ESPN"#;
        assert_eq!(extract_attr(line, "group-title"), Some("Sports".into()));
    }

    #[test]
    fn extract_attr_returns_none_when_missing() {
        let line = "#EXTINF:-1,Channel Name";
        assert_eq!(extract_attr(line, "group-title"), None);
    }

    #[test]
    fn extract_attr_returns_empty_for_empty_value() {
        let line = r#"#EXTINF:-1 group-title="",Test"#;
        assert_eq!(extract_attr(line, "group-title"), Some(String::new()));
    }

    #[test]
    fn extract_attr_allows_spaces_in_value() {
        let line = r#"#EXTINF:-1 group-title="News & Sports",CNN"#;
        assert_eq!(
            extract_attr(line, "group-title"),
            Some("News & Sports".into())
        );
    }

    #[test]
    fn extract_attr_matches_whole_names() {
        for (line, expected) in [
            (r#"#EXTINF:-1 x-tvg-logo="a" tvg-logo="b",N"#, Some("b")),
            (r#"#EXTINF:-1 x-tvg-logo="a",N"#, None),
            (
                r#"#EXTINF:-1 tvg-name="tvg-logo=" tvg-logo="b",N"#,
                Some("b"),
            ),
            (r#"#EXTINF:-1 tvg-name=" tvg-logo=",N"#, None),
            (r#"#EXTINF:-1,Name tvg-logo="c""#, None),
        ] {
            assert_eq!(
                extract_attr(line, "tvg-logo").as_deref(),
                expected,
                "{line}"
            );
        }
    }

    #[test]
    fn entries_past_the_cap_are_dropped() {
        let at_cap = "http://a/\n".repeat(100_000);
        let parsed = parse_m3u_content(&at_cap);
        assert_eq!((parsed.entries.len(), parsed.truncated), (100_000, false));
        let parsed = parse_m3u_content(&format!("{at_cap}http://b/\n"));
        assert_eq!((parsed.entries.len(), parsed.truncated), (100_000, true));
        assert_eq!(parsed.entries.last().unwrap().url, "http://a/");
    }
}
