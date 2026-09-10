//! Generic playback queue for ordered, shuffled, and repeated media lists.

use std::collections::HashSet;

/// Repeat behaviour for [`PlayQueue::advance`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    /// Stop after the last track.
    #[default]
    None,
    /// Replay the current track forever.
    One,
    /// Wrap to the first track after the last one.
    All,
}

/// Item identity used by [`PlayQueue::remove_duplicates`].
pub trait QueueEntry {
    /// Return the comparison key for duplicate detection.
    fn queue_key(&self) -> &str;
}

/// Music-library play queue with ordering, shuffle, and repeat.
///
/// Generic over any `T: QueueEntry`, so apps reuse it for tracks, video
/// items, or anything else that has a stable identifying key. Internal
/// shuffle uses an xorshift PRNG seeded from the system clock; not
/// suitable for cryptographic use.
///
/// # Examples
///
/// ```
/// use big_media_engines::queue::{PlayQueue, QueueEntry, RepeatMode};
///
/// #[derive(Clone)]
/// struct Track { path: String }
/// impl QueueEntry for Track {
///     fn queue_key(&self) -> &str { &self.path }
/// }
///
/// let mut q = PlayQueue::new();
/// q.set_tracks(vec![
///     Track { path: "/a.flac".into() },
///     Track { path: "/b.flac".into() },
/// ]);
/// q.set_repeat(RepeatMode::All);
///
/// assert_eq!(q.advance().unwrap().path, "/b.flac");
/// assert_eq!(q.advance().unwrap().path, "/a.flac");
/// ```
#[derive(Debug)]
pub struct PlayQueue<T> {
    tracks: Vec<T>,
    current_index: Option<usize>,
    shuffle: bool,
    repeat: RepeatMode,
    shuffle_order: Vec<usize>,
    shuffle_pos: usize,
}

impl<T> Default for PlayQueue<T> {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            current_index: None,
            shuffle: false,
            repeat: RepeatMode::None,
            shuffle_order: Vec::new(),
            shuffle_pos: 0,
        }
    }
}

impl<T: QueueEntry> PlayQueue<T> {
    /// Build an empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace all tracks; current position resets to `0` (or `None` if empty).
    pub fn set_tracks(&mut self, tracks: Vec<T>) {
        log::debug!("[QUEUE] set_tracks: {} tracks", tracks.len());
        self.tracks = tracks;
        self.current_index = if self.tracks.is_empty() {
            None
        } else {
            Some(0)
        };
        if self.shuffle {
            self.regenerate_shuffle();
        }
    }

    /// Append `track` to the end of the queue.
    pub fn add_track(&mut self, track: T) {
        self.tracks.push(track);
        if self.current_index.is_none() {
            self.current_index = Some(0);
        }
        if self.shuffle {
            self.shuffle_order.push(self.tracks.len() - 1);
        }
    }

    /// Insert `track` at `index` (clamped to `len`), shifting the current track if needed.
    pub fn insert_at(&mut self, index: usize, track: T) {
        let pos = index.min(self.tracks.len());
        self.tracks.insert(pos, track);
        if self.current_index.is_none() {
            self.current_index = Some(0);
        } else if let Some(cur) = self.current_index
            && pos <= cur
        {
            self.current_index = Some(cur + 1);
        }
        if self.shuffle {
            self.regenerate_shuffle();
        }
    }

    /// Insert `track` immediately after the current track ("play next").
    pub fn add_next(&mut self, track: T) {
        let insert_pos = self.current_index.map_or(0, |i| i + 1);
        self.tracks.insert(insert_pos, track);
        if self.current_index.is_none() {
            self.current_index = Some(0);
        }
        if self.shuffle {
            self.regenerate_shuffle();
        }
    }

    /// Remove every track and reset the current index.
    pub fn clear(&mut self) {
        self.tracks.clear();
        self.current_index = None;
        self.shuffle_order.clear();
        self.shuffle_pos = 0;
    }

    /// Borrow the currently-playing track, if any.
    #[must_use]
    pub fn current(&self) -> Option<&T> {
        self.current_index.and_then(|i| self.tracks.get(i))
    }

    /// Borrow the underlying track list.
    #[must_use]
    pub fn tracks(&self) -> &[T] {
        &self.tracks
    }

    /// Borrow the underlying track list mutably (callers must keep ordering invariants).
    pub fn tracks_mut(&mut self) -> &mut Vec<T> {
        &mut self.tracks
    }

    /// Index of the currently-playing track, if any.
    #[must_use]
    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    /// Number of tracks in the queue.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// `true` if the queue has no tracks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// Move to the next track honouring [`RepeatMode`] and shuffle settings.
    pub fn advance(&mut self) -> Option<&T> {
        let old_idx = self.current_index;
        if self.tracks.is_empty() {
            log::debug!("[QUEUE] advance: tracks empty");
            return None;
        }

        match self.repeat {
            RepeatMode::One => self.current(),
            _ if self.shuffle => self.advance_shuffled(old_idx),
            _ => self.advance_ordered(old_idx),
        }
    }

    /// Move to the previous track, wrapping when off the start.
    pub fn previous(&mut self) -> Option<&T> {
        let old_idx = self.current_index;
        if self.tracks.is_empty() {
            return None;
        }
        if self.shuffle {
            if self.shuffle_pos > 0 {
                self.shuffle_pos -= 1;
                self.current_index = Some(self.shuffle_order[self.shuffle_pos]);
            }
        } else {
            let prev = self
                .current_index
                .map_or(0, |i| if i > 0 { i - 1 } else { self.tracks.len() - 1 });
            self.current_index = Some(prev);
        }
        log::debug!(
            "[QUEUE] previous: {old_idx:?} -> {:?} (total={})",
            self.current_index,
            self.tracks.len()
        );
        self.current()
    }

    /// Set the current track to `index`, or return [`None`] if out of range.
    pub fn jump_to(&mut self, index: usize) -> Option<&T> {
        let old_idx = self.current_index;
        if index < self.tracks.len() {
            self.current_index = Some(index);
            log::debug!(
                "[QUEUE] jump_to({index}): {old_idx:?} -> {:?} (total={})",
                self.current_index,
                self.tracks.len()
            );
            self.current()
        } else {
            log::warn!(
                "[QUEUE] jump_to({index}): out of range (total={})",
                self.tracks.len()
            );
            None
        }
    }

    /// Enable or disable shuffle; enabling regenerates the shuffle order with the current track pinned.
    pub fn set_shuffle(&mut self, enabled: bool) {
        self.shuffle = enabled;
        if enabled {
            self.regenerate_shuffle();
        }
    }

    /// `true` if shuffle mode is enabled.
    #[must_use]
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Set the [`RepeatMode`] used by [`advance`](Self::advance).
    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    /// Current [`RepeatMode`].
    #[must_use]
    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    /// Remove the track at `index`, adjusting `current_index` so playback continues sanely.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.tracks.len() {
            return None;
        }
        let track = self.tracks.remove(index);
        if self.tracks.is_empty() {
            self.current_index = None;
        } else if let Some(cur) = self.current_index {
            if index < cur {
                self.current_index = Some(cur - 1);
            } else if index == cur && cur >= self.tracks.len() {
                self.current_index = Some(self.tracks.len() - 1);
            }
        }
        if self.shuffle {
            self.regenerate_shuffle();
        }
        Some(track)
    }

    /// Adjusts `current_index` so playback follows the moved track.
    pub fn move_track(&mut self, from: usize, to: usize) {
        if from >= self.tracks.len() || to >= self.tracks.len() || from == to {
            return;
        }
        let track = self.tracks.remove(from);
        self.tracks.insert(to, track);
        if let Some(cur) = self.current_index {
            if cur == from {
                self.current_index = Some(to);
            } else if from < cur && to >= cur {
                self.current_index = Some(cur - 1);
            } else if from > cur && to <= cur {
                self.current_index = Some(cur + 1);
            }
        }
        if self.shuffle {
            self.regenerate_shuffle();
        }
    }

    /// Identity = `QueueEntry::queue_key`. First occurrence wins.
    pub fn remove_duplicates(&mut self) -> usize {
        let current_key = self.current().map(QueueEntry::queue_key).map(str::to_owned);
        let mut seen = HashSet::new();
        let mut kept = Vec::with_capacity(self.tracks.len());
        let mut removed = 0_usize;

        for track in self.tracks.drain(..) {
            if seen.insert(track.queue_key().to_owned()) {
                kept.push(track);
            } else {
                removed += 1;
            }
        }

        self.tracks = kept;
        self.current_index = current_key.as_deref().and_then(|key| {
            self.tracks
                .iter()
                .position(|track| track.queue_key() == key)
        });

        if self.shuffle {
            self.regenerate_shuffle();
        }

        removed
    }

    fn advance_shuffled(&mut self, old_idx: Option<usize>) -> Option<&T> {
        self.shuffle_pos += 1;
        if self.shuffle_pos >= self.shuffle_order.len() {
            if self.repeat == RepeatMode::All {
                self.regenerate_shuffle();
            } else {
                return None;
            }
        }
        self.current_index = Some(self.shuffle_order[self.shuffle_pos]);
        log::debug!(
            "[QUEUE] advance(shuffle): {old_idx:?} -> {:?} (total={})",
            self.current_index,
            self.tracks.len()
        );
        self.current()
    }

    fn advance_ordered(&mut self, old_idx: Option<usize>) -> Option<&T> {
        let next = self.current_index.map_or(0, |i| i + 1);
        if next >= self.tracks.len() {
            if self.repeat == RepeatMode::All {
                self.current_index = Some(0);
            } else {
                return None;
            }
        } else {
            self.current_index = Some(next);
        }
        log::debug!(
            "[QUEUE] advance: {old_idx:?} -> {:?} (total={})",
            self.current_index,
            self.tracks.len()
        );
        self.current()
    }

    fn regenerate_shuffle(&mut self) {
        let len = self.tracks.len();
        self.shuffle_order = (0..len).collect();

        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(42, |d| d.as_nanos());
        let mut rng = seed as u64;

        for i in (1..len).rev() {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let j = (rng as usize) % (i + 1);
            self.shuffle_order.swap(i, j);
        }

        self.shuffle_pos = 0;
        if let Some(cur) = self.current_index
            && let Some(pos) = self.shuffle_order.iter().position(|&x| x == cur)
        {
            self.shuffle_order.swap(0, pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestTrack {
        id: usize,
        path: String,
    }

    impl QueueEntry for TestTrack {
        fn queue_key(&self) -> &str {
            &self.path
        }
    }

    fn tracks(count: usize) -> Vec<TestTrack> {
        (0..count)
            .map(|id| TestTrack {
                id,
                path: format!("/media/{id}.flac"),
            })
            .collect()
    }

    #[test]
    fn ordered_advance_stops_without_repeat() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(2));

        assert_eq!(queue.current().unwrap().id, 0);
        assert_eq!(queue.advance().unwrap().id, 1);
        assert!(queue.advance().is_none());
        assert_eq!(queue.current().unwrap().id, 1);
    }

    #[test]
    fn repeat_all_wraps_to_first_track() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(2));
        queue.set_repeat(RepeatMode::All);

        assert_eq!(queue.advance().unwrap().id, 1);
        assert_eq!(queue.advance().unwrap().id, 0);
    }

    #[test]
    fn previous_wraps_to_last_track() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(3));

        assert_eq!(queue.previous().unwrap().id, 2);
    }

    #[test]
    fn insert_before_current_keeps_same_current_track() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(3));
        queue.jump_to(2);
        queue.insert_at(
            1,
            TestTrack {
                id: 99,
                path: "/media/99.flac".to_string(),
            },
        );

        assert_eq!(queue.current().unwrap().id, 2);
        assert_eq!(queue.current_index(), Some(3));
    }

    #[test]
    fn move_track_follows_current_track() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(4));
        queue.jump_to(2);
        queue.move_track(2, 0);

        assert_eq!(queue.current().unwrap().id, 2);
        assert_eq!(queue.current_index(), Some(0));
    }

    #[test]
    fn remove_current_moves_to_previous_valid_slot() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(3));
        queue.jump_to(2);

        assert_eq!(queue.remove(2).unwrap().id, 2);
        assert_eq!(queue.current().unwrap().id, 1);
    }

    #[test]
    fn remove_duplicates_keeps_first_and_reanchors_current() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(vec![
            TestTrack {
                id: 1,
                path: "/same.flac".to_string(),
            },
            TestTrack {
                id: 2,
                path: "/unique.flac".to_string(),
            },
            TestTrack {
                id: 3,
                path: "/same.flac".to_string(),
            },
        ]);
        queue.jump_to(1);

        assert_eq!(queue.remove_duplicates(), 1);
        assert_eq!(queue.tracks().len(), 2);
        assert_eq!(queue.current().unwrap().id, 2);
    }

    #[test]
    fn shuffle_keeps_current_track_pinned() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(5));
        queue.jump_to(3);
        queue.set_shuffle(true);

        assert_eq!(queue.current().unwrap().id, 3);
        assert!(queue.shuffle());
    }

    #[test]
    fn repeat_one_stays_on_current_track() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(3));
        queue.jump_to(1);
        queue.set_repeat(RepeatMode::One);
        assert_eq!(queue.advance().unwrap().id, 1);
        assert_eq!(queue.current_index(), Some(1));
    }

    #[test]
    fn jump_to_out_of_range_returns_none_and_keeps_current() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(3));
        queue.jump_to(1);
        assert!(queue.jump_to(99).is_none());
        assert_eq!(queue.current().unwrap().id, 1);
        assert_eq!(queue.current_index(), Some(1));
    }

    #[test]
    fn remove_before_current_shifts_index_down() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(4));
        queue.jump_to(2);
        assert_eq!(queue.remove(0).unwrap().id, 0);
        // Same track stays current (id 2); its index shifts 2 -> 1.
        assert_eq!(queue.current().unwrap().id, 2);
        assert_eq!(queue.current_index(), Some(1));
    }

    #[test]
    fn remove_after_current_keeps_index() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(4));
        queue.jump_to(1);
        queue.remove(3);
        assert_eq!(queue.current().unwrap().id, 1);
        assert_eq!(queue.current_index(), Some(1));
    }

    #[test]
    fn remove_current_at_end_clamps_to_new_last() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(3));
        queue.jump_to(2);
        queue.remove(2); // removing the last + current track clamps to new last.
        assert_eq!(queue.current_index(), Some(1));
        assert_eq!(queue.current().unwrap().id, 1);
    }

    #[test]
    fn remove_last_remaining_track_clears_current() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(1));
        queue.jump_to(0);
        queue.remove(0);
        assert_eq!(queue.current_index(), None);
        assert!(queue.current().is_none());
    }

    #[test]
    fn move_current_track_follows_to_new_position() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(4));
        queue.jump_to(1);
        queue.move_track(1, 3); // from == current -> index follows to `to`.
        assert_eq!(queue.current_index(), Some(3));
        assert_eq!(queue.current().unwrap().id, 1);
    }

    #[test]
    fn move_track_from_before_to_after_current_decrements_index() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(4));
        queue.jump_to(2);
        queue.move_track(0, 3); // from < cur && to >= cur -> cur - 1.
        assert_eq!(queue.current_index(), Some(1));
        assert_eq!(queue.current().unwrap().id, 2);
    }

    #[test]
    fn move_track_from_after_to_before_current_increments_index() {
        let mut queue = PlayQueue::new();
        queue.set_tracks(tracks(4));
        queue.jump_to(1);
        queue.move_track(3, 0); // from > cur && to <= cur -> cur + 1.
        assert_eq!(queue.current_index(), Some(2));
        assert_eq!(queue.current().unwrap().id, 1);
    }
}
