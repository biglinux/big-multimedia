// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Playlist ordering helpers.
//!
//! Pure functions that build and apply playlist permutations. The shuffle
//! uses an internal xorshift PRNG seeded from the system clock, which is
//! fine for UI shuffle and unfit for cryptographic uses.

use std::cmp::Ordering;
use std::path::Path;

/// Compare names the way people read them: ignoring case, and with runs of
/// digits compared as numbers, so "Episode 2" comes before "Episode 10".
/// Names equal that way fall back to a plain comparison, so the order is
/// total.
#[must_use]
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut left, mut right) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(digit) = chars.next_if(char::is_ascii_digit) {
                        digits.push(digit);
                    }
                    digits
                };
                let (x, y) = (take(&mut left), take(&mut right));
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                left.next();
                right.next();
            }
        }
    }
}

/// [`natural_cmp`] folder by folder, so the files of a folder stay together
/// ahead of a sibling folder whose name merely sorts between them.
#[must_use]
pub fn natural_path_cmp(a: &str, b: &str) -> Ordering {
    let mut left = a.split('/');
    let mut right = b.split('/');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let order = natural_cmp(x, y);
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// Build a Fisher–Yates shuffle of `0..count` with `current_index` pinned at index 0.
///
/// The returned vector is a permutation of `0..count` whose first element
/// is always `current_index` (the currently-playing track), so shuffling
/// never restarts the listener mid-song.
#[must_use]
pub fn shuffled_order(count: usize, current_index: usize) -> Vec<usize> {
    let random_seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    shuffled_order_with_seed(count, current_index, random_seed)
}

fn shuffled_order_with_seed(
    count: usize,
    current_index: usize,
    mut random_state: u64,
) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).collect();

    for target_index in (1..count).rev() {
        let swap_index =
            (next_shuffle_random(&mut random_state) as usize) % shuffle_bucket_size(target_index);
        order.swap(target_index, swap_index);
    }

    if let Some(current_index_position) = order
        .iter()
        .position(|&playlist_index| playlist_index == current_index)
    {
        order.swap(0, current_index_position);
    }

    order
}

fn next_shuffle_random(random_state: &mut u64) -> u64 {
    *random_state ^= *random_state << 13;
    *random_state ^= *random_state >> 7;
    *random_state ^= *random_state << 17;
    *random_state
}

fn shuffle_bucket_size(target_index: usize) -> usize {
    target_index + 1
}

/// Sort `paths` in place by `sort_by` key and optional descending order.
///
/// Recognised keys are `"date"` (filesystem modified time), `"size"`
/// (filesystem length in bytes), and anything else, which falls back to a
/// case-insensitive sort on the file-stem. Filesystem errors on individual
/// entries silently sort those entries as `UNIX_EPOCH` or `0`.
pub fn sort_paths(paths: &mut [String], sort_key: &str, descending: bool) {
    match sort_key {
        "date" => sort_paths_by_metadata_key(paths, descending, |path| {
            std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH)
        }),
        "size" => sort_paths_by_metadata_key(paths, descending, |path| {
            std::fs::metadata(path).map_or(0, |metadata| metadata.len())
        }),
        _ => {
            paths.sort_by(|a, b| natural_cmp(&display_stem(a), &display_stem(b)));
            if descending {
                paths.reverse();
            }
        }
    }
}

/// Apply `permutation` to `playlist_entries` in place using a cycle-following swap.
///
/// `permutation[i]` is the *source* index of the element that should end up
/// at position `i`. `permutation` must be a valid permutation of
/// `0..playlist_entries.len()`; out-of-range indices cause a panic.
pub fn apply_permutation(playlist_entries: &mut [String], permutation: &[usize]) {
    assert!(
        permutation.len() >= playlist_entries.len(),
        "permutation must provide one source index for each playlist entry"
    );

    let original_playlist_entries = playlist_entries.to_vec();
    for (target_index, source_index) in permutation
        .iter()
        .take(playlist_entries.len())
        .copied()
        .enumerate()
    {
        playlist_entries[target_index].clone_from(&original_playlist_entries[source_index]);
    }
}

fn sort_paths_by_metadata_key<T: Ord>(
    paths: &mut [String],
    descending: bool,
    metadata_key_for_path: impl Fn(&str) -> T,
) {
    let mut indexed: Vec<(usize, T)> = paths
        .iter()
        .enumerate()
        .map(|(source_index, path)| (source_index, metadata_key_for_path(path)))
        .collect();
    indexed.sort_by(|left, right| left.1.cmp(&right.1));
    if descending {
        indexed.reverse();
    }
    apply_permutation(
        paths,
        &indexed.iter().map(|(idx, _)| *idx).collect::<Vec<_>>(),
    );
}

fn display_stem(path: &str) -> String {
    Path::new(path).file_stem().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_reads_numbers_as_numbers() {
        let mut names = vec![
            "Episode 10.mkv",
            "episode 2.mkv",
            "Episode 1.mkv",
            "10.mp3",
            "2.mp3",
            "02.mp3",
            "1.mp3",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            [
                "1.mp3",
                "02.mp3",
                "2.mp3",
                "10.mp3",
                "Episode 1.mkv",
                "episode 2.mkv",
                "Episode 10.mkv"
            ]
        );
    }

    #[test]
    fn natural_path_order_keeps_a_folder_together() {
        let mut paths = vec!["/m/a b/x.mkv", "/m/a/y.mkv", "/m/a/x.mkv", "/m/a2/x.mkv"];
        paths.sort_by(|a, b| natural_path_cmp(a, b));
        assert_eq!(
            paths,
            ["/m/a/x.mkv", "/m/a/y.mkv", "/m/a b/x.mkv", "/m/a2/x.mkv"]
        );
    }

    #[test]
    fn name_sort_is_natural() {
        let mut paths = vec!["/v/Ep 10.mkv".to_string(), "/v/Ep 9.mkv".to_string()];
        sort_paths(&mut paths, "name", false);
        assert_eq!(paths, ["/v/Ep 9.mkv", "/v/Ep 10.mkv"]);
    }
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime};

    static TEST_DIRECTORY_COUNTER: AtomicUsize = AtomicUsize::new(0);

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn create(test_name: &str) -> Self {
            let directory_index = TEST_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "big_media_components_playlist_order_{test_name}_{}_{}",
                std::process::id(),
                directory_index
            ));
            std::fs::create_dir_all(&path).expect("test temp directory must be created");
            Self { path }
        }

        fn file_with_contents(&self, name: &str, contents: &[u8]) -> String {
            let path = self.path.join(name);
            std::fs::write(&path, contents).expect("test file must be written");
            path_string(&path)
        }

        fn file_with_modified_time(
            &self,
            name: &str,
            contents: &[u8],
            modified_seconds: u64,
        ) -> String {
            let path = self.path.join(name);
            std::fs::write(&path, contents).expect("test file must be written");
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("test file must reopen for modified time");
            file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(modified_seconds))
                .expect("test file modified time must be set");
            path_string(&path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            if self.path.starts_with(std::env::temp_dir()) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
    }

    fn path_string(path: &Path) -> String {
        path.to_string_lossy().to_string()
    }

    #[test]
    fn shuffle_keeps_current_first_and_count() {
        let order = shuffled_order(10, 5);
        assert_eq!(order.len(), 10);
        assert_eq!(order[0], 5);
        let mut sorted = order;
        sorted.sort_unstable();
        assert_eq!(sorted, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn xorshift_sequence_matches_shuffle_contract() {
        let mut random_state = 0x1234_5678_9abc_def0;

        assert_eq!(
            next_shuffle_random(&mut random_state),
            0xfe80_0d65_69fa_1b4d
        );
        assert_eq!(
            next_shuffle_random(&mut random_state),
            0x0ce7_c497_a6b0_9c3b
        );
        assert_eq!(
            next_shuffle_random(&mut random_state),
            0x41df_b69a_11db_93c3
        );
        assert_eq!(shuffle_bucket_size(5), 6);
    }

    #[test]
    fn seeded_shuffle_is_deterministic() {
        assert_eq!(
            shuffled_order_with_seed(6, 2, 0x1234_5678_9abc_def0),
            vec![2, 4, 0, 3, 5, 1]
        );
        assert_eq!(
            shuffled_order_with_seed(5, 4, 0xfeed_face_cafe_beef),
            vec![4, 3, 1, 2, 0]
        );
        assert_eq!(shuffled_order_with_seed(4, 99, 0x1234), vec![2, 3, 1, 0]);
    }

    #[test]
    fn sort_paths_by_name_uses_file_stem() {
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
    fn sort_paths_by_name_supports_descending_order() {
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
    fn sort_paths_by_size_uses_filesystem_lengths() {
        let directory = TestDirectory::create("size");
        let small = directory.file_with_contents("small.mp4", b"1");
        let large = directory.file_with_contents("large.mp4", b"12345");
        let medium = directory.file_with_contents("medium.mp4", b"123");
        let mut paths = vec![large.clone(), small.clone(), medium.clone()];

        sort_paths(&mut paths, "size", false);
        assert_eq!(paths, vec![small.clone(), medium.clone(), large.clone()]);

        sort_paths(&mut paths, "size", true);
        assert_eq!(paths, vec![large, medium, small]);
    }

    // Setting filesystem mtimes calls futimens on Linux, which Miri cannot execute.
    // Normal tests still cover the real metadata path; Miri covers ordering logic above.
    #[test]
    #[cfg_attr(miri, ignore)]
    fn date_sort_uses_modified_time() {
        let directory = TestDirectory::create("date");
        let older = directory.file_with_modified_time("older.mp4", b"older", 10);
        let newer = directory.file_with_modified_time("newer.mp4", b"newer", 30);
        let middle = directory.file_with_modified_time("middle.mp4", b"middle", 20);
        let mut paths = vec![newer.clone(), older.clone(), middle.clone()];

        sort_paths(&mut paths, "date", false);
        assert_eq!(paths, vec![older.clone(), middle.clone(), newer.clone()]);

        sort_paths(&mut paths, "date", true);
        assert_eq!(paths, vec![newer, middle, older]);
    }

    #[test]
    fn apply_permutation_reorders_in_place() {
        let mut playlist_entries = vec!["a".into(), "b".into(), "c".into()];
        apply_permutation(&mut playlist_entries, &[2, 1, 0]);
        assert_eq!(playlist_entries, vec!["c", "b", "a"]);
    }

    #[test]
    #[should_panic(expected = "permutation must provide one source index")]
    fn apply_permutation_rejects_missing_source_indices() {
        let mut playlist_entries = vec!["a".into(), "b".into(), "c".into()];
        apply_permutation(&mut playlist_entries, &[0, 1]);
    }
}
