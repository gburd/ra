//! Cost-based plan extraction from the e-graph.
//!
//! After equality saturation explores the space of equivalent plans,
//! the extractor selects the cheapest plan using a cost model informed
//! by table statistics and neural predictions.

mod api;
mod convert;
pub(crate) mod cost;
mod helpers;
pub mod hybrid_cost;
pub mod neural_cost;
mod scalar;

#[cfg(test)]
mod tests;

pub use api::{extract_best, extract_best_hybrid, extract_best_with_staleness};
#[cfg(feature = "ml")]
pub use api::extract_best_with_cardinality;
pub use convert::rec_expr_to_rel_expr;
pub use cost::RelCostFn;
pub use hybrid_cost::HybridCostFn;
pub use neural_cost::{NeuralPlanScorer, CostWeights};
