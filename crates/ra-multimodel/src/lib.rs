//! Multi-model optimization rules for non-relational data models.
//!
//! This crate provides optimization rules for graph, document, and
//! time-series databases. Each module contains model-specific
//! operators, cost models, and egg rewrite rules.
//!
//! # Modules
//!
//! - [`graph`] - Graph traversal optimizations (Neo4j, `JanusGraph`)
//! - [`document`] - Document query optimizations (`MongoDB`, Couchbase)
//! - [`timeseries`] - Time-series optimizations (`InfluxDB`, `TimescaleDB`)
//! - [`cost`] - Multi-model cost estimation

#![warn(missing_docs)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::similar_names)]

pub mod cost;
pub mod document;
pub mod graph;
pub mod timeseries;
