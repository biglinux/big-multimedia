//! Pure trim-mode state: the marked clip segments + the pending first mark + the
//! chosen cut mode. No GTK — unit-tested in isolation; [`super::bar`] renders it
//! and [`super::run`] exports it.

/// How a clip export turns the marked segments into a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CutMode {
    /// Stream-copy into the source container: near-instant, keyframe-aligned cut,
    /// no re-encode, no filters. The default — "Manter formato (rápido)".
    #[default]
    Fast,
    /// Re-encode: frame-accurate, honours the chosen codec/quality + filters.
    Precise,
}

/// One marked clip range, in seconds. `id` is stable for the row's lifetime so
/// the UI can address a chip for editing/deletion without index drift.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrimSegment {
    /// Stable identifier for UI addressing.
    pub id: u64,
    /// Range start (seconds, ≥ 0).
    pub start: f64,
    /// Range end (seconds, > start).
    pub end: f64,
}

/// Marked segments + the pending first mark + cut mode. Pure logic.
#[derive(Debug, Default)]
pub struct TrimState {
    /// Total media duration (seconds); marks clamp to `[0, duration]`.
    pub duration: f64,
    /// Live playhead (seconds).
    pub playhead: f64,
    /// The first mark of a segment, awaiting its second. The user marks two
    /// points in any order; the earlier becomes the start, the later the end.
    pub pending: Option<f64>,
    /// Committed segments, in mark order.
    pub segments: Vec<TrimSegment>,
    /// Chosen cut mode.
    pub mode: CutMode,
    /// True while an export is running (controls are disabled).
    pub busy: bool,
    next_id: u64,
}

impl TrimState {
    /// Clamp a raw time to the valid `[0, duration]` window.
    fn clamp(&self, t: f64) -> f64 {
        if self.duration > 0.0 {
            t.clamp(0.0, self.duration)
        } else {
            t.max(0.0)
        }
    }

    /// Mark a point. The first mark is held pending; the second commits a segment
    /// spanning the two marks — **in either order** (mark the end first then the
    /// start and the segment is still `[min, max]`). Returns true when a segment
    /// was committed. A zero-length pair (two marks at the same time) is ignored
    /// and keeps the pending mark.
    pub fn mark(&mut self, t: f64) -> bool {
        let t = self.clamp(t);
        match self.pending {
            None => {
                self.pending = Some(t);
                false
            }
            Some(first) => {
                let (start, end) = if t >= first { (first, t) } else { (t, first) };
                if end <= start {
                    return false;
                }
                self.push_segment(start, end);
                self.pending = None;
                true
            }
        }
    }

    fn push_segment(&mut self, start: f64, end: f64) {
        let id = self.next_id;
        self.next_id += 1;
        self.segments.push(TrimSegment { id, start, end });
    }

    /// Replace the times of the segment `id` (the manual-edit form). The pair is
    /// clamped to `[0, duration]` and ordered; returns false when `id` is unknown
    /// or the range is empty.
    pub fn set_segment_times(&mut self, id: u64, a: f64, b: f64) -> bool {
        let lo = self.clamp(a.min(b));
        let hi = self.clamp(a.max(b));
        if hi <= lo {
            return false;
        }
        if let Some(seg) = self.segments.iter_mut().find(|s| s.id == id) {
            seg.start = lo;
            seg.end = hi;
            true
        } else {
            false
        }
    }

    /// Remove the segment with `id`. Returns true when one was removed.
    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.segments.len();
        self.segments.retain(|s| s.id != id);
        self.segments.len() != before
    }

    /// Drop every segment and any pending mark.
    pub fn clear(&mut self) {
        self.segments.clear();
        self.pending = None;
    }

    /// At least one committed segment exists → export is allowed.
    #[must_use]
    pub fn is_exportable(&self) -> bool {
        !self.segments.is_empty()
    }

    /// Raw per-segment `(start, end)` ranges in mark order — for display on the
    /// seek bar (NOT merged; each marked cut shows as itself).
    #[must_use]
    pub fn segment_ranges(&self) -> Vec<(f64, f64)> {
        self.segments.iter().map(|s| (s.start, s.end)).collect()
    }

    /// Export ranges as `(start, end)` pairs, sorted by start and with
    /// overlapping/touching ranges merged so the output never replays a span.
    #[must_use]
    pub fn export_ranges(&self) -> Vec<(f64, f64)> {
        let mut ranges = self.segment_ranges();
        ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            match merged.last_mut() {
                Some(last) if start <= last.1 => last.1 = last.1.max(end),
                _ => merged.push((start, end)),
            }
        }
        merged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(duration: f64) -> TrimState {
        TrimState {
            duration,
            ..TrimState::default()
        }
    }

    fn mark(s: &mut TrimState, a: f64, b: f64) {
        assert!(!s.mark(a), "first mark is pending");
        assert!(s.mark(b), "second mark commits");
    }

    #[test]
    fn two_marks_commit_one_segment() {
        let mut s = state(100.0);
        assert!(!s.mark(10.0));
        assert_eq!(s.pending, Some(10.0));
        assert!(s.mark(25.0));
        assert_eq!(s.segments.len(), 1);
        assert_eq!((s.segments[0].start, s.segments[0].end), (10.0, 25.0));
        assert!(s.pending.is_none());
    }

    #[test]
    fn marks_auto_order_when_end_marked_first() {
        let mut s = state(100.0);
        s.mark(40.0); // user marks the END first
        s.mark(15.0); // then the START
        assert_eq!((s.segments[0].start, s.segments[0].end), (15.0, 40.0));
    }

    #[test]
    fn duplicate_mark_keeps_pending() {
        let mut s = state(100.0);
        s.mark(20.0);
        assert!(!s.mark(20.0)); // zero-length → ignored
        assert_eq!(s.pending, Some(20.0));
        assert!(s.segments.is_empty());
    }

    #[test]
    fn marks_clamp_to_duration() {
        let mut s = state(40.0);
        s.mark(-5.0);
        assert_eq!(s.pending, Some(0.0));
        s.mark(999.0);
        assert_eq!((s.segments[0].start, s.segments[0].end), (0.0, 40.0));
    }

    #[test]
    fn set_segment_times_clamps_and_orders() {
        let mut s = state(100.0);
        mark(&mut s, 10.0, 20.0);
        let id = s.segments[0].id;
        // edit with reversed + out-of-range values → clamped + ordered.
        assert!(s.set_segment_times(id, 55.5, 33.25));
        assert_eq!((s.segments[0].start, s.segments[0].end), (33.25, 55.5));
        assert!(!s.set_segment_times(id, 9.0, 9.0)); // empty rejected
        assert!(!s.set_segment_times(999, 1.0, 2.0)); // unknown id
    }

    #[test]
    fn delete_removes_by_id_without_index_drift() {
        let mut s = state(100.0);
        mark(&mut s, 0.0, 10.0);
        mark(&mut s, 20.0, 30.0);
        let first_id = s.segments[0].id;
        assert!(s.delete(first_id));
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0].start, 20.0);
        assert!(!s.delete(first_id));
    }

    #[test]
    fn export_ranges_sorts_and_merges_overlaps() {
        let mut s = state(100.0);
        mark(&mut s, 20.0, 30.0);
        mark(&mut s, 2.0, 5.0);
        mark(&mut s, 4.0, 8.0); // overlaps the [2,5] range
        mark(&mut s, 30.0, 35.0); // touches [20,30] → merge
        assert_eq!(s.export_ranges(), vec![(2.0, 8.0), (20.0, 35.0)]);
        // raw display ranges stay unmerged, in mark order.
        assert_eq!(s.segment_ranges().len(), 4);
    }

    #[test]
    fn not_exportable_until_a_segment_exists() {
        let mut s = state(100.0);
        assert!(!s.is_exportable());
        s.mark(1.0);
        assert!(!s.is_exportable()); // pending mark alone is not a segment
        s.mark(2.0);
        assert!(s.is_exportable());
    }
}
