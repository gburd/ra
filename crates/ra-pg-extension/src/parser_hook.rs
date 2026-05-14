//! Raw parser hook: intercepts PostgreSQL's `raw_parser()` to own all parsing.
//!
//! When the `parser_hook` feature is enabled (requires patched PostgreSQL with
//! `raw_parser_hook`), Ra intercepts SQL at the earliest possible stage —
//! before PostgreSQL's own parser runs. This eliminates double-parsing and
//! enables custom syntax extensions.
//!
//! The parsed [`Statement`] is stored in a thread-local for the planner hook
//! to consume without re-parsing.

use std::cell::RefCell;
use std::ffi::CStr;

use ra_core::algebra::Statement;

/// Thread-local storage for the most recently parsed statement.
///
/// Set by the parser hook, consumed by the planner hook. Using `RefCell`
/// because PostgreSQL is single-threaded per backend.
thread_local! {
    static PARSED_STATEMENT: RefCell<Option<Statement>> = const { RefCell::new(None) };
}

/// Store a parsed statement for the planner hook to consume.
pub fn set_parsed(stmt: Statement) {
    PARSED_STATEMENT.with(|cell| {
        *cell.borrow_mut() = Some(stmt);
    });
}

/// Take the parsed statement (if any). Returns `None` if the parser hook
/// did not fire or the statement was already consumed.
pub fn take_parsed() -> Option<Statement> {
    PARSED_STATEMENT.with(|cell| cell.borrow_mut().take())
}

/// The raw parser hook implementation.
///
/// # Behavior
///
/// - Skips non-default parse modes (PL/pgSQL internal parses, type parsing).
/// - Parses the SQL using Ra's Lime parser via [`ra_parser::parse_statement`].
/// - On success, stores the result in the thread-local and returns NULL
///   (letting PG also parse, since the planner hook still needs PG's Query).
/// - On failure, returns NULL to fall through to PG's standard parser.
///
/// # Safety
///
/// Called by PostgreSQL's raw_parser infrastructure. `query_string` must be
/// a valid C string. `mode` must be a valid `RawParseMode` value.
#[cfg(feature = "parser_hook")]
pub unsafe extern "C-unwind" fn ra_raw_parser_hook(
    query_string: *const std::ffi::c_char,
    mode: pg_sys::RawParseMode,
) -> *mut pg_sys::List {
    use pgrx::pg_sys;

    // Skip non-default modes (PL/pgSQL block parsing, type name parsing).
    if mode != pg_sys::RawParseMode::RAW_PARSE_DEFAULT {
        return std::ptr::null_mut();
    }

    if query_string.is_null() {
        return std::ptr::null_mut();
    }

    let sql = CStr::from_ptr(query_string).to_string_lossy();

    // Parse with Ra's Lime parser.
    match ra_parser::parse_statement(&sql) {
        Ok(stmt) => {
            // Store for the planner hook phase.
            set_parsed(stmt);
            // Return NULL — let PG parse too. We'll use our parsed
            // statement in the planner hook where we have PG's Query
            // tree for OID resolution.
            std::ptr::null_mut()
        }
        Err(_) => {
            // Fall through to PG's standard parser.
            std::ptr::null_mut()
        }
    }
}

/// Register the raw parser hook (requires patched PostgreSQL).
///
/// # Safety
///
/// Must be called during `_PG_init()` while PostgreSQL is initializing.
#[cfg(feature = "parser_hook")]
pub unsafe fn register_parser_hook() {
    use pgrx::pg_sys;
    pg_sys::raw_parser_hook = Some(ra_raw_parser_hook);
}

/// No-op registration when the feature is disabled.
#[cfg(not(feature = "parser_hook"))]
pub fn register_parser_hook() {
    // Parser hook requires patched PostgreSQL. On stock PG, the planner
    // hook handles parsing internally.
}
