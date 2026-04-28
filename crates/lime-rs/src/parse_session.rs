use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::snapshot::Snapshot;
use crate::Error;

/// A parse session pinned to a snapshot.
///
/// Feeds tokens to the Lime parser engine and drives reductions according
/// to the grammar captured in the snapshot. The session acquires a
/// reference to the snapshot on the C side, ensuring the grammar tables
/// remain valid throughout parsing.
///
/// The lifetime `'s` ties this session to its `Snapshot`.
pub struct ParseSession<'s> {
    inner: NonNull<lime_sys::ParseContext>,
    _snapshot: PhantomData<&'s Snapshot>,
}

impl<'s> ParseSession<'s> {
    /// Begin a new parse session using the given snapshot.
    ///
    /// The C side acquires a reference to the snapshot, so the grammar
    /// tables remain valid even if other Rust `Snapshot` handles are
    /// dropped.
    ///
    /// # Errors
    ///
    /// Returns `Error::NullPointer` if the C context allocation fails.
    pub fn new(snapshot: &'s Snapshot) -> Result<Self, Error> {
        // SAFETY: snapshot.as_ptr() is a valid ParserSnapshot pointer.
        // parse_begin acquires a reference to it.
        let ctx = unsafe { lime_sys::parse_begin(snapshot.as_ptr()) };
        let inner = NonNull::new(ctx).ok_or(Error::NullPointer {
            function: "parse_begin",
        })?;
        Ok(Self {
            inner,
            _snapshot: PhantomData,
        })
    }

    /// Feed a single token to the parser.
    ///
    /// `code` is the token type code (from `TokenKind::to_raw()` or a
    /// grammar-specific keyword code). `value` is the semantic value
    /// pointer whose type must match the grammar's `%token_type`.
    ///
    /// # Safety
    ///
    /// `value` must be a valid pointer for the grammar's `%token_type`,
    /// or null if the grammar does not use token values.
    ///
    /// # Errors
    ///
    /// Returns `Error::ParseToken` if the parser rejects the token.
    pub unsafe fn feed_token(&mut self, code: i32, value: *mut c_void) -> Result<(), Error> {
        // SAFETY: inner is a valid ParseContext. The caller guarantees
        // value is valid for the grammar's token type.
        let rc = unsafe { lime_sys::parse_token(self.inner.as_ptr(), code, value) };
        if rc == 0 {
            Ok(())
        } else {
            Err(Error::ParseToken { code: rc })
        }
    }

    /// Signal end-of-input to the parser.
    ///
    /// Equivalent to feeding a token with code 0 (`TK_EOF`) and a null
    /// value.
    ///
    /// # Errors
    ///
    /// Returns `Error::ParseToken` if the parser rejects the EOF signal
    /// (e.g. incomplete input).
    pub fn feed_eof(&mut self) -> Result<(), Error> {
        // SAFETY: Feeding EOF (code=0, value=null) is always valid
        // regardless of the grammar's %token_type.
        let rc = unsafe { lime_sys::parse_token(self.inner.as_ptr(), 0, std::ptr::null_mut()) };
        if rc == 0 {
            Ok(())
        } else {
            Err(Error::ParseToken { code: rc })
        }
    }

    /// Return the raw `ParseContext` pointer for advanced FFI use.
    #[must_use]
    pub fn as_ptr(&self) -> *mut lime_sys::ParseContext {
        self.inner.as_ptr()
    }
}

impl Drop for ParseSession<'_> {
    fn drop(&mut self) {
        // SAFETY: inner is a valid ParseContext pointer. parse_end
        // releases the snapshot reference and frees internal state.
        unsafe { lime_sys::parse_end(self.inner.as_ptr()) };
    }
}

impl std::fmt::Debug for ParseSession<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParseSession")
            .field("ptr", &self.inner)
            .finish()
    }
}
