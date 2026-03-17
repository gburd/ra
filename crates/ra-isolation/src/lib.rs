//! Cross-database isolation testing framework.
//!
//! Adapted from `PostgreSQL`'s isolation test infrastructure, this crate
//! provides tools to parse `.spec` files, schedule concurrent transaction
//! steps across multiple sessions, and detect isolation anomalies such as
//! dirty reads, phantom reads, and deadlocks.
//!
//! # Architecture
//!
//! - [`spec_parser`] - Parses PostgreSQL-format `.spec` files into a
//!   structured [`SpecFile`] representation.
//! - [`session`] - Manages individual database sessions and their
//!   transaction lifecycles.
//! - [`scheduler`] - Controls step ordering across sessions, supporting
//!   explicit permutations and blocking semantics.
//! - [`executor`] - Coordinates sessions and the scheduler to run a
//!   complete isolation test.
//! - [`locks`] - Lock monitoring and deadlock detection.
//! - [`snapshot`] - Snapshot visibility queries for verifying isolation
//!   levels.
//! - [`markers`] - Synchronization markers for coordinating steps.
//! - [`events`] - Event recording for test output and diagnostics.
//! - [`wasm_bridge`] - Bridge adapters for connecting WASM database
//!   backends (`SQLite`, `DuckDB`) to the isolation testing framework.

#![warn(missing_docs)]

pub mod adapter;
pub mod events;
pub mod executor;
pub mod locks;
pub mod markers;
pub mod scheduler;
pub mod session;
pub mod snapshot;
pub mod spec_parser;
pub mod wasm_bridge;

/// Direct `ra-wasm` adapter integration.
///
/// Provides convenience functions to wrap `ra_wasm` adapters
/// (`SqliteAdapter`, `DuckDbAdapter`) as isolation test adapters.
/// Requires the `wasm` feature flag.
#[cfg(feature = "wasm")]
pub mod wasm_adapters;

pub use adapter::DatabaseAdapter;
pub use events::{TestEvent, TestEventLog};
pub use executor::{TestExecutor, TestResult};
pub use locks::{LockInfo, LockMonitor, LockType};
pub use markers::Marker;
pub use scheduler::{Scheduler, StepOrder};
pub use session::Session;
pub use snapshot::SnapshotQuery;
pub use spec_parser::SpecFile;
