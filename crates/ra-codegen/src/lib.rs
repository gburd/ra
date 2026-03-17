//! Code generation backends for query execution.
//!
//! This crate provides multiple backends for executing optimized query
//! plans:
//!
//! - **IR** ([`ir`]): Physical plan intermediate representation with
//!   typed operators and column-index-based expressions.
//! - **Volcano** ([`volcano`]): Pull-based iterator interpreter that
//!   executes physical plans against in-memory data.
//! - **Bytecode** ([`bytecode`]): Stack-based bytecode compiler and VM
//!   for fast expression evaluation in tight loops.
//! - **Cranelift** ([`cranelift_backend`]): JIT compilation of integer
//!   expressions to native machine code via Cranelift.
//! - **WASM** ([`wasm`]): WebAssembly code generation for portable,
//!   sandboxed expression evaluation.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod bytecode;
pub mod cranelift_backend;
pub mod ir;
pub mod volcano;
pub mod wasm;
