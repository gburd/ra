//! Arena-based AST builder layer for the parser.
//!
//! This module provides the builder functions the generated parser's
//! reduction actions invoke (via the wrappers in
//! [`crate::rust_parser::builders`]) to construct the `RelExpr` and `Expr`
//! AST nodes in an arena-based `RaParseState`: `ra_scan`, `ra_filter`,
//! `ra_join`, etc.
//!
//! # Architecture
//!
//! - [`node::RaParseState`] — arena-based state holding all AST nodes
//! - [`node::RaNode`] — opaque tagged pointer (numeric handle, never deref'd)
//! - [`builders`] — the node-building functions

pub mod builders;
pub mod node;

pub use builders::*;
pub use node::{ParseErrors, RaNode, RaParseState, StructuredParseError};
