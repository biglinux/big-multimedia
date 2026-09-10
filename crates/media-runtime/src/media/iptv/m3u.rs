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
//! lines. Attribute parsing is intentionally permissive — IPTV playlists in
//! the wild are sloppy and strict parsing just loses channels.

use std::path::Path;

use super::channel::IptvChannel;

/// Parse an in-memory M3U/M3U8 playlist string into channel entries.
///
/// Returns an empty vector when the input has no URLs.
#[must_use]
pub fn parse_m3u_content(content: &str) -> Vec<IptvChannel> {
    let mut channels = Vec::new();
    let mut pending = IptvChannel::default();

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("#EXTINF:") {
            pending = IptvChannel {
                name: extract_display_name(line),
                group: extract_attr(line, "group-title").unwrap_or_default(),
                logo: extract_attr(line, "tvg-logo").unwrap_or_default(),
                tvg_id: extract_attr(line, "tvg-id").unwrap_or_default(),
                url: String::new(),
            };
        } else if !line.is_empty() && !line.starts_with('#') {
            let mut ch = std::mem::take(&mut pending);
            if ch.name.is_empty() {
                ch.name = line.to_string();
            }
            ch.url = line.to_string();
            channels.push(ch);
        }
    }

    channels
}

/// Read and parse an M3U/M3U8 file from disk.
///
/// Returns an empty vector on read error — callers that need to distinguish
/// "file missing" from "valid but empty" should check existence first.
#[must_use]
pub fn parse_m3u_file(path: &Path) -> Vec<IptvChannel> {
    std::fs::read_to_string(path)
        .map(|s| parse_m3u_content(&s))
        .unwrap_or_default()
}

fn extract_attr(line: &str, attr: &str) -> Option<String> {
    let pattern = format!("{attr}=\"");
    let start = line.find(&pattern)?;
    let rest = &line[start + pattern.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_display_name(line: &str) -> String {
    line.rfind(',')
        .map(|pos| line[pos + 1..].trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_channels() {
        let input = "\
#EXTM3U
#EXTINF:-1,Channel One
http://stream1.example.com/live
#EXTINF:-1,Channel Two
http://stream2.example.com/live
";
        let channels = parse_m3u_content(input);
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
        let channels = parse_m3u_content(input);
        assert_eq!(channels[0].group, "Sports");
        assert_eq!(channels[1].group, "News");
    }

    #[test]
    fn captures_tvg_logo_and_id() {
        let input = "#EXTINF:-1 tvg-id=\"espn.us\" tvg-logo=\"http://logo.png\" group-title=\"HD\",ESPN\n\
             http://espn.example.com\n";
        let channels = parse_m3u_content(input);
        assert_eq!(channels[0].tvg_id, "espn.us");
        assert_eq!(channels[0].logo, "http://logo.png");
        assert_eq!(channels[0].group, "HD");
    }

    #[test]
    fn empty_input_yields_empty() {
        assert!(parse_m3u_content("").is_empty());
    }

    #[test]
    fn header_only_yields_empty() {
        assert!(parse_m3u_content("#EXTM3U\n").is_empty());
    }

    #[test]
    fn url_without_extinf_uses_url_as_name() {
        let input = "http://stream.example.com/live\n";
        let channels = parse_m3u_content(input);
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
        let channels = parse_m3u_content(input);
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
        let channels = parse_m3u_content(input);
        assert_eq!(channels[0].group, "Sports");
        assert_eq!(channels[1].group, "");
    }

    #[test]
    fn trims_whitespace_around_name_and_url() {
        let input = "  #EXTINF:-1,  Trimmed  \n  http://example.com  \n";
        let channels = parse_m3u_content(input);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Trimmed");
        assert_eq!(channels[0].url, "http://example.com");
    }

    #[test]
    fn display_name_is_after_last_comma() {
        let input = "#EXTINF:-1 tvg-id=\"x,y\",Real Name\nhttp://u.example.com\n";
        let channels = parse_m3u_content(input);
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
}
