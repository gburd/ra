//! Safe Rust wrapper over the Lime parser generator C library.
//!
//! This crate provides safe, idiomatic Rust types around the low-level
//! FFI bindings in [`lime_sys`]. The main types are:
//!
//! - [`Snapshot`] — a reference-counted, frozen parser grammar snapshot
//! - [`TokenTable`] — a thread-safe keyword lookup table
//! - [`Tokenizer`] — a SIMD-accelerated SQL tokenizer
//! - [`ParseSession`] — a stateful parse session that feeds tokens to the
//!   parser engine
//! - [`Arena`] — a fast bump allocator for parse tree nodes
//! - [`Token`] / [`TokenKind`] — tokenizer output types

pub mod error;

mod arena;
mod parse_session;
mod snapshot;
mod token_table;
mod tokenizer;

pub use arena::Arena;
pub use error::Error;
pub use parse_session::ParseSession;
pub use snapshot::Snapshot;
pub use token_table::TokenTable;
pub use tokenizer::{Token, TokenKind, Tokenizer};
