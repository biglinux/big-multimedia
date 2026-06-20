//! Pure (GTK-free) queue undo/redo for the Relm4 `AudioShell`.
//!
//! Snapshot-based: each user queue edit pushes the prior track list onto the
//! undo stack (bounded by `MAX_UNDO`, oldest evicted), and a new edit clears the
//! redo stack. Undo/redo swap the current list with the neighbouring snapshot.
//! Snapshotting the full list (rather than inverting per-op) is lossless for
//! order/identity and keeps the shell wiring to a single `snapshot` call per
//! edit. Bounded queues make the memory cost negligible.

use std::collections::VecDeque;

use crate::db::models::Track;

/// Maximum retained undo/redo steps (oldest evicted on overflow).
const MAX_UNDO: usize = 30;

/// Undo + redo snapshot stacks for the play queue.
#[derive(Default)]
pub(crate) struct QueueHistory {
    undo: VecDeque<Vec<Track>>,
    redo: VecDeque<Vec<Track>>,
}

impl QueueHistory {
    /// Record the pre-edit track list; clears the redo history. Call before any
    /// user-initiated queue mutation.
    pub(crate) fn snapshot(&mut self, current: Vec<Track>) {
        if self.undo.len() >= MAX_UNDO {
            self.undo.pop_front();
        }
        self.undo.push_back(current);
        self.redo.clear();
    }

    /// Pop the last snapshot, stashing `current` onto the redo stack. Returns the
    /// track list to restore, or `None` when there is nothing to undo.
    pub(crate) fn undo(&mut self, current: Vec<Track>) -> Option<Vec<Track>> {
        let previous = self.undo.pop_back()?;
        self.redo.push_back(current);
        Some(previous)
    }

    /// Re-apply the last undone edit, stashing `current` back onto the undo
    /// stack. Returns the track list to restore, or `None` when redo is empty.
    pub(crate) fn redo(&mut self, current: Vec<Track>) -> Option<Vec<Track>> {
        let next = self.redo.pop_back()?;
        self.undo.push_back(current);
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: i64) -> Track {
        Track::new_podcast_episode(id, "u", "t", "p", 1)
    }

    /// `Track` has no `PartialEq`; compare restored lists by id.
    fn ids(tracks: &Option<Vec<Track>>) -> Option<Vec<i64>> {
        tracks.as_ref().map(|ts| ts.iter().map(|t| t.id).collect())
    }

    #[test]
    fn undo_then_redo_round_trips() {
        let mut h = QueueHistory::default();
        let v0 = vec![track(1)];
        let v1 = vec![track(1), track(2)];
        h.snapshot(v0.clone()); // editing v0 → v1
        assert_eq!(ids(&h.undo(v1.clone())), Some(vec![1]));
        assert_eq!(ids(&h.redo(v0.clone())), Some(vec![1, 2]));
        assert_eq!(ids(&h.undo(v0)), Some(vec![1]));
    }

    #[test]
    fn nothing_to_undo_or_redo() {
        let mut h = QueueHistory::default();
        assert!(h.undo(Vec::new()).is_none());
        assert!(h.redo(Vec::new()).is_none());
    }

    #[test]
    fn new_edit_clears_redo() {
        let mut h = QueueHistory::default();
        h.snapshot(vec![track(1)]);
        let _ = h.undo(vec![track(1), track(2)]);
        h.snapshot(vec![track(1)]); // a fresh edit invalidates redo
        assert!(h.redo(vec![track(1)]).is_none());
    }

    #[test]
    fn undo_stack_is_bounded() {
        let mut h = QueueHistory::default();
        for i in 0..(MAX_UNDO + 5) {
            h.snapshot(vec![track(i as i64)]);
        }
        assert_eq!(h.undo.len(), MAX_UNDO);
    }
}
