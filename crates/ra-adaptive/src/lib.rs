//! Adaptive query execution with runtime reoptimization.
//!
//! This crate implements adaptive execution strategies that monitor
//! runtime statistics and reoptimize query plans when the optimizer's
//! estimates diverge from observed reality. Inspired by SQL Server
//! adaptive joins, Spark Adaptive Query Execution (AQE), and
//! `PostgreSQL` runtime partition pruning.
//!
//! # Architecture
//!
//! - [`runtime_stats`]: Collects actual row counts, selectivity, and
//!   timing during execution at instrumentation points.
//! - [`triggers`]: Detects when runtime statistics diverge enough
//!   from estimates to warrant reoptimization.
//! - [`plan_switch`]: Swaps physical operators (join algorithms,
//!   scan strategies) based on observed data characteristics.
//! - [`checkpoint`]: Marks safe points in execution where plan
//!   transitions can occur without losing intermediate state.
//! - [`executor`]: Orchestrates adaptive execution by threading
//!   statistics collection, trigger evaluation, and plan switching
//!   through the query execution pipeline.

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
// Test code legitimately uses expect/unwrap for assertions and setup.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]
#![allow(clippy::cast_possible_wrap)]

pub mod batch;
pub mod cache_adapter;
pub mod checkpoint;
pub mod executor;
pub mod plan_switch;
pub mod runtime_stats;
pub mod triggers;
