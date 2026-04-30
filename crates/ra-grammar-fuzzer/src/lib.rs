//! Grammar-based property testing for SQL optimizer correctness.
//!
//! This crate generates SQL queries through grammar-guided fuzzing
//! and validates optimizer invariants via property-based testing.
//!
//! # Architecture
//!
//! - [`storyline`] -- SQL lifecycle patterns (create/insert/query/drop)
//! - [`generator`] -- Grammar-guided SQL expression generation
//! - [`properties`] -- Optimizer property validators (convergence, cost monotonicity)
//! - [`minimizer`] -- Automatic test case minimization for failure reproduction
//! - [`reference`] -- Reference optimizer comparison (PostgreSQL, DuckDB)
//!
//! # Feature Flags
//!
//! - `long-duration-testing` -- Enable long-running fuzz campaigns
//! - `reference-comparison` -- Enable reference optimizer comparison

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
// Test-oriented crate: property tests legitimately use expect/unwrap.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

pub mod generator;
pub mod minimizer;
pub mod properties;
#[cfg(feature = "reference-comparison")]
pub mod reference;
pub mod storyline;

pub use generator::SqlGenerator;
pub use minimizer::TestMinimizer;
pub use properties::{OptimizerProperty, PropertyValidator};
pub use storyline::{SqlStoryline, StorylinePattern};
