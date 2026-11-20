use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::media::playlist_scan::trash::{TrashEntry, newest_trashed};

/// Items in the trash: name, original path as bytes, deletion date.
const TRASH: &[(&str, Option<&[u8]>, Option<i64>)] = &[
    ("first", Some(b"/videos/a.mp4"), Some(1_700_000_000)),
    ("second", Some(b"/videos/a.mp4"), Some(1_700_000_060)),
    ("undated", Some(b"/videos/a.mp4"), None),
    ("other", Some(b"/videos/b.mp4"), Some(1_800_000_000)),
    ("longer", Some(b"/videos/a.mp4.bak"), Some(1_900_000_000)),
    ("elsewhere", Some(b"/old/videos/a.mp4"), Some(1_900_000_000)),
    ("no origin", None, Some(1_900_000_000)),
    ("utf-8", Some("/videos/f\u{e9}rias.mp4".as_bytes()), Some(2)),
    ("latin-1", Some(b"/videos/f\xe9rias.mp4"), Some(1)),
    ("undated only", Some(b"/videos/c.mp4"), None),
];

/// Expected items follow the restore rule: the latest deletion date among
/// entries whose original path equals the wanted path byte for byte.
#[test]
fn restore_picks_newest_matching_item() {
    let cases: &[(&[u8], Option<&str>)] = &[
        (b"/videos/a.mp4", Some("second")),
        (b"/videos/b.mp4", Some("other")),
        (b"/videos/f\xe9rias.mp4", Some("latin-1")),
        (b"/videos/c.mp4", Some("undated only")),
        (b"/videos/d.mp4", None),
        (b"", None),
    ];
    let path = |bytes: &[u8]| Path::new(OsStr::from_bytes(bytes)).to_path_buf();
    for (wanted, expected) in cases {
        let entries = TRASH
            .iter()
            .map(|(item, original_path, deleted_at)| TrashEntry {
                item: *item,
                original_path: original_path.map(path),
                deleted_at: *deleted_at,
            })
            .collect();
        let chosen = newest_trashed(entries, &path(wanted));
        assert_eq!(chosen, *expected, "restore choice for {wanted:?}");
    }
    let empty: Vec<TrashEntry<()>> = Vec::new();
    assert_eq!(newest_trashed(empty, Path::new("/videos/a.mp4")), None);
}

#[test]
#[ignore = "requires an isolated home with GVfs"]
fn undo_restores_latest_trashing() {
    use crate::media::playlist::{restore_from_trash, trash_file};

    // A newline once forged a line of `gio trash --list`.
    let path = big_os_kit::xdg_dirs::home_dir()
        .expect("an absolute home folder")
        .join("trash undo\nprobe.txt");
    for content in ["older", "newer"] {
        std::fs::write(&path, content).unwrap();
        trash_file(&path).unwrap();
        // The trash records deletion dates to the second.
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }
    restore_from_trash(&path).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "newer");
    assert!(
        restore_from_trash(&path).is_err(),
        "replaced a file in the way"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "newer");
    std::fs::remove_file(&path).unwrap();
    restore_from_trash(&path).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "older");
    std::fs::remove_file(&path).unwrap();
    assert!(
        restore_from_trash(&path).is_err(),
        "restored from an empty trash"
    );
}
