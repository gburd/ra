//! Lime-based SQL parser integration.
//!
//! This module provides the entry point for parsing SQL using the
//! Lime-generated LALR(1) parser. The grammar is compiled at build time
//! by `build.rs` and linked as a C static library.
//!
//! The generated C parser calls back into the `extern "C"` functions in
//! [`crate::ffi::builders`] to construct `RelExpr` / `Expr` AST nodes.
//!
//! # Usage
//!
//! ```ignore
//! use ra_parser::lime_parser;
//! let rel = lime_parser::parse_sql("SELECT id FROM users WHERE age > 21")?;
//! ```

// Placeholder module. The full parse_sql() entry point will be wired up
// when the tokenizer integration is complete (Phase 4).
