use super::*;

#[test]
fn shuffled_order_preserves_count() {
    let order = shuffled_order(10, 0);
    assert_eq!(order.len(), 10);
}

#[test]
fn shuffled_order_starts_with_current() {
    let order = shuffled_order(10, 5);
    assert_eq!(order[0], 5);
}

#[test]
fn shuffled_order_contains_all_indices() {
    let order = shuffled_order(10, 3);
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..10).collect::<Vec<_>>());
}

#[test]
fn shuffled_order_keeps_single_index() {
    let order = shuffled_order(1, 0);
    assert_eq!(order, vec![0]);
}

#[test]
fn sort_paths_by_name() {
    let mut paths = vec![
        "/videos/charlie.mp4".into(),
        "/videos/alice.mp4".into(),
        "/videos/bob.mp4".into(),
    ];
    sort_paths(&mut paths, "name", false);
    assert_eq!(
        paths,
        vec![
            "/videos/alice.mp4",
            "/videos/bob.mp4",
            "/videos/charlie.mp4"
        ]
    );
}

#[test]
fn sort_paths_by_name_descending() {
    let mut paths = vec![
        "/videos/alice.mp4".into(),
        "/videos/charlie.mp4".into(),
        "/videos/bob.mp4".into(),
    ];
    sort_paths(&mut paths, "name", true);
    assert_eq!(
        paths,
        vec![
            "/videos/charlie.mp4",
            "/videos/bob.mp4",
            "/videos/alice.mp4"
        ]
    );
}

#[test]
fn apply_permutation_identity() {
    let mut paths = vec!["a".into(), "b".into(), "c".into()];
    crate::media::playlist_scan::ordering::apply_permutation(&mut paths, &[0, 1, 2]);
    assert_eq!(paths, vec!["a", "b", "c"]);
}

#[test]
fn apply_permutation_reverse() {
    let mut paths = vec!["a".into(), "b".into(), "c".into()];
    crate::media::playlist_scan::ordering::apply_permutation(&mut paths, &[2, 1, 0]);
    assert_eq!(paths, vec!["c", "b", "a"]);
}

#[test]
fn apply_permutation_cycle() {
    let mut paths: Vec<String> = vec!["a".into(), "b".into(), "c".into(), "d".into()];
    crate::media::playlist_scan::ordering::apply_permutation(&mut paths, &[1, 2, 3, 0]);
    assert_eq!(paths, vec!["b", "c", "d", "a"]);
}

#[test]
fn apply_permutation_single_element() {
    let mut paths = vec!["only".to_string()];
    crate::media::playlist_scan::ordering::apply_permutation(&mut paths, &[0]);
    assert_eq!(paths, vec!["only"]);
}

#[test]
fn apply_permutation_empty() {
    let mut paths: Vec<String> = vec![];
    crate::media::playlist_scan::ordering::apply_permutation(&mut paths, &[]);
    assert!(paths.is_empty());
}

#[test]
fn apply_permutation_swap_two() {
    let mut paths: Vec<String> = vec!["first".into(), "second".into()];
    crate::media::playlist_scan::ordering::apply_permutation(&mut paths, &[1, 0]);
    assert_eq!(paths, vec!["second", "first"]);
}
