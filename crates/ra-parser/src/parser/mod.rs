//! Parser module for Ra SQL parsing with profile support.
//!
//! This module provides the main `RaParser` facade for parsing SQL queries
//! with support for multiple dialects, versions, and extensions through a
//! profile-based system.

pub mod inference;
pub mod profile_dialect;
pub mod ra_parser;

pub use inference::DialectInference;
pub use profile_dialect::ProfileDialect;
pub use ra_parser::RaParser;
