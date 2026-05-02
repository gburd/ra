//! FFI layer for Lime parser callbacks.
//!
//! This module provides the `extern "C"` builder functions that the
//! Lime-generated parser calls as reduction actions. The parser calls
//! `ra_scan`, `ra_filter`, `ra_join`, etc. to construct the `RelExpr`
//! and `Expr` AST nodes in an arena-based `RaParseState`.
//!
//! # Architecture
//!
//! - [`node::RaParseState`] — arena-based state holding all AST nodes
//! - [`node::RaNode`] — opaque tagged pointer returned to C code
//! - [`builders`] — `#[no_mangle] extern "C"` functions called by Lime

pub mod builders;
pub mod node;

pub use builders::*;
pub use node::{ParseErrors, RaNode, RaParseState, StructuredParseError};
