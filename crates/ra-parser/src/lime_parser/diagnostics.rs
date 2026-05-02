//! Safe wrappers around the Lime parser diagnostics FFI.
//!
//! The generated parser (`%name ra`) emits `raTokenName`, `raState`,
//! and `raExpectedTokens` into `ra_sql.c`. These functions expose
//! LALR parser state for precise error reporting.

use std::ffi::CStr;
use std::os::raw::{c_int, c_void};

use super::{raExpectedTokens, raState, raTokenName};

/// Return the grammar name for a terminal token code.
///
/// Returns `None` if `code` is out of range or the function
/// returns NULL.
#[must_use]
pub fn token_name(code: i32) -> Option<&'static str> {
    // SAFETY: raTokenName returns a static string pointer or NULL.
    let ptr = unsafe { raTokenName(code as c_int) };
    if ptr.is_null() {
        return None;
    }
    // SAFETY: the returned pointer is a static C string literal
    // embedded in the generated parser.
    let cstr = unsafe { CStr::from_ptr(ptr) };
    cstr.to_str().ok()
}

/// Return the current LALR state number of the parser.
///
/// Returns `None` if the parser pointer is invalid or the state
/// number is negative.
///
/// # Safety
/// `parser` must be a valid handle returned by `raAlloc`, or NULL.
pub unsafe fn parser_state(parser: *mut c_void) -> Option<i32> {
    let state = unsafe { raState(parser) };
    if state < 0 {
        None
    } else {
        Some(state)
    }
}

/// Return the names of tokens valid at the given LALR state.
///
/// Uses the two-call pattern: first call with a zero-length buffer
/// to get the count, then allocate and fill.
#[must_use]
pub fn expected_tokens(stateno: i32) -> Vec<&'static str> {
    // First call: get total count.
    // SAFETY: NULL out buffer with max=0 is safe per the API.
    let count = unsafe {
        raExpectedTokens(stateno as c_int, std::ptr::null_mut(), 0)
    };
    if count <= 0 {
        return Vec::new();
    }

    let count_usize = usize::try_from(count).unwrap_or(0);
    let mut codes: Vec<c_int> = vec![0; count_usize];

    // Second call: fill the buffer.
    // SAFETY: codes has exactly `count` elements.
    let filled = unsafe {
        raExpectedTokens(
            stateno as c_int,
            codes.as_mut_ptr(),
            count,
        )
    };

    let actual = usize::try_from(filled).unwrap_or(0).min(count_usize);
    let mut names = Vec::with_capacity(actual);
    for &code in &codes[..actual] {
        if let Some(name) = token_name(code) {
            // Replace Lime's internal EOF symbol "$" with a
            // human-readable name.
            if name == "$" {
                names.push("end of input");
            } else {
                names.push(name);
            }
        }
    }
    names
}
