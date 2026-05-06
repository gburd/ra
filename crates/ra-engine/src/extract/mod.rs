//! Cost-based plan extraction from the e-graph.
//!
//! After equality saturation explores the space of equivalent plans,
//! the extractor selects the cheapest plan using a cost model informed
//! by table statistics.

mod api;
mod convert;
pub(crate) mod cost;
mod helpers;
pub mod neural_cost;
pub mod plan_variants;
mod scalar;

#[cfg(test)]
mod tests;

pub use api::{extract_best, extract_best_with_staleness, extract_best_with_neural};
#[cfg(feature = "ml")]
pub use api::extract_best_with_cardinality;
pub use convert::rec_expr_to_rel_expr;
pub use cost::RelCostFn;
pub use neural_cost::{NeuralPlanScorer, CostWeights};
pub use plan_variants::{generate_variants, PlanCandidate, MIN_CONFIDENCE_FOR_VARIANT_SELECTION};
