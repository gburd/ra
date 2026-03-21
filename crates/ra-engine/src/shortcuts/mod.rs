//! Database optimization shortcuts.
//!
//! Rewrite rules that exploit index structures, metadata caches,
//! and other physical properties to avoid full table scans.

pub mod min_max_index;
