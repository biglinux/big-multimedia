use super::*;

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
