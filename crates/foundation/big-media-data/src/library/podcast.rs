//! RSS 2.0 podcast feed parser (`<enclosure>` audio items).

use crate::error::Result;
use big_os_kit::deferred::Document;
use big_os_kit::http_client::http_get_with_retry;
use big_os_kit::url::ListOrigin;
use roxmltree::Node;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PodcastFeed {
    pub title: String,
    pub description: String,
    pub image_url: Option<String>,
    pub episodes: Vec<PodcastEpisode>,
    /// Episodes left out because their audio address is not a stream URI.
    pub skipped: usize,
}

#[derive(Debug, Clone)]
pub struct PodcastEpisode {
    pub title: String,
    pub description: String,
    pub audio_url: String,
    pub duration: Option<String>,
    pub pub_date: Option<String>,
}

/// Blocking; call from a background thread.
pub fn fetch_feed(feed_url: &str) -> Option<PodcastFeed> {
    let body = http_get_with_retry(feed_url)?;
    parse_feed(&body)
}

pub fn parse_feed(xml: &str) -> Option<PodcastFeed> {
    let doc = roxmltree::Document::parse(xml).ok()?;
    let channel = doc.descendants().find(|n| n.has_tag_name("channel"))?;

    let title = child_text(&channel, "title").unwrap_or_default();
    let description = child_text(&channel, "description").unwrap_or_default();

    // RSS spec image first, then itunes:image href fallback.
    let image_url = channel
        .children()
        .find(|n| n.has_tag_name("image"))
        .and_then(|img| child_text(&img, "url"))
        .or_else(|| {
            channel
                .children()
                .find(|n| n.tag_name().name() == "image")
                .and_then(|n| n.attribute("href").map(String::from))
        });

    let mut skipped = 0;
    let episodes = channel
        .children()
        .filter(|n| n.has_tag_name("item"))
        .filter_map(|episode_node| {
            let episode_title = child_text(&episode_node, "title").unwrap_or_default();
            let episode_description = child_text(&episode_node, "description")
                .or_else(|| child_text_ns(&episode_node, "summary"))
                .unwrap_or_default();
            let duration = child_text_ns(&episode_node, "duration");
            let pub_date = child_text(&episode_node, "pubDate");

            let audio_url = episode_node.children().find_map(|enclosure_node| {
                if !enclosure_node.has_tag_name("enclosure") {
                    return None;
                }
                let enclosure_media_type = enclosure_node.attribute("type").unwrap_or("");
                if enclosure_media_type.starts_with("audio/") || !enclosure_media_type.contains('/')
                {
                    enclosure_node.attribute("url").map(String::from)
                } else {
                    None
                }
            })?;
            if !big_os_kit::url::accepts_list_entry(&audio_url, ListOrigin::Network) {
                skipped += 1;
                return None;
            }

            Some(PodcastEpisode {
                title: episode_title,
                description: episode_description,
                audio_url,
                duration,
                pub_date,
            })
        })
        .collect();

    Some(PodcastFeed {
        title,
        description,
        image_url,
        episodes,
        skipped,
    })
}

fn child_text(parent: &Node, tag_name: &str) -> Option<String> {
    parent
        .children()
        .find(|n| n.has_tag_name(tag_name))
        .and_then(|n| n.text().map(|t| t.trim().to_string()))
        .filter(|s| !s.is_empty())
}

/// Local-name match: ignores `itunes:` and other namespace prefixes.
fn child_text_ns(parent: &Node, local_name: &str) -> Option<String> {
    parent
        .children()
        .find(|n| n.tag_name().name() == local_name)
        .and_then(|n| n.text().map(|t| t.trim().to_string()))
        .filter(|s| !s.is_empty())
}

/// Subscription list and title cache are small program state.
const MAX_STATE_BYTES: usize = 1024 * 1024;

fn state_path(name: &str) -> Option<PathBuf> {
    big_os_kit::xdg_dirs::config_dir()
        .map(|dir| dir.join(crate::config::audio::CONFIG_DIR).join(name))
}

fn read_state(name: &str) -> Option<String> {
    let bytes =
        big_os_kit::storage::read_regular_limited(&state_path(name)?, MAX_STATE_BYTES).ok()??;
    String::from_utf8(bytes).ok()
}

const SUBSCRIPTIONS_FILE: &str = "podcast-subscriptions.txt";

/// Subscribed feed URLs and the writer that publishes them. It exists only
/// after a confirmed read, so a save never replaces a list that could not be read.
#[derive(Debug)]
pub struct Subscriptions {
    pub urls: Vec<String>,
    document: Document,
}

impl Subscriptions {
    /// Admit a save of [`Subscriptions::urls`]; saves run one at a time and the
    /// latest list wins.
    ///
    /// # Errors
    /// Returns the refusal of [`big_os_kit::deferred::Document::replace`]: an
    /// earlier failed save or a full background lane.
    pub fn save(&self) -> Result<()> {
        self.document.replace(self.urls.join("\n").into_bytes())?;
        Ok(())
    }
}

/// A missing file is an empty list.
///
/// # Errors
/// Returns an error when the configuration directory is unknown, or the file
/// is unreadable, not a regular file, over 1 MiB or not UTF-8.
pub fn load_subscriptions() -> Result<Subscriptions> {
    let path = state_path(SUBSCRIPTIONS_FILE).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "configuration directory unavailable",
        )
    })?;
    load_subscriptions_from(path)
}

fn load_subscriptions_from(path: PathBuf) -> Result<Subscriptions> {
    let bytes = big_os_kit::storage::read_regular_limited(&path, MAX_STATE_BYTES)?;
    let urls = match &bytes {
        Some(bytes) => std::str::from_utf8(bytes)?
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        None => Vec::new(),
    };
    Ok(Subscriptions {
        urls,
        document: Document::reviewed(path, bytes),
    })
}

const TITLE_CACHE_FILE: &str = "podcast-title-cache.json";

/// `[feed_url, title]` pairs, oldest first.
fn read_titles() -> Vec<(String, String)> {
    read_state(TITLE_CACHE_FILE)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// `{feed_url: title}` map cached so the sidebar shows the name before the
/// feed has been re-fetched.
pub fn load_title_cache() -> std::collections::HashMap<String, String> {
    read_titles().into_iter().collect()
}

/// Feeds resolve on parallel workers; the lock keeps one read-modify-write
/// from dropping another's title.
pub fn cache_feed_title(feed_url: &str, title: &str) {
    static UPDATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _update = UPDATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut titles = read_titles();
    titles.retain(|(url, _)| url != feed_url);
    titles.push((feed_url.to_string(), title.to_string()));
    if let (Some(path), Ok(json)) = (state_path(TITLE_CACHE_FILE), title_cache_json(&titles))
        && let Err(error) = big_os_kit::storage::atomic_write(&path, &json)
    {
        log::warn!("podcast title cache not saved: {error}");
    }
}

/// The newest of `titles` that fit in [`MAX_STATE_BYTES`], so the file stays
/// readable.
fn title_cache_json(titles: &[(String, String)]) -> serde_json::Result<Vec<u8>> {
    // `[` and `]`, then `["url","title"],` per pair; the last comma is not written.
    let mut size = 1;
    let mut first_kept = titles.len();
    for (index, pair) in titles.iter().enumerate().rev() {
        size += serde_json::to_vec(pair)?.len() + 1;
        if size > MAX_STATE_BYTES {
            break;
        }
        first_kept = index;
    }
    serde_json::to_vec(&titles[first_kept..])
}

#[derive(Debug, Clone)]
pub struct PodcastSearchResult {
    pub title: String,
    pub author: String,
    pub feed_url: String,
    pub artwork_url: String,
    pub genre: String,
    pub episode_count: u32,
}

/// PodcastIndex Apple-replacement endpoint; no API key required.
/// Blocking; call from a background thread.
pub fn search_podcasts(query: &str) -> Option<Vec<PodcastSearchResult>> {
    if query.trim().is_empty() {
        return Some(Vec::new());
    }

    let encoded = big_os_kit::url::encode_query_value(query);
    let url = format!("https://api.podcastindex.org/search?term={encoded}");
    let body = http_get_with_retry(&url)?;
    parse_search_results(&body)
}

/// Items missing `title` or `feedUrl` are dropped; other fields default to
/// `""` / `0`. `None` only on unparsable JSON or missing `results` array.
fn parse_search_results(body: &str) -> Option<Vec<PodcastSearchResult>> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let results = json
        .get("results")?
        .as_array()?
        .iter()
        .filter_map(parse_podcast_search_result)
        .collect();
    Some(results)
}

fn parse_podcast_search_result(
    search_result_json: &serde_json::Value,
) -> Option<PodcastSearchResult> {
    let title = search_result_json
        .get("collectionName")
        .or_else(|| search_result_json.get("trackName"))
        .and_then(|v| v.as_str())?
        .trim()
        .to_string();
    let feed_url = search_result_json
        .get("feedUrl")
        .and_then(|v| v.as_str())?
        .trim()
        .to_string();
    if title.is_empty() || feed_url.is_empty() {
        return None;
    }
    let author = search_result_json
        .get("artistName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let artwork_url = search_result_json
        .get("artworkUrl100")
        .or_else(|| search_result_json.get("artworkUrl600"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let genre = search_result_json
        .get("primaryGenreName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let episode_count = search_result_json
        .get("trackCount")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;

    Some(PodcastSearchResult {
        title,
        author,
        feed_url,
        artwork_url,
        genre,
        episode_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle by hand: each pair is `["u000000","x…"]`, 1014 bytes plus a
    /// comma, so 1 + 1015 * 1033 bytes is the most that fits in 1 MiB.
    #[test]
    fn title_cache_drops_oldest_past_cap() {
        let titles: Vec<(String, String)> = (0..2000)
            .map(|index| (format!("u{index:06}"), "x".repeat(1000)))
            .collect();
        let json = title_cache_json(&titles).unwrap();
        assert_eq!(json.len(), 1 + 1015 * 1033);
        let kept: Vec<(String, String)> = serde_json::from_slice(&json).unwrap();
        assert_eq!(kept, titles[2000 - 1033..]);
    }

    #[test]
    fn child_text_finds_tag() {
        let xml = r"<root><title>Hello</title></root>";
        let doc = roxmltree::Document::parse(xml).unwrap();
        let root = doc.root_element();
        assert_eq!(child_text(&root, "title"), Some("Hello".into()));
    }

    #[test]
    fn child_text_missing_returns_none() {
        let xml = r"<root><other>x</other></root>";
        let doc = roxmltree::Document::parse(xml).unwrap();
        let root = doc.root_element();
        assert_eq!(child_text(&root, "title"), None);
    }

    #[test]
    fn parse_feed_minimal() {
        let xml = r#"<?xml version="1.0"?>
<rss><channel>
<title>My Podcast</title>
<description>A great show</description>
<item>
<title>Episode 1</title>
<description>First ep</description>
<enclosure url="https://example.com/ep1.mp3" type="audio/mpeg"/>
</item>
</channel></rss>"#;

        let feed = parse_feed(xml).unwrap();
        assert_eq!(feed.title, "My Podcast");
        assert_eq!(feed.description, "A great show");
        assert_eq!(feed.episodes.len(), 1);
        assert_eq!(feed.episodes[0].title, "Episode 1");
        assert_eq!(feed.episodes[0].audio_url, "https://example.com/ep1.mp3");
    }

    #[test]
    fn enclosures_must_be_streams() {
        let mut items = String::new();
        for url in [
            "av://lavfi:sine,ametadata=mode=print:file=/tmp/x",
            "fdclose://0",
            "/home/user/song.mp3",
            "file:///etc/hostname",
            "HTTPS://example.com/ep.mp3",
            "rtsp://example.com/live",
        ] {
            items.push_str(&format!(
                "<item><title>t</title><enclosure url=\"{url}\" type=\"audio/mpeg\"/></item>"
            ));
        }
        let feed = parse_feed(&format!("<rss><channel>{items}</channel></rss>")).unwrap();
        let urls: Vec<&str> = feed.episodes.iter().map(|e| e.audio_url.as_str()).collect();
        assert_eq!(
            (urls, feed.skipped),
            (
                vec!["HTTPS://example.com/ep.mp3", "rtsp://example.com/live"],
                4
            )
        );
    }

    #[test]
    fn parse_feed_no_channel_returns_none() {
        assert!(parse_feed("<rss></rss>").is_none());
    }

    #[test]
    fn parse_feed_cdata_titles() {
        let xml = r#"<rss><channel>
<title><![CDATA[CDATA Title]]></title>
<description>desc</description>
<item>
<title><![CDATA[Ep CDATA]]></title>
<description>d</description>
<enclosure url="https://x.com/a.mp3" type="audio/mpeg"/>
</item>
</channel></rss>"#;

        let feed = parse_feed(xml).unwrap();
        assert_eq!(feed.title, "CDATA Title");
        assert_eq!(feed.episodes[0].title, "Ep CDATA");
    }

    #[test]
    fn parse_feed_itunes_image_fallback() {
        let xml = r#"<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel>
<title>T</title>
<description>D</description>
<itunes:image href="https://img.example.com/art.jpg"/>
<item>
<title>E</title>
<description>D</description>
<enclosure url="https://x.com/e.mp3" type="audio/mpeg"/>
</item>
</channel></rss>"#;

        let feed = parse_feed(xml).unwrap();
        assert_eq!(
            feed.image_url,
            Some("https://img.example.com/art.jpg".into())
        );
    }

    #[test]
    fn parse_feed_skip_video_enclosure() {
        let xml = r#"<rss><channel>
<title>T</title>
<description>D</description>
<item>
<title>Video Only</title>
<description>D</description>
<enclosure url="https://x.com/v.mp4" type="video/mp4"/>
</item>
</channel></rss>"#;

        let feed = parse_feed(xml).unwrap();
        assert!(feed.episodes.is_empty());
    }

    #[test]
    fn parse_feed_multiple_episodes() {
        let xml = r#"<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel>
<title>T</title>
<description>D</description>
<item>
<title>Ep1</title><description>D1</description>
<enclosure url="https://x.com/1.mp3" type="audio/mpeg"/>
<itunes:duration>12:34</itunes:duration>
<pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
</item>
<item>
<title>Ep2</title><description>D2</description>
<enclosure url="https://x.com/2.mp3" type="audio/mpeg"/>
</item>
</channel></rss>"#;

        let feed = parse_feed(xml).unwrap();
        assert_eq!(feed.episodes.len(), 2);
        assert_eq!(feed.episodes[0].duration, Some("12:34".into()));
        assert_eq!(
            feed.episodes[0].pub_date,
            Some("Mon, 01 Jan 2024 00:00:00 GMT".into())
        );
        assert_eq!(feed.episodes[1].duration, None);
    }

    #[test]
    fn parse_feed_no_type_enclosure_accepted() {
        let xml = r#"<rss><channel>
<title>T</title>
<description>D</description>
<item>
<title>E</title>
<description>D</description>
<enclosure url="https://x.com/e.mp3"/>
</item>
</channel></rss>"#;

        let feed = parse_feed(xml).unwrap();
        assert_eq!(feed.episodes.len(), 1);
        assert_eq!(feed.episodes[0].audio_url, "https://x.com/e.mp3");
    }

    #[test]
    fn child_text_ns_matches_namespaced_local_name() {
        let xml = r#"<root xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd">
<itunes:summary>wrapped in ns</itunes:summary>
</root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let root = doc.root_element();
        assert_eq!(
            child_text_ns(&root, "summary"),
            Some("wrapped in ns".into())
        );
    }

    #[test]
    fn child_text_ns_trims_and_filters_empty() {
        let xml = r#"<root xmlns:itunes="http://x">
<itunes:a>   </itunes:a>
<itunes:b>  ok  </itunes:b>
</root>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let root = doc.root_element();
        assert_eq!(child_text_ns(&root, "a"), None);
        assert_eq!(child_text_ns(&root, "b"), Some("ok".into()));
    }

    #[test]
    fn feed_falls_back_to_itunes_summary() {
        let xml = r#"<rss xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel>
<title>T</title>
<description>D</description>
<item>
<title>E</title>
<itunes:summary>the long summary</itunes:summary>
<enclosure url="https://x.com/e.mp3" type="audio/mpeg"/>
</item>
</channel></rss>"#;
        let feed = parse_feed(xml).unwrap();
        assert_eq!(feed.episodes.len(), 1);
        assert_eq!(feed.episodes[0].description, "the long summary");
    }

    #[test]
    fn parse_search_results_reads_apple_shape() {
        let body = r#"{
  "results": [
    {
      "collectionName": "Podcast A",
      "artistName": "Author A",
      "feedUrl": "https://a.example/feed.xml",
      "artworkUrl100": "https://a.example/art.jpg",
      "primaryGenreName": "Technology",
      "trackCount": 42
    }
  ]
}"#;
        let results = parse_search_results(body).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Podcast A");
        assert_eq!(results[0].author, "Author A");
        assert_eq!(results[0].feed_url, "https://a.example/feed.xml");
        assert_eq!(results[0].artwork_url, "https://a.example/art.jpg");
        assert_eq!(results[0].genre, "Technology");
        assert_eq!(results[0].episode_count, 42);
    }

    #[test]
    fn search_drops_untitled_or_feedless_items() {
        let body = r#"{
  "results": [
    { "collectionName": "Valid", "feedUrl": "https://x/y" },
    { "collectionName": "No feed" },
    { "feedUrl": "https://z/a" },
    { "trackName": "Fallback Title", "feedUrl": "https://z/b" }
  ]
}"#;
        let results = parse_search_results(body).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Valid");
        assert_eq!(results[1].title, "Fallback Title");
    }

    #[test]
    fn parse_search_results_handles_malformed_json() {
        assert!(parse_search_results("not json").is_none());
        assert!(parse_search_results("{}").is_none());
        assert!(parse_search_results(r#"{"results": "not an array"}"#).is_none());
    }

    #[test]
    fn search_defaults_absent_optional_fields() {
        let body = r#"{
  "results": [
    { "collectionName": "Minimal", "feedUrl": "https://m/f" }
  ]
}"#;
        let results = parse_search_results(body).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].author, "");
        assert_eq!(results[0].artwork_url, "");
        assert_eq!(results[0].genre, "");
        assert_eq!(results[0].episode_count, 0);
    }

    #[test]
    fn unreadable_subscriptions_refused() {
        let dir = tempfile::tempdir().unwrap();
        let invalid_utf8 = dir.path().join("invalid-utf8");
        std::fs::write(&invalid_utf8, b"https://a.test/feed\n\xff").unwrap();
        let over_cap = dir.path().join("over-cap");
        std::fs::write(&over_cap, vec![b'a'; MAX_STATE_BYTES + 1]).unwrap();
        let directory = dir.path().join("directory");
        std::fs::create_dir(&directory).unwrap();
        let symlink = dir.path().join("symlink");
        std::os::unix::fs::symlink(&invalid_utf8, &symlink).unwrap();

        for path in [invalid_utf8, over_cap, directory, symlink] {
            assert!(
                load_subscriptions_from(path.clone()).is_err(),
                "{} loaded",
                path.display()
            );
        }
        let missing = load_subscriptions_from(dir.path().join("missing")).unwrap();
        assert!(missing.urls.is_empty());
    }

    #[test]
    fn saves_publish_latest_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SUBSCRIPTIONS_FILE);
        std::fs::write(&path, "https://a.test/feed\n").unwrap();
        let mut subscriptions = load_subscriptions_from(path.clone()).unwrap();
        assert_eq!(subscriptions.urls, ["https://a.test/feed"]);

        for url in ["https://b.test/feed", "https://c.test/feed"] {
            subscriptions.urls.push(url.into());
            subscriptions.save().unwrap();
        }
        let expected = "https://a.test/feed\nhttps://b.test/feed\nhttps://c.test/feed";
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::fs::read_to_string(&path).unwrap() != expected
            && std::time::Instant::now() < until
        {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
    }
}
