use super::*;

#[test]
fn format_file_size_bytes() {
    assert_eq!(format_file_size(512), "512 B");
}

#[test]
fn format_file_size_kilobytes() {
    assert_eq!(format_file_size(2048), "2.0 KB");
}

#[test]
fn format_file_size_megabytes() {
    assert_eq!(format_file_size(5 * 1024 * 1024), "5.00 MB");
}

#[test]
fn format_file_size_gigabytes() {
    assert_eq!(format_file_size(2 * 1024 * 1024 * 1024), "2.00 GB");
}

#[test]
fn format_file_size_zero() {
    assert_eq!(format_file_size(0), "0 B");
}

#[test]
fn format_file_size_boundary_kb() {
    assert_eq!(format_file_size(1023), "1023 B");
    assert_eq!(format_file_size(1024), "1.0 KB");
}

#[test]
fn format_file_size_boundary_mb() {
    assert_eq!(format_file_size(1024 * 1024 - 1), "1024.0 KB");
    assert_eq!(format_file_size(1024 * 1024), "1.00 MB");
}

#[test]
fn folder_display_name_basename() {
    assert_eq!(folder_display_name("/home/user/Videos"), "Videos");
}

#[test]
fn folder_display_name_root() {
    assert_eq!(folder_display_name("/"), "/");
}

#[test]
fn folder_display_name_simple() {
    assert_eq!(folder_display_name("myfolder"), "myfolder");
}

#[test]
fn breadcrumb_segments_structure() {
    let segs = breadcrumb_segments("/tmp/a/b");
    assert!(segs.len() >= 3);
    let last = segs.last().unwrap();
    assert_eq!(last.0, "b");
    assert_eq!(last.1, "/tmp/a/b");
}

#[test]
fn breadcrumb_segments_first_is_root_or_home() {
    let segs = breadcrumb_segments("/tmp/test");
    let first = &segs[0].0;
    assert!(
        first == "/" || first == "Home",
        "expected / or Home, got {first}"
    );
}
