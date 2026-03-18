//! Function catalog and metadata for SQL query optimization.
//!
//! This crate provides a comprehensive function catalog that the query
//! optimizer uses to reason about SQL function behavior during planning:
//!
//! - **Function Definitions** ([`functions`]): 200+ built-in SQL functions
//!   with signatures, behavioral properties, and cost metadata.
//! - **Property Inference**: Determines whether expressions can be
//!   constant-folded, pushed past joins, or matched to expression indexes.
//! - **Cost Estimation**: Per-function cost multipliers that feed into
//!   the overall plan cost model.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::must_use_candidate)]
#![cfg_attr(test, allow(clippy::float_cmp))]

pub mod functions;

pub use functions::{
    DataType, FunctionCatalog, FunctionCategory, FunctionDefinition,
    FunctionProperties, FunctionSignature, load_catalog_from_toml,
};
