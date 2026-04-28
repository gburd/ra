use std::ffi::CStr;

/// Errors produced by the Lime parser wrapper.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A grammar file could not be parsed or contained errors.
    #[error("grammar error: {message}")]
    Grammar {
        /// Human-readable description of the grammar problem.
        message: String,
    },

    /// A syntax error encountered during parsing.
    #[error("parse error at {line}:{column}: {message}")]
    Parse {
        /// 1-based line number where the error occurred.
        line: u32,
        /// 1-based column number where the error occurred.
        column: u32,
        /// Human-readable description of the parse error.
        message: String,
        /// Token names that would have been valid at this position.
        expected: Vec<String>,
    },

    /// The tokenizer encountered invalid input.
    #[error("tokenizer error: {0}")]
    Tokenizer(String),

    /// A C function returned a null pointer unexpectedly.
    #[error("null pointer from {function}")]
    NullPointer {
        /// Name of the C function that returned null.
        function: &'static str,
    },

    /// A C allocator failed.
    #[error("allocation failed in {function}")]
    Allocation {
        /// Name of the C function whose allocation failed.
        function: &'static str,
    },

    /// The input string contained an interior null byte.
    #[error("input contains null byte at position {position}")]
    NullByte {
        /// Byte offset of the null byte.
        position: usize,
    },

    /// A parse token feed returned a non-zero status.
    #[error("parse_token returned error code {code}")]
    ParseToken {
        /// Non-zero return code from `parse_token`.
        code: i32,
    },
}

/// Convert a `LimeError` linked list into a `Vec<Error>`, freeing the
/// C-allocated memory.
///
/// # Safety
///
/// `head` must be null or a valid pointer to a `LimeError` linked list
/// allocated by the Lime C library.
pub unsafe fn collect_lime_errors(head: *mut lime_sys::LimeError) -> Vec<Error> {
    let mut errors = Vec::new();
    let mut current = head;

    while !current.is_null() {
        // SAFETY: caller guarantees the list is valid.
        let err = unsafe { &*current };

        let message = if err.message.is_null() {
            String::new()
        } else {
            // SAFETY: message is a valid C string allocated by Lime.
            unsafe { CStr::from_ptr(err.message) }
                .to_string_lossy()
                .into_owned()
        };

        let expected = if err.expected.is_null() {
            Vec::new()
        } else {
            // SAFETY: expected is a valid C string allocated by Lime.
            let expected_str = unsafe { CStr::from_ptr(err.expected) }.to_string_lossy();
            expected_str
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        };

        errors.push(Error::Parse {
            line: err.line,
            column: err.column,
            message,
            expected,
        });

        current = err.next;
    }

    // SAFETY: head is a valid LimeError list (or null). lime_error_free
    // is null-safe and frees the entire linked list.
    unsafe { lime_sys::lime_error_free(head) };

    errors
}
