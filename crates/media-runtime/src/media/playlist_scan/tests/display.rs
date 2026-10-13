use super::*;
use crate::media::playlist_scan::display::trashed_uri;

#[test]
fn breadcrumb_ends_at_folder() {
    let segs = breadcrumb_segments("/tmp/a/b");
    assert!(segs.len() >= 3);
    let last = segs.last().unwrap();
    assert_eq!(last.0, "b");
    assert_eq!(last.1, "/tmp/a/b");
}

#[test]
fn breadcrumb_starts_at_root_or_home() {
    let segs = breadcrumb_segments("/tmp/test");
    let first = &segs[0].0;
    assert!(
        first == "/" || first == "Home",
        "expected / or Home, got {first}"
    );
}

#[test]
fn trashed_uri_matches_whole_path() {
    let listing = "trash:///a.mp4.bak\t/videos/a.mp4.bak\n\
                   trash:///b.mp4\t/old/videos/a.mp4\n\
                   trash:///My%20a.mp4\t/videos/My a.mp4\n\
                   trash:///a.mp4\t/videos/a.mp4\n";
    assert_eq!(
        trashed_uri(listing, "/videos/a.mp4"),
        Some("trash:///a.mp4")
    );
    assert_eq!(
        trashed_uri(listing, "/videos/My a.mp4"),
        Some("trash:///My%20a.mp4")
    );
    assert_eq!(trashed_uri(listing, "/videos/c.mp4"), None);
}
