//! Ra — relational algebra query optimization engine.
//!
//! Ra codifies database transformation rules into a unified optimization
//! framework using equality saturation (egg e-graphs) and differential
//! dataflow.
//!
//! # Architecture Layers
//!
//! **Core** (default): query optimization.
//! - [`ra_core`] — AST types, cost model, statistics, configuration
//! - [`ra_parser`] — SQL → `RelExpr` conversion, .rra rule file parsing
//! - [`ra_compiler`] — Rule registry and compilation
//! - [`ra_engine`] — E-graph equality saturation optimizer
//! - [`ra_hardware`] — Hardware-aware cost models
//! - [`ra_cache_api`] — Plan cache trait interface
//!
//! **CLI** (feature `cli`): Research and educational tooling.
//! - `ra-cli` — CLI for rule exploration (binary, build separately)
//! - [`ra_metadata`] — Schema introspection
//!
//! **Experimental** (feature `experimental`): Research innovations.
//! - [`ra_ml`] — Neural network cardinality estimation
//! - [`ra_cache_impl`] — Reference LRU/LFU/adaptive cache
//! - [`ra_adaptive`] — Runtime reoptimization
//!
//! Dialect translation, DB adapters, and the QUEL parser stub moved to
//! the `ra-lab` repository (<https://codeberg.org/gregburd/ra-lab>).

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

// ── Core layer (always available) ──

pub use ra_cache_api as cache_api;
pub use ra_compiler as compiler;
pub use ra_core as core;
pub use ra_engine as engine;
pub use ra_hardware as hardware;
pub use ra_parser as parser;

// ── CLI layer (feature "cli") ──

#[cfg(feature = "cli")]
pub use ra_metadata as metadata;

// ── Experimental layer (feature "experimental") ──

#[cfg(feature = "experimental")]
pub use ra_adaptive as adaptive;
#[cfg(feature = "experimental")]
pub use ra_cache_impl as cache_impl;
#[cfg(feature = "experimental")]
pub use ra_ml as ml;
#[cfg(feature = "experimental")]
pub use ra_test_utils as test_utils;
