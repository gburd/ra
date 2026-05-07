//! Neural-guided optimization components.
//!
//! This module contains the learned models that guide the optimizer's
//! decisions at every stage of the pipeline:
//!
//! - [`NeuralRuleSelector`] — Pre-saturation: selects which rule groups to apply
//! - [`NeuralConvergenceDetector`] — During saturation: decides when to stop
//! - [`RuleStallingTracker`] — During saturation: disables unproductive rules
//! - Per-node scoring — During extraction: hybrid cost function inputs

pub mod rule_selector;
pub mod saturation;

pub use rule_selector::NeuralRuleSelector;
pub use saturation::{NeuralConvergenceDetector, RuleStallingTracker};
