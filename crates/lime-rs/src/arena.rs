use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr::NonNull;

use crate::Error;

/// An arena allocator for parse tree nodes.
///
/// Allocates from large contiguous blocks, freeing everything at once when
/// the arena is dropped. This is ideal for parse trees that are built during
/// a single parse session and freed together.
///
/// The arena is single-threaded and must not be shared across threads.
#[derive(Debug)]
pub struct Arena {
    inner: NonNull<lime_sys::LimeArena>,
}

impl Arena {
    /// Create a new arena with the given initial block size in bytes.
    ///
    /// # Errors
    ///
    /// Returns `Error::Allocation` if the initial allocation fails.
    pub fn new(initial_size: usize) -> Result<Self, Error> {
        // SAFETY: lime_arena_create allocates an arena block. Returns
        // NULL only on malloc failure.
        let ptr = unsafe { lime_sys::lime_arena_create(initial_size) };
        let inner = NonNull::new(ptr).ok_or(Error::Allocation {
            function: "lime_arena_create",
        })?;
        Ok(Self { inner })
    }

    /// Allocate `size` bytes with pointer alignment.
    ///
    /// The returned pointer is valid until this arena is dropped.
    ///
    /// # Errors
    ///
    /// Returns `Error::Allocation` if the underlying allocator fails.
    pub fn alloc(&self, size: usize) -> Result<NonNull<u8>, Error> {
        // SAFETY: inner is a valid arena pointer. lime_arena_alloc
        // returns NULL only on malloc failure for a new block.
        let ptr = unsafe { lime_sys::lime_arena_alloc(self.inner.as_ptr(), size) };
        NonNull::new(ptr.cast::<u8>()).ok_or(Error::Allocation {
            function: "lime_arena_alloc",
        })
    }

    /// Allocate and zero-fill `size` bytes.
    ///
    /// The returned pointer is valid until this arena is dropped.
    ///
    /// # Errors
    ///
    /// Returns `Error::Allocation` if the underlying allocator fails.
    pub fn calloc(&self, size: usize) -> Result<NonNull<u8>, Error> {
        // SAFETY: inner is a valid arena pointer.
        let ptr = unsafe { lime_sys::lime_arena_calloc(self.inner.as_ptr(), size) };
        NonNull::new(ptr.cast::<u8>()).ok_or(Error::Allocation {
            function: "lime_arena_calloc",
        })
    }

    /// Duplicate a string into the arena.
    ///
    /// The returned pointer is valid until this arena is dropped.
    ///
    /// # Errors
    ///
    /// Returns `Error::NullByte` if `s` contains an interior null byte.
    /// Returns `Error::Allocation` if allocation fails.
    pub fn strdup(&self, s: &str) -> Result<NonNull<c_char>, Error> {
        let c_str = CString::new(s).map_err(|e| Error::NullByte {
            position: e.nul_position(),
        })?;
        // SAFETY: inner is a valid arena, c_str is a valid C string.
        let ptr = unsafe { lime_sys::lime_arena_strdup(self.inner.as_ptr(), c_str.as_ptr()) };
        NonNull::new(ptr).ok_or(Error::Allocation {
            function: "lime_arena_strdup",
        })
    }

    /// Total bytes allocated across all arena blocks.
    #[must_use]
    pub fn total_allocated(&self) -> usize {
        // SAFETY: inner is a valid arena pointer.
        unsafe { lime_sys::lime_arena_total_allocated(self.inner.as_ptr()) }
    }

    /// Total bytes used across all arena blocks.
    #[must_use]
    pub fn total_used(&self) -> usize {
        // SAFETY: inner is a valid arena pointer.
        unsafe { lime_sys::lime_arena_total_used(self.inner.as_ptr()) }
    }

    /// Return the raw arena pointer for FFI use.
    #[must_use]
    pub fn as_ptr(&self) -> *mut lime_sys::LimeArena {
        self.inner.as_ptr()
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        // SAFETY: inner is a valid arena pointer. lime_arena_destroy
        // frees all blocks in the linked list.
        unsafe { lime_sys::lime_arena_destroy(self.inner.as_ptr()) };
    }
}
