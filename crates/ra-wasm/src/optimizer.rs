//! WASM bindings for the query optimizer.
//!
//! Exposes the ra-engine optimizer to JavaScript for browser-based
//! query optimization demonstrations, including hardware-aware cost
//! models and statistics configuration.

use wasm_bindgen::prelude::*;
use ra_core::algebra::RelExpr;
use ra_core::statistics::Statistics;
use ra_hardware::HardwareProfile;
use serde::{Deserialize, Serialize};

/// WASM wrapper for the query optimizer.
#[wasm_bindgen]
pub struct WasmOptimizer {
    inner: Box<OptimizerState>,
}

struct OptimizerState {
    config: OptimizerConfig,
    hardware_profile: Option<HardwareProfile>,
    table_stats: Vec<(String, Statistics)>,
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
    /// Cost breakdown for the original plan.
    pub original_cost_breakdown: CostBreakdownJs,
    /// Cost breakdown for the optimized plan.
    pub optimized_cost_breakdown: CostBreakdownJs,
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
    /// Hardware profile name used for this optimization.
    pub hardware_profile_name: String,
}

/// JS-friendly cost breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostBreakdownJs {
    /// CPU computation cost.
    pub cpu: f64,
    /// I/O cost.
    pub io: f64,
    /// Memory allocation cost.
    pub memory: f64,
    /// Network transfer cost.
    pub network: f64,
    /// Total cost.
    pub total: f64,
}

/// JS-friendly table statistics for configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableStatsJs {
    /// Table name.
    pub table: String,
    /// Estimated row count.
    pub row_count: f64,
    /// Average row size in bytes.
    pub avg_row_size: u64,
    /// Number of distinct values.
    pub distinct_count: u64,
    /// Null fraction (0.0 to 1.0).
    pub null_fraction: f64,
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
                hardware_profile: None,
                table_stats: Vec::new(),
            }),
        })
    }

    /// Set optimizer configuration from JSON.
    #[wasm_bindgen(js_name = setConfig)]
    pub fn set_config(&mut self, config_json: &str) -> Result<(), JsValue> {
        let config: OptimizerConfig = serde_json::from_str(config_json)
            .map_err(|e| JsValue::from_str(&format!("Invalid config: {e}")))?;
        self.inner.config = config;
        Ok(())
    }

    /// Set the hardware profile from JSON.
    #[wasm_bindgen(js_name = setHardwareProfile)]
    pub fn set_hardware_profile(
        &mut self,
        profile_json: &str,
    ) -> Result<(), JsValue> {
        let profile: HardwareProfile =
            serde_json::from_str(profile_json)
                .map_err(|e| JsValue::from_str(
                    &format!("Invalid hardware profile: {e}"),
                ))?;
        self.inner.hardware_profile = Some(profile);
        Ok(())
    }

    /// Set a named hardware profile.
    #[wasm_bindgen(js_name = setHardwarePreset)]
    pub fn set_hardware_preset(&mut self, preset: &str) -> Result<(), JsValue> {
        let profile = match preset {
            "gpu_server" => HardwareProfile::gpu_server(),
            "fpga_appliance" => HardwareProfile::fpga_appliance(),
            "laptop" => ra_hardware::detect_hardware(),
            _ => {
                return Err(JsValue::from_str(
                    &format!("Unknown preset: {preset}"),
                ));
            }
        };
        self.inner.hardware_profile = Some(profile);
        Ok(())
    }

    /// Add table statistics from JSON.
    #[wasm_bindgen(js_name = addTableStats)]
    pub fn add_table_stats(
        &mut self,
        stats_json: &str,
    ) -> Result<(), JsValue> {
        let stats: TableStatsJs = serde_json::from_str(stats_json)
            .map_err(|e| JsValue::from_str(
                &format!("Invalid table stats: {e}"),
            ))?;
        let core_stats = Statistics::new(stats.row_count);
        self.inner.table_stats.push((stats.table, core_stats));
        Ok(())
    }

    /// Clear all table statistics.
    #[wasm_bindgen(js_name = clearTableStats)]
    pub fn clear_table_stats(&mut self) {
        self.inner.table_stats.clear();
    }

    /// Get available hardware profiles as JSON.
    #[wasm_bindgen(js_name = getHardwareProfiles)]
    pub fn get_hardware_profiles() -> Result<String, JsValue> {
        let profiles = vec![
            ("gpu_server", HardwareProfile::gpu_server()),
            ("fpga_appliance", HardwareProfile::fpga_appliance()),
            ("auto_detect", ra_hardware::detect_hardware()),
        ];
        serde_json::to_string(&profiles)
            .map_err(|e| JsValue::from_str(
                &format!("Serialization error: {e}"),
            ))
    }

    /// Get the current hardware profile as JSON.
    #[wasm_bindgen(js_name = getCurrentHardwareProfile)]
    pub fn get_current_hardware_profile(&self) -> Result<String, JsValue> {
        let profile = self.inner.hardware_profile.clone()
            .unwrap_or_else(ra_hardware::detect_hardware);
        serde_json::to_string(&profile)
            .map_err(|e| JsValue::from_str(
                &format!("Serialization error: {e}"),
            ))
    }

    /// Optimize a SQL query and return result as JSON.
    #[wasm_bindgen(js_name = optimizeSQL)]
    pub fn optimize_sql(&self, sql: &str) -> Result<String, JsValue> {
        let plan = ra_parser::sql_to_relexpr(sql)
            .map_err(|e| JsValue::from_str(
                &format!("SQL parse error: {e}"),
            ))?;
        self.optimize_plan(&plan)
    }

    /// Optimize a plan provided as JSON and return result as JSON.
    #[wasm_bindgen(js_name = optimizePlanJSON)]
    pub fn optimize_plan_json(
        &self,
        plan_json: &str,
    ) -> Result<String, JsValue> {
        let plan: RelExpr = serde_json::from_str(plan_json)
            .map_err(|e| JsValue::from_str(
                &format!("Invalid plan JSON: {e}"),
            ))?;
        self.optimize_plan(&plan)
    }

    fn optimize_plan(&self, plan: &RelExpr) -> Result<String, JsValue> {
        let engine_config = ra_engine::OptimizerConfig {
            node_limit: self.inner.config.node_limit,
            iter_limit: self.inner.config.max_iterations,
            time_limit_secs: self.inner.config.time_limit_ms / 1000,
        };
        let mut optimizer =
            ra_engine::Optimizer::with_config(engine_config);

        if let Some(ref profile) = self.inner.hardware_profile {
            optimizer.set_hardware_profile(profile.clone());
        }

        for (table, stats) in &self.inner.table_stats {
            optimizer.add_table_stats(table.clone(), stats.clone());
        }

        let start = js_sys::Date::now();

        let (optimized, egraph) = optimizer
            .optimize_with_egraph(plan)
            .map_err(|e| JsValue::from_str(
                &format!("Optimization error: {e}"),
            ))?;

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let time_ms = (js_sys::Date::now() - start) as u64;
        let egraph_nodes = egraph.total_number_of_nodes();

        let original_cost = estimate_cost(plan);
        let optimized_cost = estimate_cost(&optimized);
        let improvement = if original_cost > 0.0 {
            (original_cost - optimized_cost) / original_cost
        } else {
            0.0
        };

        let hw_name = self.inner.hardware_profile
            .as_ref()
            .map_or_else(
                || "auto-detect".to_owned(),
                |p| p.name.clone(),
            );

        let result = OptimizationResult {
            original_plan: plan_to_json(plan),
            optimized_plan: plan_to_json(&optimized),
            original_cost,
            optimized_cost,
            original_cost_breakdown: estimate_cost_breakdown(plan),
            optimized_cost_breakdown: estimate_cost_breakdown(&optimized),
            improvement,
            iterations: self.inner.config.max_iterations,
            egraph_nodes,
            time_ms,
            applied_rules: vec![],
            hardware_profile_name: hw_name,
        };

        serde_json::to_string(&result)
            .map_err(|e| JsValue::from_str(
                &format!("Serialization error: {e}"),
            ))
    }

    /// Get optimizer version.
    #[wasm_bindgen(js_name = getVersion)]
    pub fn get_version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

/// Convert `RelExpr` to JSON for visualization.
fn plan_to_json(plan: &RelExpr) -> serde_json::Value {
    match serde_json::to_value(plan) {
        Ok(v) => v,
        Err(_) => serde_json::json!({
            "error": "Failed to serialize plan"
        }),
    }
}

/// Estimate cost breakdown for a plan.
fn estimate_cost_breakdown(plan: &RelExpr) -> CostBreakdownJs {
    let total = estimate_cost(plan);
    let (cpu_frac, io_frac, mem_frac) = match plan {
        RelExpr::Scan { .. } => (0.1, 0.8, 0.1),
        RelExpr::Filter { .. } => (0.7, 0.2, 0.1),
        RelExpr::Join { .. } => (0.4, 0.2, 0.4),
        RelExpr::Aggregate { .. } => (0.5, 0.1, 0.4),
        RelExpr::Sort { .. } => (0.6, 0.1, 0.3),
        _ => (0.5, 0.3, 0.2),
    };
    CostBreakdownJs {
        cpu: total * cpu_frac,
        io: total * io_frac,
        memory: total * mem_frac,
        network: 0.0,
        total,
    }
}

/// Recursive cost estimation.
fn estimate_cost(plan: &RelExpr) -> f64 {
    match plan {
        RelExpr::Scan { .. } => 100.0,
        RelExpr::Filter { input, .. } => estimate_cost(input) + 10.0,
        RelExpr::Project { input, .. } => estimate_cost(input) + 5.0,
        RelExpr::Join { left, right, .. } => {
            estimate_cost(left) * estimate_cost(right) * 0.1
        }
        RelExpr::Aggregate { input, .. } => estimate_cost(input) * 2.0,
        RelExpr::Sort { input, .. } => estimate_cost(input) * 3.0,
        RelExpr::Limit { input, .. } => estimate_cost(input) + 1.0,
        RelExpr::Union { left, right, .. }
        | RelExpr::Intersect { left, right, .. }
        | RelExpr::Except { left, right, .. } => {
            estimate_cost(left) + estimate_cost(right)
        }
        RelExpr::CTE {
            definition, body, ..
        } => estimate_cost(definition) + estimate_cost(body),
        RelExpr::Window { input, .. } => estimate_cost(input) * 2.5,
        RelExpr::Distinct { input, .. } => estimate_cost(input) * 1.5,
        RelExpr::Values { rows, .. } => rows.len() as f64,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn config_serialization_roundtrip() {
        let config = OptimizerConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: OptimizerConfig =
            serde_json::from_str(&json).unwrap();
        assert_eq!(config.max_iterations, parsed.max_iterations);
    }

    #[test]
    fn table_stats_serialization() {
        let stats = TableStatsJs {
            table: "orders".into(),
            row_count: 1_000_000.0,
            avg_row_size: 128,
            distinct_count: 50_000,
            null_fraction: 0.01,
        };
        let json = serde_json::to_string(&stats).unwrap();
        let parsed: TableStatsJs =
            serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.table, "orders");
    }

    #[test]
    fn cost_breakdown_structure() {
        let plan = RelExpr::scan("test");
        let breakdown = estimate_cost_breakdown(&plan);
        let sum = breakdown.cpu + breakdown.io + breakdown.memory
            + breakdown.network;
        assert!((sum - breakdown.total).abs() < 0.001);
    }

    #[test]
    fn cost_estimation_filter() {
        let plan = RelExpr::scan("users");
        let base = estimate_cost(&plan);
        let filtered = RelExpr::Filter {
            predicate: ra_core::expr::Expr::Const(
                ra_core::expr::Const::Bool(true),
            ),
            input: Box::new(plan),
        };
        assert!(estimate_cost(&filtered) > base);
    }

    #[test]
    fn hardware_profile_roundtrip() {
        let profile = HardwareProfile::gpu_server();
        let json = serde_json::to_string(&profile).unwrap();
        let parsed: HardwareProfile =
            serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, profile.name);
    }
}
