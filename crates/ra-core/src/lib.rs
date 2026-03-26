#![allow(clippy::doc_markdown)]
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
pub mod distributed_agg;
pub mod document_algebra;
pub mod distribution;
pub mod expr;
pub mod facts;
pub mod federated;
pub mod isolation;
pub mod formats;
pub mod pattern;
pub mod precondition;
pub mod row_pattern;
pub mod physical_properties;
pub mod properties;
pub mod rule;
pub mod statistics;
pub mod table_formats;

pub use algebra::*;
pub use cost::*;
pub use distributed_agg::*;
pub use distribution::*;
pub use expr::*;
pub use facts::{
    DataType, EmptyFactsProvider, FactsProvider, ForeignKey,
    HardwareProfile as CoreHardwareProfile, IndexInfo,
    IndexType, OperatorStats, SqlDialect, TableInfo,
    TableStats as CoreTableStats,
};
pub use federated::*;
pub use isolation::{
    BackendKind, IsolationLevel, MultiXactPressure,
    TransactionContext,
};
pub use pattern::*;
pub use precondition::*;
pub use row_pattern::*;
pub use physical_properties::*;
pub use properties::*;
pub use rule::*;
pub use statistics::*;
