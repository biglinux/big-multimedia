use super::*;

#[test]
fn scan_folder_flat_filters_media_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.mp3"), "").unwrap();
    std::fs::write(dir.path().join("b.txt"), "").unwrap();
    std::fs::write(dir.path().join("c.mp4"), "").unwrap();
    let files = scan_folder_flat(dir.path());
    assert_eq!(files.len(), 2);
    assert!(files.iter().any(|file| file.ends_with("a.mp3")));
    assert!(files.iter().any(|file| file.ends_with("c.mp4")));
}

#[test]
fn list_subfolders_returns_visible_directories_only() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("albums")).unwrap();
    std::fs::create_dir(dir.path().join(".cache")).unwrap();
    let dirs = list_subfolders(dir.path());
    assert_eq!(dirs.len(), 1);
    assert!(dirs[0].ends_with("albums"));
}
