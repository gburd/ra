//! Rule compilation and indexing.
//!
//! This crate builds indices of rules, analyzes dependencies,
//! and manages the rule registry.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod analyzer;
pub mod checker;
pub mod index;
pub mod registry;

pub use analyzer::*;
pub use checker::*;
pub use index::*;
pub use registry::*;
