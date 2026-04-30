//! Database optimization shortcuts.
//!
//! Rewrite rules and fast-path detectors that exploit index
//! structures, metadata caches, materialized views, and other
//! physical properties to avoid full table scans or bypass e-graph
//! optimization entirely.

pub mod fast_path;
pub mod min_max_index;
