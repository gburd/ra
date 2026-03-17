//! Automatic rule discovery from execution logs.
//!
//! This crate implements a pipeline for discovering optimization
//! rules from observed query execution data.  The approach is
//! inspired by learned query optimization research (Neo, Bao) and
//! adapts frequent pattern mining to relational algebra rewrites.
//!
//! # Architecture
//!
//! The discovery process follows these stages:
//!
//! 1. **Log collection** ([`log`]) -- record query plans with their
//!    execution metrics (wall time, actual cardinalities, costs).
//!
//! 2. **Fingerprinting** ([`fingerprint`]) -- convert plan trees
//!    into linearized token sequences, abstracting over concrete
//!    table names and constants so structural patterns can be
//!    recognized across queries.
//!
//! 3. **Pattern mining** ([`mining`]) -- apply FP-Growth-style
//!    n-gram counting to find frequently occurring operator
//!    subsequences and pairs of patterns that correlate with
//!    optimization improvements.
//!
//! 4. **Rule synthesis** ([`synthesis`]) -- transform mined pattern
//!    pairs into candidate rewrite rules with structural match and
//!    replacement patterns.
//!
//! 5. **Validation** ([`validation`]) -- test candidate rules on
//!    held-out queries to verify they improve cost without
//!    introducing regressions.
//!
//! 6. **Pipeline** ([`pipeline`]) -- orchestrate the full cycle
//!    with support for incremental / continuous learning.
//!
//! # Example
//!
//! ```rust
//! use ra_discovery::log::{ExecutionLog, LogStore};
//! use ra_discovery::pipeline::{run_discovery, PipelineConfig};
//! use ra_discovery::validation::CostEstimator;
//! use ra_core::algebra::RelExpr;
//! use ra_core::cost::Cost;
//!
//! let mut store = LogStore::new();
//! // ... record execution logs via store.record(...) ...
//!
//! let estimator = CostEstimator::new(|_plan| {
//!     Cost::new(1.0, 1.0, 0.0, 0)
//! });
//! let config = PipelineConfig::default();
//! let output = run_discovery(&store, &estimator, &config);
//!
//! for (rule, result) in &output.accepted_rules {
//!     assert!(result.passed);
//! }
//! ```

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]

pub mod fingerprint;
pub mod log;
pub mod mining;
pub mod pipeline;
pub mod synthesis;
pub mod validation;
