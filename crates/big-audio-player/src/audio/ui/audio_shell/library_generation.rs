//! Generation tickets are independent of GTK and can cross the query pool.
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
pub(super) struct LibraryGeneration(Arc<AtomicU64>);

impl LibraryGeneration {
    pub(super) fn begin(&self) -> LibraryTicket {
        let value = self.0.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        LibraryTicket {
            current: Arc::clone(&self.0),
            value,
        }
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

/// Only one task plus one newest pending request. Call complete even after panic.
pub(super) struct LatestMailbox<T>(Arc<std::sync::Mutex<(bool, Option<T>)>>);
impl<T> Clone for LatestMailbox<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Default for LatestMailbox<T> {
    fn default() -> Self {
        Self(Arc::new(std::sync::Mutex::new((false, None))))
    }
}
impl<T> LatestMailbox<T> {
    pub(super) fn submit(&self, request: T) -> Option<T> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.0 {
            state.1 = Some(request);
            None
        } else {
            state.0 = true;
            Some(request)
        }
    }
    pub(super) fn complete(&self) -> Option<T> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let next = state.1.take();
        if next.is_none() {
            state.0 = false;
        }
        next
    }
}
impl Drop for LibraryGeneration {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}
#[cfg(test)]
mod mailbox_tests {
    use super::*;
    #[test]
    fn never_accumulates_intermediate_searches_and_recovers_after_completion() {
        let mailbox = LatestMailbox::default();
        assert_eq!(mailbox.submit(0), Some(0));
        for i in 1..1000 {
            assert_eq!(mailbox.submit(i), None);
        }
        assert_eq!(mailbox.complete(), Some(999));
        assert_eq!(mailbox.complete(), None);
        assert_eq!(mailbox.submit(1001), Some(1001));
    }
}
