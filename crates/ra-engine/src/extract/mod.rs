//! Cost-based plan extraction from the e-graph.
//!
//! After equality saturation explores the space of equivalent plans,
//! the extractor selects the cheapest plan using a cost model informed
//! by table statistics.

mod api;
mod convert;
pub(crate) mod cost;
mod helpers;
mod scalar;

#[cfg(test)]
mod tests;

pub use api::{extract_best, extract_best_with_cardinality, extract_best_with_staleness};
pub use convert::rec_expr_to_rel_expr;
pub use cost::RelCostFn;
