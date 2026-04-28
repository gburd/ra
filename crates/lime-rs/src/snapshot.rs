use std::ffi::{c_char, CStr, CString};
use std::ptr::NonNull;

use crate::Error;

/// A reference-counted handle to a frozen parser snapshot.
///
/// A snapshot captures the complete state of a Lime parser's tables
/// (symbols, rules, states, action arrays) at a point in time. Snapshots
/// are immutable after creation and use atomic reference counting, making
/// them safe to share across threads.
///
/// `Clone` increments the reference count; `Drop` decrements it. When the
/// last reference is released the C-side memory is freed.
#[derive(Debug)]
pub struct Snapshot {
    inner: NonNull<lime_sys::ParserSnapshot>,
}

// SAFETY: ParserSnapshot is frozen after creation and reference-counted
// with atomic operations. Concurrent reads are safe; no mutation occurs
// after construction.
unsafe impl Send for Snapshot {}
// SAFETY: See above — all access is read-only through atomic refcount.
unsafe impl Sync for Snapshot {}

impl Snapshot {
    /// Parse a grammar file and produce a new snapshot.
    ///
    /// The snapshot starts with a reference count of 1.
    ///
    /// # Errors
    ///
    /// Returns `Error::Grammar` if the grammar file cannot be read or
    /// contains syntax/semantic errors. Returns `Error::NullByte` if
    /// `path` contains an interior null byte.
    pub fn from_grammar(path: &str) -> Result<Self, Error> {
        let c_path = CString::new(path).map_err(|e| Error::NullByte {
            position: e.nul_position(),
        })?;

        let mut error: *mut c_char = std::ptr::null_mut();

        // SAFETY: c_path is a valid NUL-terminated string. error is a
        // valid out-parameter that receives a malloc'd string on failure.
        let snap = unsafe { lime_sys::lemon_snapshot_create(c_path.as_ptr(), &raw mut error) };

        if snap.is_null() {
            let message = if error.is_null() {
                "unknown grammar error".to_owned()
            } else {
                // SAFETY: error is a valid malloc'd C string on failure.
                let msg = unsafe { CStr::from_ptr(error) }
                    .to_string_lossy()
                    .into_owned();
                // SAFETY: error was malloc'd by the C library.
                unsafe { libc::free(error.cast()) };
                msg
            };
            return Err(Error::Grammar { message });
        }

        Ok(Self {
            // SAFETY: we just verified snap is non-null.
            inner: unsafe { NonNull::new_unchecked(snap) },
        })
    }

    /// Wrap an existing raw snapshot pointer.
    ///
    /// # Safety
    ///
    /// `ptr` must be a valid, non-null `ParserSnapshot` pointer with at
    /// least one live reference. The caller transfers ownership of that
    /// reference to this `Snapshot`.
    pub unsafe fn from_raw(ptr: *mut lime_sys::ParserSnapshot) -> Self {
        Self {
            inner: unsafe { NonNull::new_unchecked(ptr) },
        }
    }

    /// Return the raw pointer without affecting the reference count.
    ///
    /// The caller must not release this pointer — the `Snapshot` still
    /// owns its reference.
    #[must_use]
    pub fn as_ptr(&self) -> *mut lime_sys::ParserSnapshot {
        self.inner.as_ptr()
    }
}

impl Clone for Snapshot {
    fn clone(&self) -> Self {
        // SAFETY: inner is a valid snapshot pointer that we hold a
        // reference to. lemon_snapshot_acquire atomically increments the
        // refcount and returns the same pointer.
        let ptr = unsafe { lime_sys::lemon_snapshot_acquire(self.inner.as_ptr()) };
        Self {
            // SAFETY: lemon_snapshot_acquire returns the same non-null
            // pointer passed in.
            inner: unsafe { NonNull::new_unchecked(ptr) },
        }
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        // SAFETY: inner is a valid snapshot pointer with at least one
        // reference (ours). lemon_snapshot_release atomically decrements
        // the refcount and frees the snapshot when it reaches zero.
        unsafe { lime_sys::lemon_snapshot_release(self.inner.as_ptr()) };
    }
}
