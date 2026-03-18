//! Core types and traits for the relational algebra system.
//!
//! This crate provides the fundamental building blocks:
//! - Relational algebra AST
//! - Expression types
//! - Rule traits
//! - Cost model traits
//! - Statistics types

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]

// Re-export main types
pub mod algebra;
pub mod cost;
pub mod expr;
pub mod federated;
pub mod pattern;
pub mod properties;
pub mod rule;
pub mod statistics;

pub use algebra::*;
pub use cost::*;
pub use expr::*;
pub use federated::*;
pub use pattern::*;
pub use properties::*;
pub use rule::*;
pub use statistics::*;
