//! Parser module for Ra SQL parsing with profile support.
//!
//! This module provides the main `RaParser` facade for parsing SQL queries
//! with support for multiple dialects, versions, and extensions through a
//! profile-based system.

pub mod ra_parser;
pub mod inference;

pub use ra_parser::RaParser;
pub use inference::DialectInference;
