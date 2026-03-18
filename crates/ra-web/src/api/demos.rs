//! Interactive demonstration endpoints.
//!
//! Provides 10+ demonstrations showcasing how statistics staleness,
//! hardware profiles, and optimizer decisions interact.

use ra_hardware::HardwareProfile;
use ra_stats::{Staleness, StatisticsState, StatisticsSource};
use rocket::serde::json::Json;
use rocket::{get, post};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// Shared demo state for persistence across requests.
pub type DemoState = Arc<Mutex<DemoStore>>;

/// Store for demo configurations and results.
///
/// Used by save/load configuration routes (not yet wired).
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct DemoStore {
    configurations: std::collections::HashMap<String, DemoConfiguration>,
}

#[allow(dead_code)]
impl DemoStore {
    /// Create new demo store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Save a configuration.
    pub fn save(&mut self, id: String, config: DemoConfiguration) {
        self.configurations.insert(id, config);
    }

    /// Get a configuration.
    pub fn get(&self, id: &str) -> Option<&DemoConfiguration> {
        self.configurations.get(id)
    }
}

/// Demo configuration that can be saved/loaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemoConfiguration {
    /// Demo type.
    pub demo_type: String,
    /// Configuration parameters.
    pub parameters: serde_json::Value,
    /// Timestamp when saved.
    pub saved_at: i64,
}

/// Demo 1: Statistics Staleness Impact
#[derive(Debug, Serialize, Deserialize)]
pub struct StalenessImpactRequest {
    /// Number of rows at statistics gathering.
    pub initial_rows: u64,
    /// Number of modifications since.
    pub modifications: u64,
    /// Statistics source.
    pub source: String,
}

#[derive(Debug, Serialize)]
pub struct StalenessImpactResponse {
    /// Current staleness level.
    pub staleness: String,
    /// Confidence score.
    pub confidence: f64,
    /// Should refresh statistics.
    pub should_refresh: bool,
    /// Plan quality impact (0.0 to 1.0).
    pub plan_quality: f64,
    /// Estimated cardinality error (percentage).
    pub cardinality_error_pct: f64,
    /// Recommendation.
    pub recommendation: String,
}

#[post("/api/demos/staleness-impact", data = "<request>")]
pub fn demo_staleness_impact(
    request: Json<StalenessImpactRequest>,
) -> Json<StalenessImpactResponse> {
    let source = match request.source.as_str() {
        "exact" => StatisticsSource::ExactCount,
        "sampled_10" => StatisticsSource::Sampled { sample_rate: 10 },
        "sampled_50" => StatisticsSource::Sampled { sample_rate: 50 },
        "histogram" => StatisticsSource::Histogram,
        "ml_model" => StatisticsSource::MlModel {
            model_name: "default".to_string(),
        },
        _ => StatisticsSource::Default,
    };

    let mut state = StatisticsState::new(source, request.initial_rows);
    state.record_modifications(request.modifications);

    let staleness = state.staleness();
    let confidence = state.confidence;

    let staleness_str = match staleness {
        Staleness::Fresh => "Fresh",
        Staleness::SlightlyStale => "Slightly Stale",
        Staleness::ModeratelyStale => "Moderately Stale",
        Staleness::VeryStale => "Very Stale",
        Staleness::Unknown => "Unknown",
    };

    let change_rate = request.modifications as f64 / request.initial_rows as f64;
    let cardinality_error_pct = change_rate * 100.0 * (1.0 - confidence);
    let plan_quality = 1.0 - (change_rate * 0.5);

    let recommendation = if staleness >= Staleness::ModeratelyStale {
        "Run ANALYZE to refresh statistics".to_string()
    } else if staleness == Staleness::SlightlyStale {
        "Consider scheduling ANALYZE during low-traffic period".to_string()
    } else {
        "Statistics are fresh, no action needed".to_string()
    };

    Json(StalenessImpactResponse {
        staleness: staleness_str.to_string(),
        confidence,
        should_refresh: staleness >= Staleness::ModeratelyStale,
        plan_quality,
        cardinality_error_pct,
        recommendation,
    })
}

/// Demo 2: Hardware-Specific Plans
#[derive(Debug, Serialize, Deserialize)]
pub struct HardwarePlanRequest {
    /// Query workload type.
    pub workload: String,
    /// Data size in bytes.
    pub data_size_bytes: u64,
    /// Available hardware profile.
    pub hardware_profile: String,
}

#[derive(Debug, Serialize)]
pub struct HardwarePlanResponse {
    /// Selected device for execution.
    pub selected_device: String,
    /// Estimated speedup vs CPU.
    pub speedup: f64,
    /// Estimated execution time (ms).
    pub estimated_time_ms: u64,
    /// Memory required (bytes).
    pub memory_required: u64,
    /// Recommended operator placement.
    pub operator_placement: Vec<OperatorPlacement>,
    /// Explanation.
    pub explanation: String,
}

#[derive(Debug, Serialize)]
pub struct OperatorPlacement {
    /// Operator name.
    pub operator: String,
    /// Device assigned.
    pub device: String,
    /// Reason for placement.
    pub reason: String,
}

#[post("/api/demos/hardware-plan", data = "<request>")]
pub fn demo_hardware_plan(
    request: Json<HardwarePlanRequest>,
) -> Json<HardwarePlanResponse> {
    let profile = match request.hardware_profile.as_str() {
        "gpu_server" => HardwareProfile::gpu_server(),
        "fpga_appliance" => HardwareProfile::fpga_appliance(),
        "cpu_only" => HardwareProfile::cpu_only(),
        _ => HardwareProfile::cpu_only(),
    };

    let (selected_device, speedup, placement, explanation) = match request.workload.as_str() {
        "scan" if profile.gpu_available => {
            let speedup = 5.0;
            let placement = vec![OperatorPlacement {
                operator: "Scan".to_string(),
                device: "GPU".to_string(),
                reason: "Large scan benefits from GPU parallelism".to_string(),
            }];
            ("GPU", speedup, placement, "GPU scan provides 5x speedup for large table scans")
        }
        "join" if profile.gpu_available && request.data_size_bytes > 1_000_000 => {
            let speedup = 8.0;
            let placement = vec![OperatorPlacement {
                operator: "HashJoin".to_string(),
                device: "GPU".to_string(),
                reason: "Hash join with large tables benefits from GPU memory bandwidth".to_string(),
            }];
            ("GPU", speedup, placement, "GPU hash join provides 8x speedup for large joins")
        }
        "aggregation" if profile.gpu_available => {
            let speedup = 6.0;
            let placement = vec![OperatorPlacement {
                operator: "Aggregation".to_string(),
                device: "GPU".to_string(),
                reason: "High-cardinality aggregation benefits from GPU parallelism".to_string(),
            }];
            ("GPU", speedup, placement, "GPU aggregation provides 6x speedup")
        }
        "filter" if profile.fpga_available => {
            let speedup = 3.0;
            let placement = vec![OperatorPlacement {
                operator: "Filter".to_string(),
                device: "FPGA".to_string(),
                reason: "Streaming filter benefits from FPGA pipeline".to_string(),
            }];
            ("FPGA", speedup, placement, "FPGA filter provides 3x speedup with streaming")
        }
        _ => {
            let speedup = 1.0;
            let placement = vec![OperatorPlacement {
                operator: "Generic".to_string(),
                device: "CPU".to_string(),
                reason: "Fallback to CPU for small data or unavailable accelerators".to_string(),
            }];
            ("CPU", speedup, placement, "CPU execution for general workloads")
        }
    };

    let base_time_ms = (request.data_size_bytes / 1_000_000).max(10);
    let estimated_time_ms = (base_time_ms as f64 / speedup) as u64;
    let memory_required = request.data_size_bytes / 2;

    Json(HardwarePlanResponse {
        selected_device: selected_device.to_string(),
        speedup,
        estimated_time_ms,
        memory_required,
        operator_placement: placement,
        explanation: explanation.to_string(),
    })
}

/// Demo 3: Join Algorithm Selection
#[derive(Debug, Serialize, Deserialize)]
pub struct JoinAlgorithmRequest {
    /// Left table size (rows).
    pub left_size: u64,
    /// Right table size (rows).
    pub right_size: u64,
    /// Selectivity (0.0 to 1.0).
    pub selectivity: f64,
    /// Available memory (bytes).
    pub memory_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct JoinAlgorithmResponse {
    /// Selected join algorithm.
    pub algorithm: String,
    /// Estimated cost.
    pub estimated_cost: f64,
    /// Estimated output rows.
    pub output_rows: u64,
    /// Memory usage (bytes).
    pub memory_usage: u64,
    /// Why this algorithm was chosen.
    pub reasoning: String,
    /// Alternatives considered.
    pub alternatives: Vec<JoinAlternative>,
}

#[derive(Debug, Serialize)]
pub struct JoinAlternative {
    /// Algorithm name.
    pub algorithm: String,
    /// Cost.
    pub cost: f64,
    /// Why not chosen.
    pub reason: String,
}

#[post("/api/demos/join-algorithm", data = "<request>")]
pub fn demo_join_algorithm(
    request: Json<JoinAlgorithmRequest>,
) -> Json<JoinAlgorithmResponse> {
    let output_rows =
        (request.left_size as f64 * request.right_size as f64 * request.selectivity) as u64;

    let smaller_table = request.left_size.min(request.right_size);
    let hash_table_size = smaller_table * 100;

    let (algorithm, cost, memory_usage, reasoning, alternatives) =
        if hash_table_size < request.memory_bytes {
            // Hash join fits in memory
            let cost = (request.left_size + request.right_size) as f64;
            let alternatives = vec![
                JoinAlternative {
                    algorithm: "Nested Loop".to_string(),
                    cost: (request.left_size * request.right_size) as f64,
                    reason: "Too expensive for large tables".to_string(),
                },
                JoinAlternative {
                    algorithm: "Sort-Merge".to_string(),
                    cost: (request.left_size as f64 + request.right_size as f64)
                        * (request.left_size as f64).log2(),
                    reason: "Hash join is faster when it fits in memory".to_string(),
                },
            ];
            (
                "Hash Join",
                cost,
                hash_table_size,
                "Build phase fits in memory, optimal for equi-join",
                alternatives,
            )
        } else if request.left_size < 10_000 || request.right_size < 10_000 {
            // Small table, nested loop acceptable
            let cost = (request.left_size * request.right_size) as f64 * 0.01;
            let alternatives = vec![JoinAlternative {
                algorithm: "Hash Join".to_string(),
                cost: (request.left_size + request.right_size) as f64 * 2.0,
                reason: "Would require disk spilling".to_string(),
            }];
            (
                "Nested Loop",
                cost,
                1024,
                "Small table makes nested loop acceptable",
                alternatives,
            )
        } else {
            // Fall back to sort-merge
            let cost = (request.left_size as f64 + request.right_size as f64)
                * (request.left_size as f64).log2();
            let alternatives = vec![JoinAlternative {
                algorithm: "Hash Join".to_string(),
                cost: (request.left_size + request.right_size) as f64 * 3.0,
                reason: "Would require multiple spill passes".to_string(),
            }];
            (
                "Sort-Merge Join",
                cost,
                request.memory_bytes / 2,
                "Large tables with insufficient memory for hash join",
                alternatives,
            )
        };

    Json(JoinAlgorithmResponse {
        algorithm: algorithm.to_string(),
        estimated_cost: cost,
        output_rows,
        memory_usage,
        reasoning: reasoning.to_string(),
        alternatives,
    })
}

/// Demo 4: Aggregation Strategy Selection
#[derive(Debug, Serialize, Deserialize)]
pub struct AggregationStrategyRequest {
    /// Input rows.
    pub input_rows: u64,
    /// Number of distinct groups.
    pub num_groups: u64,
    /// Available memory (bytes).
    pub memory_bytes: u64,
    /// Parallel workers available.
    pub workers: u32,
}

#[derive(Debug, Serialize)]
pub struct AggregationStrategyResponse {
    /// Selected strategy.
    pub strategy: String,
    /// Estimated time (ms).
    pub estimated_time_ms: u64,
    /// Memory usage (bytes).
    pub memory_usage: u64,
    /// Parallelism used.
    pub parallelism: u32,
    /// Reasoning.
    pub reasoning: String,
}

#[post("/api/demos/aggregation-strategy", data = "<request>")]
pub fn demo_aggregation_strategy(
    request: Json<AggregationStrategyRequest>,
) -> Json<AggregationStrategyResponse> {
    let hash_table_size = request.num_groups * 200;
    let can_fit_memory = hash_table_size < request.memory_bytes;

    let (strategy, time_ms, memory, parallelism, reasoning) = if can_fit_memory
        && request.num_groups < 1_000_000
    {
        let time_ms = (request.input_rows / 1_000_000).max(10);
        (
            "Hash Aggregation",
            time_ms,
            hash_table_size,
            request.workers.min(8),
            "Hash table fits in memory, supports parallel execution",
        )
    } else if request.num_groups < 100 {
        let time_ms = (request.input_rows / 500_000).max(10);
        (
            "Streaming Aggregation",
            time_ms,
            1024 * 1024,
            1,
            "Few groups, streaming is efficient",
        )
    } else {
        let time_ms = (request.input_rows / 200_000).max(50);
        (
            "Sort-Based Aggregation",
            time_ms,
            request.memory_bytes / 2,
            request.workers.min(4),
            "Large number of groups requires sort-based approach",
        )
    };

    Json(AggregationStrategyResponse {
        strategy: strategy.to_string(),
        estimated_time_ms: time_ms,
        memory_usage: memory,
        parallelism,
        reasoning: reasoning.to_string(),
    })
}

/// List all available demos.
#[derive(Debug, Serialize)]
pub struct DemoListResponse {
    /// Number of demos.
    pub count: usize,
    /// Demo metadata.
    pub demos: Vec<DemoMetadata>,
}

#[derive(Debug, Serialize)]
pub struct DemoMetadata {
    /// Demo ID.
    pub id: String,
    /// Demo title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Endpoint.
    pub endpoint: String,
    /// Category.
    pub category: String,
}

#[get("/api/demos")]
pub fn list_demos() -> Json<DemoListResponse> {
    let demos = vec![
        DemoMetadata {
            id: "staleness-impact".to_string(),
            title: "Statistics Staleness Impact".to_string(),
            description: "Visualize how stale statistics affect query plan quality and cardinality estimation".to_string(),
            endpoint: "/api/demos/staleness-impact".to_string(),
            category: "Statistics".to_string(),
        },
        DemoMetadata {
            id: "hardware-plan".to_string(),
            title: "Hardware-Specific Plans".to_string(),
            description: "See how GPU/FPGA availability changes operator placement decisions".to_string(),
            endpoint: "/api/demos/hardware-plan".to_string(),
            category: "Hardware".to_string(),
        },
        DemoMetadata {
            id: "join-algorithm".to_string(),
            title: "Join Algorithm Selection".to_string(),
            description: "Compare hash join vs nested loop vs sort-merge based on data size and memory".to_string(),
            endpoint: "/api/demos/join-algorithm".to_string(),
            category: "Algorithms".to_string(),
        },
        DemoMetadata {
            id: "aggregation-strategy".to_string(),
            title: "Aggregation Strategy Selection".to_string(),
            description: "Choose between hash, streaming, and sort-based aggregation".to_string(),
            endpoint: "/api/demos/aggregation-strategy".to_string(),
            category: "Algorithms".to_string(),
        },
        DemoMetadata {
            id: "index-selection".to_string(),
            title: "Index Selection".to_string(),
            description: "Compare index scan vs full table scan based on selectivity and clustering".to_string(),
            endpoint: "/api/demos/index-selection".to_string(),
            category: "Access Methods".to_string(),
        },
        DemoMetadata {
            id: "subquery-unnesting".to_string(),
            title: "Subquery Unnesting".to_string(),
            description: "Transform correlated subqueries into joins for better performance".to_string(),
            endpoint: "/api/demos/subquery-unnesting".to_string(),
            category: "Optimization".to_string(),
        },
        DemoMetadata {
            id: "parallel-query".to_string(),
            title: "Parallel Query Execution".to_string(),
            description: "Determine optimal parallelism level based on data size and complexity".to_string(),
            endpoint: "/api/demos/parallel-query".to_string(),
            category: "Execution".to_string(),
        },
        DemoMetadata {
            id: "gpu-offloading".to_string(),
            title: "GPU Offloading Decision".to_string(),
            description: "Evaluate GPU vs CPU execution considering transfer overhead".to_string(),
            endpoint: "/api/demos/gpu-offloading".to_string(),
            category: "Hardware".to_string(),
        },
        DemoMetadata {
            id: "distributed-query".to_string(),
            title: "Distributed Query Planning".to_string(),
            description: "Choose between co-located, broadcast, and shuffle joins in distributed systems".to_string(),
            endpoint: "/api/demos/distributed-query".to_string(),
            category: "Distributed".to_string(),
        },
        DemoMetadata {
            id: "cost-calibration".to_string(),
            title: "Cost Model Calibration".to_string(),
            description: "Adjust statistics profiles based on workload patterns and accuracy".to_string(),
            endpoint: "/api/demos/cost-calibration".to_string(),
            category: "Statistics".to_string(),
        },
    ];

    Json(DemoListResponse {
        count: demos.len(),
        demos,
    })
}
