use std::ffi::CString;
use std::ptr::NonNull;

use crate::Error;

/// A thread-safe keyword lookup table for the tokenizer.
///
/// Maps lexeme strings (e.g. `"SELECT"`, `"FROM"`) to integer token codes
/// used by the parser. The table uses an internal reader-writer lock so
/// multiple threads can look up tokens concurrently while writes are
/// exclusive.
#[derive(Debug)]
pub struct TokenTable {
    inner: NonNull<lime_sys::TokenTable>,
}

// SAFETY: TokenTable uses pthread_rwlock internally for all operations.
// lookup_token acquires a read lock; add_token and
// remove_tokens_by_extension acquire a write lock.
unsafe impl Send for TokenTable {}
// SAFETY: See above — all operations are internally synchronized.
unsafe impl Sync for TokenTable {}

impl TokenTable {
    /// Create a new token table with the given initial capacity.
    ///
    /// # Errors
    ///
    /// Returns `Error::Allocation` if the initial allocation fails.
    pub fn new(initial_capacity: u32) -> Result<Self, Error> {
        // SAFETY: create_token_table allocates a new table. Returns NULL
        // on malloc failure.
        let ptr = unsafe { lime_sys::create_token_table(initial_capacity) };
        let inner = NonNull::new(ptr).ok_or(Error::Allocation {
            function: "create_token_table",
        })?;
        Ok(Self { inner })
    }

    /// Look up a token code by its lexeme string.
    ///
    /// Returns `Some(code)` if the lexeme is registered, or `None` if not
    /// found. The lookup acquires a read lock internally.
    #[must_use]
    pub fn lookup(&self, lexeme: &str) -> Option<i32> {
        // SAFETY: inner is a valid table pointer. The string pointer and
        // length are derived from a valid Rust str. lookup_token acquires
        // a read lock and does not store the pointer.
        let code = unsafe {
            lime_sys::lookup_token(self.inner.as_ptr(), lexeme.as_ptr().cast(), lexeme.len())
        };
        if code < 0 {
            None
        } else {
            Some(code)
        }
    }

    /// Register a new keyword in the table.
    ///
    /// `lexeme` is the keyword string (e.g. `"SELECT"`), `code` is the
    /// positive token code, and `extension_id` identifies which extension
    /// owns the token (0 for the base grammar).
    ///
    /// # Errors
    ///
    /// Returns `Error::NullByte` if `lexeme` contains a null byte.
    /// Returns `Error::Tokenizer` if the insertion fails (duplicate or
    /// allocation error).
    pub fn add(&mut self, lexeme: &str, code: i32, extension_id: u32) -> Result<(), Error> {
        let c_lexeme = CString::new(lexeme).map_err(|e| Error::NullByte {
            position: e.nul_position(),
        })?;
        // SAFETY: inner is a valid table pointer. c_lexeme is a valid
        // NUL-terminated string. add_token copies the lexeme internally.
        let ok = unsafe {
            lime_sys::add_token(self.inner.as_ptr(), c_lexeme.as_ptr(), code, extension_id)
        };
        if ok {
            Ok(())
        } else {
            Err(Error::Tokenizer(format!(
                "failed to add token '{lexeme}' with code {code}"
            )))
        }
    }

    /// Remove all tokens belonging to the given extension.
    ///
    /// Returns `true` if the operation succeeded.
    ///
    /// # Errors
    ///
    /// Returns `Error::Tokenizer` if the removal or hash rebuild fails.
    pub fn remove_extension(&mut self, extension_id: u32) -> Result<(), Error> {
        // SAFETY: inner is a valid table pointer.
        let ok = unsafe { lime_sys::remove_tokens_by_extension(self.inner.as_ptr(), extension_id) };
        if ok {
            Ok(())
        } else {
            Err(Error::Tokenizer(format!(
                "failed to remove tokens for extension {extension_id}"
            )))
        }
    }

    /// Return the raw pointer for FFI use (e.g. passing to `Tokenizer`).
    pub(crate) fn as_ptr(&self) -> *mut lime_sys::TokenTable {
        self.inner.as_ptr()
    }
}

impl Drop for TokenTable {
    fn drop(&mut self) {
        // SAFETY: inner is a valid table pointer.
        // destroy_token_table frees the table and all owned strings.
        unsafe { lime_sys::destroy_token_table(self.inner.as_ptr()) };
    }
}
