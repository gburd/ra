//! Fixed-size ring buffer for streaming statistics ingestion.
//!
//! Provides O(1) push and snapshot with no allocations in the hot
//! path. Uses `AtomicUsize` for lock-free head/size tracking so a
//! single writer thread can push samples while readers take
//! consistent snapshots.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Fixed-capacity ring buffer storing `f64` samples.
///
/// Designed for a single-writer / multiple-reader pattern. The writer
/// calls [`push`](Self::push) to append samples, and any thread can
/// call [`snapshot`](Self::snapshot) to get a consistent copy of the
/// most recent values.
pub struct RingBuffer {
    data: Vec<f64>,
    capacity: usize,
    head: AtomicUsize,
    len: AtomicUsize,
}

impl RingBuffer {
    /// Create a buffer that holds at most `capacity` samples.
    ///
    /// # Panics
    ///
    /// Panics if `capacity` is zero.
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "ring buffer capacity must be > 0");
        Self {
            data: vec![0.0; capacity],
            capacity,
            head: AtomicUsize::new(0),
            len: AtomicUsize::new(0),
        }
    }

    /// Append a sample, overwriting the oldest when full.
    ///
    /// O(1), no allocation.
    pub fn push(&mut self, value: f64) {
        let head = self.head.load(Ordering::Relaxed);
        // Safety: head is always in [0, capacity) due to the modulo
        // below, and `self.data` was allocated with `capacity` slots.
        self.data[head] = value;
        self.head
            .store((head + 1) % self.capacity, Ordering::Release);

        let current_len = self.len.load(Ordering::Relaxed);
        if current_len < self.capacity {
            self.len.store(current_len + 1, Ordering::Release);
        }
    }

    /// Copy the most recent `len` values into a new `Vec`, ordered
    /// oldest-first.
    ///
    /// The snapshot is a consistent point-in-time view: the length
    /// and head are read atomically so readers never see a
    /// half-written state (assuming single writer).
    pub fn snapshot(&self) -> Vec<f64> {
        let len = self.len.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);

        if len == 0 {
            return Vec::new();
        }

        let mut out = Vec::with_capacity(len);
        let start = if head >= len {
            head - len
        } else {
            self.capacity - (len - head)
        };

        for i in 0..len {
            let idx = (start + i) % self.capacity;
            out.push(self.data[idx]);
        }
        out
    }

    /// Number of samples currently stored.
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Maximum number of samples the buffer can hold.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Clear all stored samples.
    pub fn clear(&mut self) {
        self.head.store(0, Ordering::Release);
        self.len.store(0, Ordering::Release);
    }

    /// Compute the mean of all stored samples.
    ///
    /// Returns `None` when the buffer is empty.
    pub fn mean(&self) -> Option<f64> {
        let snap = self.snapshot();
        if snap.is_empty() {
            return None;
        }
        let sum: f64 = snap.iter().sum();
        Some(sum / snap.len() as f64)
    }

    /// Compute the minimum value in the buffer.
    pub fn min(&self) -> Option<f64> {
        let snap = self.snapshot();
        snap.iter().copied().reduce(f64::min)
    }

    /// Compute the maximum value in the buffer.
    pub fn max(&self) -> Option<f64> {
        let snap = self.snapshot();
        snap.iter().copied().reduce(f64::max)
    }
}

impl std::fmt::Debug for RingBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RingBuffer")
            .field("capacity", &self.capacity)
            .field("len", &self.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]

mod tests {
    use super::*;

    #[test]
    fn new_buffer_is_empty() {
        let buf = RingBuffer::new(8);
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.capacity(), 8);
    }

    #[test]
    #[should_panic(expected = "capacity must be > 0")]
    fn zero_capacity_panics() {
        let _ = RingBuffer::new(0);
    }

    #[test]
    fn push_increments_len() {
        let mut buf = RingBuffer::new(4);
        buf.push(1.0);
        assert_eq!(buf.len(), 1);
        buf.push(2.0);
        assert_eq!(buf.len(), 2);
    }

    #[test]
    fn snapshot_returns_values_in_order() {
        let mut buf = RingBuffer::new(4);
        buf.push(10.0);
        buf.push(20.0);
        buf.push(30.0);
        assert_eq!(buf.snapshot(), vec![10.0, 20.0, 30.0]);
    }

    #[test]
    fn wraps_around_when_full() {
        let mut buf = RingBuffer::new(3);
        buf.push(1.0);
        buf.push(2.0);
        buf.push(3.0);
        assert_eq!(buf.len(), 3);
        buf.push(4.0);
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.snapshot(), vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn overwrites_oldest_on_wrap() {
        let mut buf = RingBuffer::new(2);
        buf.push(1.0);
        buf.push(2.0);
        buf.push(3.0);
        buf.push(4.0);
        assert_eq!(buf.snapshot(), vec![3.0, 4.0]);
    }

    #[test]
    fn clear_resets_buffer() {
        let mut buf = RingBuffer::new(4);
        buf.push(1.0);
        buf.push(2.0);
        buf.clear();
        assert!(buf.is_empty());
        assert_eq!(buf.snapshot(), Vec::<f64>::new());
    }

    #[test]
    fn mean_empty() {
        let buf = RingBuffer::new(4);
        assert!(buf.mean().is_none());
    }

    #[test]
    fn mean_values() {
        let mut buf = RingBuffer::new(4);
        buf.push(10.0);
        buf.push(20.0);
        buf.push(30.0);
        let m = buf.mean().expect("should have mean");
        assert!((m - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn min_max() {
        let mut buf = RingBuffer::new(4);
        buf.push(5.0);
        buf.push(1.0);
        buf.push(9.0);
        assert_eq!(buf.min(), Some(1.0));
        assert_eq!(buf.max(), Some(9.0));
    }

    #[test]
    fn min_max_empty() {
        let buf = RingBuffer::new(4);
        assert!(buf.min().is_none());
        assert!(buf.max().is_none());
    }

    #[test]
    fn large_push_sequence() {
        let cap = 1024;
        let mut buf = RingBuffer::new(cap);
        for i in 0..10_000 {
            buf.push(i as f64);
        }
        assert_eq!(buf.len(), cap);
        let snap = buf.snapshot();
        assert_eq!(snap.len(), cap);
        // Last value pushed was 9999
        assert!((snap[cap - 1] - 9999.0).abs() < f64::EPSILON);
        // First value should be 10000 - 1024 = 8976
        assert!((snap[0] - 8976.0).abs() < f64::EPSILON);
    }

    #[test]
    fn capacity_one() {
        let mut buf = RingBuffer::new(1);
        buf.push(42.0);
        assert_eq!(buf.snapshot(), vec![42.0]);
        buf.push(99.0);
        assert_eq!(buf.snapshot(), vec![99.0]);
        assert_eq!(buf.len(), 1);
    }

    #[test]
    fn debug_format() {
        let buf = RingBuffer::new(16);
        let dbg = format!("{buf:?}");
        assert!(dbg.contains("RingBuffer"));
        assert!(dbg.contains("capacity: 16"));
    }
}
