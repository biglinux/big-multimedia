//! Generation tickets are independent of GTK and can cross the query pool.
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};

#[derive(Default)]
pub(super) struct LibraryGeneration(Arc<AtomicU64>);

impl LibraryGeneration {
    pub(super) fn begin(&self) -> LibraryTicket {
        let value = self.0.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        LibraryTicket { current: Arc::clone(&self.0), value }
    }

    pub(super) fn is_current(&self, value: u64) -> bool {
        self.0.load(Ordering::Acquire) == value
    }
}

pub(super) struct LibraryTicket {
    current: Arc<AtomicU64>,
    pub(super) value: u64,
}

impl LibraryTicket {
    pub(super) fn is_current(&self) -> bool {
        self.current.load(Ordering::Acquire) == self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_filter_invalidates_queued_and_completed_results() {
        let generation = LibraryGeneration::default();
        let old = generation.begin();
        assert!(old.is_current());
        let cleared = generation.begin();
        assert!(!old.is_current());
        assert!(!generation.is_current(old.value));
        assert!(generation.is_current(cleared.value));
    }

    #[test]
    fn tickets_can_be_checked_on_query_workers() {
        let generation = LibraryGeneration::default();
        let old = generation.begin();
        let _latest = generation.begin();
        assert!(!std::thread::spawn(move || old.is_current()).join().unwrap());
    }
}
