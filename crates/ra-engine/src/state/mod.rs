//! Reactive system state for neural-guided optimization.
//!
//! The state module provides a compact, atomically-readable snapshot of the
//! current system environment ([`SystemFingerprint`]) that the neural optimizer
//! consumes at every decision point.
//!
//! # Architecture
//!
//! ```text
//! [Background Monitor] --updates--> Arc<AtomicFingerprint>
//!                                          |
//!        +--------+--------+--------+------+
//!        |        |        |        |
//!        v        v        v        v
//!   RuleSelect  Saturation  Hybrid  Feedback
//!   (Phase 2)   (Phase 3)  Extract  (Phase 5)
//!                          (Phase 4)
//! ```
//!
//! The fingerprint is a fixed-size struct (< 64 bytes) that can be
//! cheaply copied via a single atomic load from the shared state.

pub mod fingerprint;

pub use fingerprint::{
    AtomicFingerprint, SystemFingerprint, capabilities, FingerprintReader,
};
