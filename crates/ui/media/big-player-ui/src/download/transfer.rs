//! HTTP download worker: streamed read with cancel polling, progress counters
//! and the file name derived from the URL.

use super::DownloadProgress;
use std::sync::atomic::Ordering;

/// Stream `url` to `dest`, updating `progress` and polling its cancel flag.
/// Returns `Err` with a human-readable message on network/IO failure or cancel.
pub fn download_file(
    url: &str,
    dest: &std::path::Path,
    progress: &DownloadProgress,
) -> Result<(), String> {
    use big_os_kit::http_client::{RequestHeaders, copy_stream_with_progress, http_get_stream};

    let mut download_response = http_get_stream(
        url,
        &RequestHeaders::default(),
        std::time::Duration::from_secs(15),
    )?;
    if !(200..300).contains(&download_response.status) {
        return Err(format!("HTTP {}", download_response.status));
    }

    if let Some(total_bytes) = download_response.content_length {
        progress.total_bytes.store(total_bytes, Ordering::Relaxed);
    }

    let stage = big_os_kit::storage::StagedFile::new(dest).map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(stage.path()).map_err(|e| e.to_string())?;
    copy_stream_with_progress(
        &mut download_response.reader,
        &mut file,
        || progress.cancelled.load(Ordering::Relaxed),
        |transferred_bytes| {
            progress
                .bytes_downloaded
                .fetch_add(transferred_bytes, Ordering::Relaxed);
        },
    )?;
    drop(file);
    if progress.cancelled.load(Ordering::Relaxed) {
        return Err("Download cancelled".into());
    }
    std::fs::rename(stage.path(), dest).map_err(|e| e.to_string())
}

/// Falls back to `<fallback_title>.mp3` when the URL has no recognizable filename.
pub fn filename_from_url(url: &str, fallback_title: &str) -> String {
    // The title comes from the feed: keep it a single path component.
    let title = fallback_title.replace(['/', '\0'], "_");
    let title = if title.trim_matches('.').is_empty() {
        "episode"
    } else {
        &title
    };
    big_os_kit::url::filename_from_url_path(url, &format!("{title}.mp3"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_publish_only_complete_results_and_clean_staging() {
        use std::io::{Read, Write};
        use std::sync::atomic::AtomicBool;
        let dir = std::env::temp_dir().join(format!("download-transaction-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let dest = dir.join("episode.mp3");
        for size in [0, 5, 25] {
            for mode in ["success", "cancel", "http-error"] {
                std::fs::write(&dest, b"previous download").unwrap();
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let url = format!("http://{}/episode.mp3", listener.local_addr().unwrap());
                let body = vec![b'x'; size];
                let reply = body.clone();
                let server = std::thread::spawn(move || {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .unwrap();
                    let mut request = [0; 2048];
                    assert!(stream.read(&mut request).unwrap() > 0);
                    let status = if mode == "http-error" {
                        "503 Unavailable"
                    } else {
                        "200 OK"
                    };
                    let header = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        reply.len()
                    );
                    stream.write_all(header.as_bytes()).unwrap();
                    let _ = stream.write_all(&reply);
                });
                let progress = DownloadProgress {
                    cancelled: AtomicBool::new(mode == "cancel"),
                    ..Default::default()
                };
                let result = download_file(&url, &dest, &progress);
                server.join().unwrap();
                if mode == "success" {
                    assert!(result.is_ok(), "{result:?}");
                    assert_eq!(std::fs::read(&dest).unwrap(), body);
                } else {
                    assert!(result.is_err());
                    assert_eq!(std::fs::read(&dest).unwrap(), b"previous download");
                }
                assert_eq!(
                    std::fs::read_dir(&dir).unwrap().count(),
                    1,
                    "staging survived completion"
                );
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_filename_standard_url() {
        assert_eq!(
            filename_from_url("http://example.com/episodes/ep01.mp3", "Episode 1"),
            "ep01.mp3"
        );
    }

    #[test]
    fn test_filename_url_with_query_params() {
        assert_eq!(
            filename_from_url("http://example.com/ep01.mp3?token=abc&t=123", "Episode 1"),
            "ep01.mp3"
        );
    }

    #[test]
    fn test_filename_trailing_slash() {
        assert_eq!(
            filename_from_url("http://example.com/path/", "My Episode"),
            "My Episode.mp3"
        );
    }

    #[test]
    fn test_filename_no_extension() {
        assert_eq!(
            filename_from_url("http://example.com/no-ext", "Fallback Title"),
            "Fallback Title.mp3"
        );
    }

    #[test]
    fn test_filename_empty_url() {
        assert_eq!(filename_from_url("", "Title"), "Title.mp3");
        assert_eq!(filename_from_url("", "../../x"), ".._.._x.mp3");
        assert_eq!(filename_from_url("", ".."), "episode.mp3");
    }

    #[test]
    fn test_filename_deep_path() {
        assert_eq!(
            filename_from_url("http://cdn.example.com/a/b/c/d/episode.ogg", "ep"),
            "episode.ogg"
        );
    }

    #[test]
    fn test_filename_preserves_original_extension() {
        assert_eq!(filename_from_url("http://x.com/file.m4a", "ep"), "file.m4a");
    }
}
