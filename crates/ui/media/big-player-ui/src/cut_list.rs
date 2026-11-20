//! The cuts both players mark in trim mode, and the stepping of "Play only
//! marked cuts" over them. No GTK; each trim bar renders a [`CutList`] and the
//! host shell seeks where [`play_only_cuts_seek`] says.

/// The shortest cut, in milliseconds: anything shorter would end within
/// the play-only-cuts tolerance of its start and never play.
pub const MIN_CUT_MILLISECONDS: f64 = 100.0;

/// How far from a cut's edge, in seconds, the playhead still counts as on it;
/// position ticks and seeks land a few milliseconds off.
const PLAY_ONLY_CUTS_TOLERANCE: f64 = 0.05;

/// One marked cut, in seconds. `id` stays the same while the cut exists, so a
/// chip or a timeline drag addresses it without index drift.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    /// Stable identifier for UI addressing.
    pub id: u64,
    /// Start, at least 0.
    pub start: f64,
    /// End, after `start`.
    pub end: f64,
}

/// Marked cuts plus the first mark of the next one. The cuts are kept sorted,
/// with overlapping or touching ones merged into the earlier cut.
#[derive(Debug, Default)]
pub struct CutList {
    duration: f64,
    pending: Option<f64>,
    cuts: Vec<Cut>,
    next_id: u64,
}

impl CutList {
    /// Set the media duration in seconds; 0 while it is unknown.
    pub fn set_duration(&mut self, duration: f64) {
        self.duration = duration;
    }

    /// The first mark of a cut, waiting for its second.
    #[must_use]
    pub fn pending(&self) -> Option<f64> {
        self.pending
    }

    /// The committed cuts, sorted and disjoint.
    #[must_use]
    pub fn cuts(&self) -> &[Cut] {
        &self.cuts
    }

    /// Mark a point. The first mark waits; the second commits a cut spanning
    /// both, in either order. Returns true when a cut was committed. A second
    /// mark too close to the first is ignored and the first keeps waiting.
    pub fn mark(&mut self, time: f64) -> bool {
        let time = self.clamp(time);
        let Some(first) = self.pending else {
            self.pending = Some(time);
            return false;
        };
        let Some((start, end)) = cut_span(first, time) else {
            return false;
        };
        let id = self.next_id;
        self.next_id += 1;
        self.cuts.push(Cut { id, start, end });
        self.pending = None;
        self.merge();
        true
    }

    /// Replace the times of cut `id`, in either order; a cut it then overlaps
    /// merges with it. Returns false when `id` is unknown or the cut would be
    /// shorter than [`MIN_CUT_MILLISECONDS`].
    pub fn set_cut_times(&mut self, id: u64, a: f64, b: f64) -> bool {
        let Some((start, end)) = cut_span(self.clamp(a), self.clamp(b)) else {
            return false;
        };
        let Some(cut) = self.cuts.iter_mut().find(|cut| cut.id == id) else {
            return false;
        };
        cut.start = start;
        cut.end = end;
        self.merge();
        true
    }

    /// Remove cut `id`. Returns true when one was removed.
    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.cuts.len();
        self.cuts.retain(|cut| cut.id != id);
        self.cuts.len() != before
    }

    /// Drop every cut and the pending mark.
    pub fn clear(&mut self) {
        self.cuts.clear();
        self.pending = None;
    }

    /// The cuts as `(start, end)` ranges, for export and [`play_only_cuts_seek`].
    #[must_use]
    pub fn ranges(&self) -> Vec<(f64, f64)> {
        self.cuts.iter().map(|cut| (cut.start, cut.end)).collect()
    }

    fn merge(&mut self) {
        self.cuts.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut merged: Vec<Cut> = Vec::with_capacity(self.cuts.len());
        for cut in self.cuts.drain(..) {
            match merged.last_mut() {
                Some(last) if cut.start <= last.end => last.end = last.end.max(cut.end),
                _ => merged.push(cut),
            }
        }
        self.cuts = merged;
    }

    fn clamp(&self, time: f64) -> f64 {
        if self.duration > 0.0 {
            time.clamp(0.0, self.duration)
        } else {
            time.max(0.0)
        }
    }
}

/// `a` and `b` as a cut's `(start, end)`, in either order, or `None` when they
/// are less than [`MIN_CUT_MILLISECONDS`] apart.
#[must_use]
pub fn cut_span(a: f64, b: f64) -> Option<(f64, f64)> {
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    // Whole milliseconds, so 10.0 to 10.1 typed in the dialog is long enough.
    let milliseconds = ((end - start) * 1000.0).round();
    (milliseconds >= MIN_CUT_MILLISECONDS).then_some((start, end))
}

/// Where "Play only marked cuts" seeks from `position`: the next cut's start at
/// the end of a cut or between cuts, the first cut's start after the last one,
/// and `None` while playing inside a cut. `ranges` are sorted and disjoint, as
/// [`CutList::ranges`] returns them.
#[must_use]
pub fn play_only_cuts_seek(ranges: &[(f64, f64)], position: f64) -> Option<f64> {
    let &(first_start, _) = ranges.first()?;
    // A cut that holds the position wins over a neighbor's tolerance.
    if ranges
        .iter()
        .any(|&(start, end)| position >= start && position < end - PLAY_ONLY_CUTS_TOLERANCE)
    {
        return None;
    }
    let within = ranges.iter().enumerate().find(|(_, (start, end))| {
        position >= start - PLAY_ONLY_CUTS_TOLERANCE && position <= end + PLAY_ONLY_CUTS_TOLERANCE
    });
    let next = match within {
        Some((_, &(_, end))) if position < end - PLAY_ONLY_CUTS_TOLERANCE => return None,
        Some((index, _)) => ranges.get(index + 1),
        None => ranges.iter().find(|&&(start, _)| start > position),
    };
    Some(next.map_or(first_start, |&(start, _)| start))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(duration: f64) -> CutList {
        let mut list = CutList::default();
        list.set_duration(duration);
        list
    }

    fn mark(list: &mut CutList, a: f64, b: f64) {
        assert!(!list.mark(a), "first mark waits");
        assert!(list.mark(b), "second mark commits");
    }

    fn spans(list: &CutList) -> Vec<(f64, f64)> {
        list.cuts().iter().map(|cut| (cut.start, cut.end)).collect()
    }

    #[test]
    fn two_marks_commit_one_cut() {
        let mut list = list(100.0);
        assert!(!list.mark(10.0));
        assert_eq!(list.pending(), Some(10.0));
        assert!(list.mark(25.0));
        assert_eq!(spans(&list), vec![(10.0, 25.0)]);
        assert_eq!(list.pending(), None);
    }

    #[test]
    fn end_marked_first_swaps() {
        let mut list = list(100.0);
        mark(&mut list, 40.0, 15.0);
        assert_eq!(spans(&list), vec![(15.0, 40.0)]);
    }

    #[test]
    fn duplicate_mark_keeps_pending() {
        let mut list = list(100.0);
        list.mark(20.0);
        assert!(!list.mark(20.0));
        assert_eq!(list.pending(), Some(20.0));
        assert!(list.cuts().is_empty());
    }

    #[test]
    fn short_cut_refused() {
        let mut list = list(100.0);
        list.mark(20.0);
        assert!(!list.mark(20.05));
        assert_eq!(list.pending(), Some(20.0));
        assert!(list.mark(20.1));
        assert_eq!(spans(&list), vec![(20.0, 20.1)]);
        let id = list.cuts()[0].id;
        assert!(!list.set_cut_times(id, 30.0, 30.09));
        assert!(list.set_cut_times(id, 10.1, 10.0));
        assert_eq!(spans(&list), vec![(10.0, 10.1)]);
        assert_eq!(cut_span(5.0, 5.0), None);
        assert_eq!(cut_span(f64::NAN, 5.0), None);
    }

    #[test]
    fn marks_clamp_to_duration() {
        let mut list = list(40.0);
        list.mark(-5.0);
        assert_eq!(list.pending(), Some(0.0));
        list.mark(999.0);
        assert_eq!(spans(&list), vec![(0.0, 40.0)]);
    }

    #[test]
    fn marks_before_duration_keep_time() {
        let mut list = CutList::default();
        list.mark(-3.0);
        assert_eq!(list.pending(), Some(0.0));
        assert!(list.mark(125.5));
        assert_eq!(spans(&list), vec![(0.0, 125.5)]);
    }

    #[test]
    fn set_cut_times_clamps_and_swaps() {
        let mut list = list(100.0);
        mark(&mut list, 10.0, 20.0);
        let id = list.cuts()[0].id;
        assert!(list.set_cut_times(id, 155.5, 33.25));
        assert_eq!(spans(&list), vec![(33.25, 100.0)]);
        assert!(list.set_cut_times(id, 40.0, -10.0));
        assert_eq!(spans(&list), vec![(0.0, 40.0)]);
        assert!(!list.set_cut_times(id, 9.0, 9.0));
        assert!(!list.set_cut_times(999, 1.0, 2.0));
    }

    #[test]
    fn delete_removes_by_id() {
        let mut list = list(100.0);
        mark(&mut list, 0.0, 10.0);
        mark(&mut list, 20.0, 30.0);
        let first = list.cuts()[0].id;
        assert!(list.delete(first));
        assert_eq!(spans(&list), vec![(20.0, 30.0)]);
        assert!(!list.delete(first));
    }

    #[test]
    fn clear_drops_cuts_and_pending() {
        let mut list = list(100.0);
        mark(&mut list, 5.0, 12.0);
        list.mark(20.0);
        list.clear();
        assert!(list.cuts().is_empty());
        assert_eq!(list.pending(), None);
    }

    #[test]
    fn cuts_sort_and_merge() {
        let mut list = list(100.0);
        mark(&mut list, 20.0, 30.0);
        mark(&mut list, 2.0, 5.0);
        mark(&mut list, 4.0, 8.0);
        mark(&mut list, 30.0, 35.0);
        mark(&mut list, 50.0, 60.0);
        mark(&mut list, 52.0, 55.0);
        assert_eq!(spans(&list), vec![(2.0, 8.0), (20.0, 35.0), (50.0, 60.0)]);
    }

    #[test]
    fn edited_cut_merges_with_overlapped() {
        let mut list = list(100.0);
        mark(&mut list, 10.0, 20.0);
        mark(&mut list, 40.0, 50.0);
        let later = list.cuts()[1].id;
        assert!(list.set_cut_times(later, 15.0, 45.0));
        assert_eq!(spans(&list), vec![(10.0, 45.0)]);
    }

    #[test]
    fn play_only_cuts_steps_through_ranges() {
        let ranges = [(10.0, 20.0), (40.0, 50.0)];
        // (position, expected seek); targets from the cut times above.
        let table = [
            (0.0, Some(10.0)),
            (15.0, None),
            (20.0, Some(40.0)),
            (30.0, Some(40.0)),
            (45.0, None),
            (50.0, Some(10.0)),
            (60.0, Some(10.0)),
        ];
        for (position, expected) in table {
            assert_eq!(
                play_only_cuts_seek(&ranges, position),
                expected,
                "at {position}"
            );
        }
        assert_eq!(play_only_cuts_seek(&[], 5.0), None);
    }

    /// Playback just inside a cut that starts within the tolerance of the
    /// previous one's end stays in it.
    #[test]
    fn close_next_cut_plays_through() {
        let ranges = [(10.0, 20.0), (20.01, 30.0)];
        assert_eq!(play_only_cuts_seek(&ranges, 20.01), None);
        assert_eq!(play_only_cuts_seek(&ranges, 20.0), Some(20.01));
    }

    /// The shell steps over the cuts the list hands it, so a cut inside
    /// another must not send playback back into the outer one.
    #[test]
    fn nested_cut_plays_through() {
        let mut list = list(60.0);
        mark(&mut list, 10.0, 30.0);
        mark(&mut list, 20.0, 25.0);
        let ranges = list.ranges();
        assert_eq!(play_only_cuts_seek(&ranges, 25.0), None);
        assert_eq!(play_only_cuts_seek(&ranges, 30.0), Some(10.0));
    }
}
