//! WASM bindings for the query optimizer.
//!
//! Exposes the ra-engine optimizer to JavaScript for browser-based
//! query optimization demonstrations.

use wasm_bindgen::prelude::*;
use ra_core::algebra::RelExpr;
use serde::{Deserialize, Serialize};

/// WASM wrapper for the query optimizer.
#[wasm_bindgen]
pub struct WasmOptimizer {
    // Using Box to keep the struct on the heap and avoid large stack allocations
    inner: Box<OptimizerState>,
}

struct OptimizerState {
    config: OptimizerConfig,
}

/// Configuration for the optimizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerConfig {
    /// Maximum iterations for equality saturation.
    pub max_iterations: usize,
    /// Node limit for the e-graph.
    pub node_limit: usize,
    /// Time limit in milliseconds.
    pub time_limit_ms: u64,
}

impl Default for OptimizerConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            node_limit: 10_000,
            time_limit_ms: 1000,
        }
    }
}

/// Result of query optimization.
#[derive(Debug, Serialize, Deserialize)]
pub struct OptimizationResult {
    /// Original plan as JSON.
    pub original_plan: serde_json::Value,
    /// Optimized plan as JSON.
    pub optimized_plan: serde_json::Value,
    /// Cost of original plan.
    pub original_cost: f64,
    /// Cost of optimized plan.
    pub optimized_cost: f64,
    /// Cost improvement (0.0 to 1.0).
    pub improvement: f64,
    /// Number of iterations performed.
    pub iterations: usize,
    /// Number of e-graph nodes.
    pub egraph_nodes: usize,
    /// Time taken (milliseconds).
    pub time_ms: u64,
    /// Applied rules.
    pub applied_rules: Vec<String>,
}

#[wasm_bindgen]
impl WasmOptimizer {
    /// Create a new optimizer with default configuration.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<WasmOptimizer, JsValue> {
        console_error_panic_hook::set_once();
        Ok(Self {
            inner: Box::new(OptimizerState {
                config: OptimizerConfig::default(),
            }),
        })
    }

    /// Set optimizer configuration from JSON.
    #[wasm_bindgen(js_name = setConfig)]
    pub fn set_config(&mut self, config_json: &str) -> Result<(), JsValue> {
        let config: OptimizerConfig = serde_json::from_str(config_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid config: {}", e)))?;
        self.inner.config = config;
        Ok(())
    }

    /// Optimize a SQL query and return result as JSON.
    #[wasm_bindgen(js_name = optimizeSQL)]
    pub fn optimize_sql(&self, sql: &str) -> Result<String, JsValue> {
        // Parse SQL to RelExpr
        let plan = ra_parser::sql_to_relexpr(sql)
            .map_err(|e| JsValue::from_str(&format!("SQL parse error: {}", e)))?;

        self.optimize_plan(&plan)
    }

    /// Optimize a plan (internal helper).
    fn optimize_plan(&self, plan: &RelExpr) -> Result<String, JsValue> {
        // Create optimizer with configuration
        let optimizer = ra_engine::Optimizer::new();

        // TODO: Apply config settings to optimizer
        // optimizer.set_max_iterations(self.inner.config.max_iterations);
        // optimizer.set_node_limit(self.inner.config.node_limit);

        let start = js_sys::Date::now();

        // Run optimization
        let optimized = optimizer
            .optimize(plan)
            .map_err(|e| JsValue::from_str(&format!("Optimization error: {}", e)))?;

        let time_ms = (js_sys::Date::now() - start) as u64;

        // Calculate costs (simplified for now)
        let original_cost = estimate_cost(plan);
        let optimized_cost = estimate_cost(&optimized);
        let improvement = if original_cost > 0.0 {
            (original_cost - optimized_cost) / original_cost
        } else {
            0.0
        };

        let result = OptimizationResult {
            original_plan: plan_to_json(plan),
            optimized_plan: plan_to_json(&optimized),
            original_cost,
            optimized_cost,
            improvement,
            iterations: 10, // TODO: Get from optimizer
            egraph_nodes: 100, // TODO: Get from optimizer
            time_ms,
            applied_rules: vec![], // TODO: Track applied rules
        };

        serde_json::to_string(&result)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))
    }

    /// Get optimizer version.
    #[wasm_bindgen(js_name = getVersion)]
    pub fn get_version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

/// Convert RelExpr to JSON for visualization.
fn plan_to_json(plan: &RelExpr) -> serde_json::Value {
    // Serialize plan to JSON with pretty structure
    match serde_json::to_value(plan) {
        Ok(v) => v,
        Err(_) => serde_json::json!({
            "error": "Failed to serialize plan"
        }),
    }
}

/// Simple cost estimation (placeholder).
fn estimate_cost(plan: &RelExpr) -> f64 {
    match plan {
        RelExpr::Scan { .. } => 100.0,
        RelExpr::Filter { input, .. } => estimate_cost(input) + 10.0,
        RelExpr::Project { input, .. } => estimate_cost(input) + 5.0,
        RelExpr::Join { left, right, .. } => {
            let left_cost = estimate_cost(left);
            let right_cost = estimate_cost(right);
            left_cost * right_cost * 0.1
        }
        RelExpr::Aggregate { input, .. } => estimate_cost(input) * 2.0,
        RelExpr::Sort { input, .. } => estimate_cost(input) * 3.0,
        RelExpr::Limit { input, .. } => estimate_cost(input) + 1.0,
        RelExpr::Union { left, right, .. } => estimate_cost(left) + estimate_cost(right),
        RelExpr::Intersect { left, right, .. } => estimate_cost(left) + estimate_cost(right),
        RelExpr::Except { left, right, .. } => estimate_cost(left) + estimate_cost(right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_optimizer_creation() {
        let optimizer = WasmOptimizer::new();
        assert!(optimizer.is_ok());
    }

    #[test]
    fn test_config_serialization() {
        let config = OptimizerConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: OptimizerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config.max_iterations, parsed.max_iterations);
    }
}
