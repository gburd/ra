//! Native-Rust parse path built on Lime's `--target=rust` output.
//!
//! This module is compiled only under the `rust-parser` feature. During the
//! migration it coexists with the default C FFI path (`crate::lime_parser`);
//! once it passes the identical test suite it becomes the default and the C
//! path is retired.
//!
//! Pieces:
//! - [`value`] — the single `Value` type threaded through every grammar symbol
//! - `builders` — native builder fns the reduction actions call (Task 4)
//! - `generated` — `include!`d `ra_sql.rs` from Lime (wired in Task 5/6)
//! - `driver` — feeds the Rust tokenizer's tokens to the generated parser

mod value;

pub use value::{node_value, Value};
