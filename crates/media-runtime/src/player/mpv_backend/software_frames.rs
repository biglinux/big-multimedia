//! Immutable, pooled software frames. GTK-free; a GBytes owner returns a frame
//! only after the final consumer releases it. Rendering never mutates a lease.

use std::sync::{Arc, Mutex, Weak};

/// Safety ceiling per compatibility-mode frame (three slots, excluding mpv/GDK).
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
const SLOT_COUNT: usize = 3;

#[repr(C, align(64))]
#[derive(Clone)]
struct Block([u8; 64]);

#[derive(Default)]
struct Storage {
    blocks: Vec<Block>,
    len: usize,
}
impl Storage {
    fn resize(&mut self, len: usize) -> std::io::Result<()> {
        if len > MAX_FRAME_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "software frame exceeds memory limit",
            ));
        }
        let count = len.div_ceil(64);
        if self.blocks.capacity() > count.saturating_mul(2) {
            self.blocks = Vec::new(); // release oversized storage after a resize
        }
        if count > self.blocks.len() {
            self.blocks
                .try_reserve_exact(count - self.blocks.len())
                .map_err(std::io::Error::other)?;
        }
        self.blocks.resize(count, Block([0; 64]));
        self.len = len;
        Ok(())
    }
    fn bytes(&self) -> &[u8] {
        // repr(C, align(64)) Block has exactly 64 initialized bytes, no padding.
        // Vec owns contiguous blocks; len <= blocks.len()*64, and no mutation is
        // possible while a shared slice/GBytes lease exists.
        unsafe { std::slice::from_raw_parts(self.blocks.as_ptr().cast::<u8>(), self.len) }
    }
    fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.blocks.as_mut_ptr().cast::<u8>(), self.len) }
    }
}

struct PoolState {
    free: Vec<Storage>,
    created: usize,
}

/// Fixed maximum of three independent pixel buffers; no per-frame pixel copy.
pub struct SoftwareFramePool(Arc<Mutex<PoolState>>);
impl Default for SoftwareFramePool {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(PoolState {
            free: Vec::with_capacity(SLOT_COUNT),
            created: 0,
        })))
    }
}
impl SoftwareFramePool {
    /// `Ok(None)` applies backpressure; a later release wakes the UI. Allocation
    /// occurs only while growing/reconfiguring a free slot, never a busy one.
    pub fn acquire(
        &self,
        len: usize,
        wake: Arc<async_channel::Sender<()>>,
    ) -> std::io::Result<Option<SoftwareFrame>> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut storage = if let Some(storage) = state.free.pop() {
            storage
        } else if state.created < SLOT_COUNT {
            state.created += 1;
            Storage::default()
        } else {
            return Ok(None);
        };
        drop(state); // never allocate a frame while holding the release mutex
        if let Err(error) = storage.resize(len) {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .free
                .push(storage);
            return Err(error);
        }
        Ok(Some(SoftwareFrame {
            storage,
            pool: Arc::downgrade(&self.0),
            wake,
        }))
    }
}

/// Owned pixels for `glib::Bytes::from_owned`, with 64-byte pointer alignment.
pub struct SoftwareFrame {
    storage: Storage,
    pool: Weak<Mutex<PoolState>>,
    wake: Arc<async_channel::Sender<()>>,
}
impl SoftwareFrame {
    /// Exclusive access before handing the frame to the immutable consumer.
    pub fn pixels_mut(&mut self) -> &mut [u8] {
        self.storage.bytes_mut()
    }
}
impl AsRef<[u8]> for SoftwareFrame {
    fn as_ref(&self) -> &[u8] {
        self.storage.bytes()
    }
}
impl Drop for SoftwareFrame {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.upgrade() {
            pool.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .free
                .push(std::mem::take(&mut self.storage));
            let _ = self.wake.try_send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_recycle_only_after_release() {
        assert_eq!(std::mem::size_of::<Block>(), 64);
        let pool = SoftwareFramePool::default();
        let (wake, receiver) = async_channel::bounded(1);
        let wake = Arc::new(wake);
        let mut first = pool.acquire(128, wake.clone()).unwrap().unwrap();
        let pointer = first.as_ref().as_ptr();
        assert_eq!(pointer as usize % 64, 0);
        first.pixels_mut().fill(7);
        let second = pool.acquire(128, wake.clone()).unwrap().unwrap();
        let third = pool.acquire(128, wake.clone()).unwrap().unwrap();
        assert_ne!(pointer, second.as_ref().as_ptr());
        assert!(pool.acquire(128, wake.clone()).unwrap().is_none());
        assert_eq!(first.as_ref(), &[7; 128]);
        drop(first);
        assert!(receiver.try_recv().is_ok());
        let reused = pool.acquire(128, wake).unwrap().unwrap();
        assert_eq!(reused.as_ref().as_ptr(), pointer);
        drop((second, third, reused));
    }

    #[test]
    fn oversized_frames_fail_before_allocation() {
        let (wake, _) = async_channel::bounded(1);
        assert!(
            SoftwareFramePool::default()
                .acquire(MAX_FRAME_BYTES + 1, Arc::new(wake))
                .is_err()
        );
    }
}
