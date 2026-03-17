//! Query optimization engine using egg and differential dataflow.
//!
//! This crate provides the core optimization algorithms:
//! - E-graph construction and equality saturation
//! - Cost-based plan extraction
//! - Incremental maintenance with differential dataflow

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod analysis;
pub mod differential;
pub mod egraph;
pub mod extract;
pub mod memo;
pub mod rewrite;
pub mod timely_integration;

pub use analysis::*;
pub use differential::*;
pub use egraph::*;
pub use extract::*;
pub use memo::*;
pub use rewrite::*;
