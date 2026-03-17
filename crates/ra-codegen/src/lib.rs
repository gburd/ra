//! Code generation backends for query execution.
//!
//! This crate generates executable code from optimized query plans:
//! - Cranelift JIT compilation
//! - WASM compilation
//! - Bytecode interpreter

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod bytecode;
pub mod cranelift_backend;
pub mod ir;
pub mod volcano;
pub mod wasm;

pub use bytecode::*;
pub use cranelift_backend::*;
pub use ir::*;
pub use volcano::*;
pub use wasm::*;
