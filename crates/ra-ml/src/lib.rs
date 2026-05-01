//! ML-based cardinality estimation for the RA query optimizer.
//!
//! This crate provides learned cardinality estimation using neural
//! networks, inspired by research papers including:
//!
//! - **MSCN** (Multi-Set Convolutional Network): Feature encoding
//!   of query plans using one-hot table/column/operator vectors
//! - **Neo** (Learned Query Optimizer): End-to-end learned cost
//!   models that replace heuristic cardinality estimation
//! - **`DeepDB`** / **`NeuroCard`**: Data-driven models that learn
//!   joint data distributions
//!
//! # Architecture
//!
//! The crate is organized into four modules:
//!
//! - [`nn`] -- Lightweight feed-forward neural network for inference
//! - [`features`] -- Feature extraction from [`ra_core::algebra::RelExpr`]
//! - [`estimator`] -- Cardinality estimation trait and implementations
//! - [`training`] -- Training data generation and model evaluation
//!
//! # Usage
//!
//! ```rust
//! use ra_ml::estimator::{
//!     CardinalityEstimator, MlEstimator, SimpleStatsProvider,
//! };
//! use ra_core::algebra::RelExpr;
//! use ra_core::statistics::Statistics;
//!
//! // Set up statistics
//! let mut provider = SimpleStatsProvider::new();
//! provider.add("users", Statistics::new(10_000.0));
//!
//! // Create an ML estimator with default (untrained) weights
//! let estimator = MlEstimator::with_default_model(
//!     &["users"],
//!     &["id", "name"],
//! );
//!
//! // Estimate cardinality
//! let plan = RelExpr::scan("users");
//! let estimate = estimator.estimate(&plan, &provider);
//! assert!(estimate.rows >= 1.0);
//! ```
//!
//! # Training Workflow
//!
//! 1. Generate training data using [`training::TrainingDataset`]
//! 2. Export to JSON with [`TrainingDataset::to_json`]
//! 3. Train the model externally (Python recommended)
//! 4. Export trained weights as JSON
//! 5. Load in Rust with [`nn::FeedForwardNet::from_json`]

#![warn(missing_docs)]
#![cfg_attr(
    feature = "streaming",
    allow(non_local_definitions) // Abomonation derive macro generates this warning
)]

#[cfg(feature = "streaming")]
pub mod belief_network;
pub mod estimator;
pub mod features;
pub mod nn;
#[cfg(feature = "streaming")]
pub mod storage;
#[cfg(feature = "streaming")]
pub mod streaming;
pub mod training;
