//! Pure trim-mode state: the set of clip segments the user has marked plus the
//! pending in-point and the chosen cut mode. No GTK — unit-tested in isolation;
//! [`super::bar`] renders it and [`super::run`] exports it.

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
/// the UI can address a chip for deletion without index drift.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrimSegment {
    /// Stable identifier for UI addressing.
    pub id: u64,
    /// Range start (seconds, ≥ 0).
    pub start: f64,
    /// Range end (seconds, > start).
    pub end: f64,
}

/// Marked segments + the in-progress in-point + cut mode. Pure logic.
#[derive(Debug, Default)]
pub struct TrimState {
    /// Total media duration (seconds); marks clamp to `[0, duration]`.
    pub duration: f64,
    /// Live playhead used for the timeline cursor (seconds).
    pub playhead: f64,
    /// A set in-point awaiting an out-point (the half-open segment being marked).
    pub pending_in: Option<f64>,
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

    /// Mark the in-point at `t` (replaces any prior pending in-point).
    pub fn set_in(&mut self, t: f64) {
        self.pending_in = Some(self.clamp(t));
    }

    /// Mark the out-point at `t`, committing `[pending_in, t]` as a segment.
    /// Returns `false` (no commit) when no in-point is pending or `t` is not
    /// strictly after it.
    pub fn set_out(&mut self, t: f64) -> bool {
        let end = self.clamp(t);
        let Some(start) = self.pending_in else {
            return false;
        };
        if end <= start {
            return false;
        }
        self.push_segment(start, end);
        self.pending_in = None;
        true
    }

    /// `M`: when no in-point is pending, set one; otherwise close the segment at
    /// `t`. Returns true when a segment was committed.
    pub fn quick_mark(&mut self, t: f64) -> bool {
        if self.pending_in.is_some() {
            self.set_out(t)
        } else {
            self.set_in(t);
            false
        }
    }

    fn push_segment(&mut self, start: f64, end: f64) {
        let id = self.next_id;
        self.next_id += 1;
        self.segments.push(TrimSegment { id, start, end });
    }

    /// Remove the segment with `id`. Returns true when one was removed.
    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.segments.len();
        self.segments.retain(|s| s.id != id);
        self.segments.len() != before
    }

    /// Drop every segment and any pending in-point.
    pub fn clear(&mut self) {
        self.segments.clear();
        self.pending_in = None;
    }

    /// At least one committed segment exists → export is allowed.
    #[must_use]
    pub fn is_exportable(&self) -> bool {
        !self.segments.is_empty()
    }

    /// Export ranges as `(start, end)` pairs, sorted by start and with
    /// overlapping/touching ranges merged so the output never replays a span.
    #[must_use]
    pub fn export_ranges(&self) -> Vec<(f64, f64)> {
        let mut ranges: Vec<(f64, f64)> = self.segments.iter().map(|s| (s.start, s.end)).collect();
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

    /// Commit a segment via the in→out path (the only way segments are made).
    fn mark(s: &mut TrimState, start: f64, end: f64) {
        s.set_in(start);
        assert!(s.set_out(end), "[{start}, {end}] should commit");
    }

    #[test]
    fn set_in_then_out_commits_one_segment() {
        let mut s = state(100.0);
        s.set_in(10.0);
        assert!(s.set_out(25.0));
        assert_eq!(s.segments.len(), 1);
        assert_eq!(s.segments[0].start, 10.0);
        assert_eq!(s.segments[0].end, 25.0);
        assert!(s.pending_in.is_none());
    }

    #[test]
    fn set_out_without_in_is_noop() {
        let mut s = state(100.0);
        assert!(!s.set_out(25.0));
        assert!(s.segments.is_empty());
    }

    #[test]
    fn set_out_before_in_rejected() {
        let mut s = state(100.0);
        s.set_in(30.0);
        assert!(!s.set_out(20.0));
        // pending in-point survives a rejected out so the user can retry.
        assert_eq!(s.pending_in, Some(30.0));
        assert!(s.segments.is_empty());
    }

    #[test]
    fn quick_mark_toggles_in_then_out() {
        let mut s = state(100.0);
        assert!(!s.quick_mark(5.0)); // sets in
        assert_eq!(s.pending_in, Some(5.0));
        assert!(s.quick_mark(15.0)); // closes segment
        assert_eq!(s.segments.len(), 1);
        assert!(s.pending_in.is_none());
    }

    #[test]
    fn marks_clamp_to_duration() {
        let mut s = state(40.0);
        s.set_in(-5.0);
        assert_eq!(s.pending_in, Some(0.0));
        assert!(s.set_out(999.0));
        assert_eq!(s.segments[0].end, 40.0);
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
        let ranges = s.export_ranges();
        assert_eq!(ranges, vec![(2.0, 8.0), (20.0, 35.0)]);
    }

    #[test]
    fn not_exportable_until_a_segment_exists() {
        let mut s = state(100.0);
        assert!(!s.is_exportable());
        s.set_in(1.0);
        assert!(!s.is_exportable()); // pending in-point alone is not a segment
        s.set_out(2.0);
        assert!(s.is_exportable());
    }
}
