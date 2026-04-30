//! Ra — relational algebra query optimization engine.
//!
//! Ra codifies 1,361+ database transformation rules into a unified
//! optimization framework using equality saturation (egg e-graphs)
//! and differential dataflow.
//!
//! # Architecture Layers
//!
//! **Core** (default): Production-ready query optimization.
//! - [`ra_core`] — AST types, cost model, statistics, configuration
//! - [`ra_parser`] — SQL → `RelExpr` conversion, .rra rule file parsing
//! - [`ra_compiler`] — Rule registry and compilation
//! - [`ra_engine`] — E-graph equality saturation optimizer
//! - [`ra_dialect`] — SQL dialect translation
//! - [`ra_hardware`] — Hardware-aware cost models
//! - [`ra_cache_api`] — Plan cache trait interface
//!
//! **CLI** (feature `cli`): Research and educational tooling.
//! - `ra-cli` — 41-command CLI for rule exploration (binary, build separately)
//! - [`ra_adapters`] — Database connection wrappers
//! - [`ra_metadata`] — Schema introspection
//!
//! **Experimental** (feature `experimental`): Research innovations.
//! - [`ra_ml`] — Neural network cardinality estimation
//! - [`ra_cache_impl`] — Reference LRU/LFU/adaptive cache
//! - [`ra_adaptive`] — Runtime reoptimization
//! - [`ra_quel_parser`] — QUEL language parser (stub)

#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

// ── Core layer (always available) ──

pub use ra_cache_api as cache_api;
pub use ra_compiler as compiler;
pub use ra_core as core;
pub use ra_dialect as dialect;
pub use ra_engine as engine;
pub use ra_hardware as hardware;
pub use ra_parser as parser;

// ── CLI layer (feature "cli") ──

#[cfg(feature = "cli")]
pub use ra_adapters as adapters;
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
pub use ra_quel_parser as quel_parser;
#[cfg(feature = "experimental")]
pub use ra_test_utils as test_utils;
