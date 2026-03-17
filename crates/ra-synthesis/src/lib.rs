//! Natural language to relational algebra query synthesis.
//!
//! This crate converts natural language questions into relational
//! algebra expressions ([`ra_core::RelExpr`]) using a pattern-based
//! approach: parse intent from text, resolve against a schema, and
//! build a validated query plan.
//!
//! # Pipeline
//!
//! 1. **Schema encoding** -- describe available tables, columns,
//!    and relationships via [`schema::SchemaInfo`].
//! 2. **Intent parsing** -- extract query intent (select, filter,
//!    join, aggregate, sort, limit) from natural language via
//!    [`intent::IntentParser`].
//! 3. **Query generation** -- convert parsed intent into a
//!    [`ra_core::RelExpr`] tree via [`generator::QueryGenerator`].
//! 4. **Validation** -- verify the plan references only existing
//!    tables and columns via [`validator::QueryValidator`].
//! 5. **SQL rendering** -- render the plan to a SQL string via
//!    [`render::SqlRenderer`].

#![warn(missing_docs)]

pub mod error;
pub mod generator;
pub mod intent;
pub mod render;
pub mod schema;
pub mod synthesizer;
pub mod validator;

pub use error::SynthesisError;
pub use schema::SchemaInfo;
pub use synthesizer::{SynthesisResult, Synthesizer};
