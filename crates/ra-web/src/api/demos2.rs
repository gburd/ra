//! Additional interactive demonstrations (part 2).

use ra_stats::profiles::StatisticsProfile;
use rocket::serde::json::Json;
use rocket::post;
use serde::{Deserialize, Serialize};

/// Demo 5: Index Selection
#[derive(Debug, Serialize, Deserialize)]
pub struct IndexSelectionRequest {
    /// Query selectivity (fraction of rows returned).
    pub selectivity: f64,
    /// Table size in rows.
    pub table_rows: u64,
    /// Available indexes.
    pub available_indexes: Vec<String>,
    /// Index clustering factor (1.0 = perfect, higher = worse).
    pub clustering_factor: f64,
}

#[derive(Debug, Serialize)]
pub struct IndexSelectionResponse {
    /// Selected access method.
    pub access_method: String,
    /// Index used (if any).
    pub index_used: Option<String>,
    /// Estimated cost.
    pub estimated_cost: f64,
    /// Estimated rows accessed.
    pub rows_accessed: u64,
    /// Reasoning.
    pub reasoning: String,
}

#[post("/api/demos/index-selection", data = "<request>")]
pub fn demo_index_selection(
    request: Json<IndexSelectionRequest>,
) -> Json<IndexSelectionResponse> {
    let index_exists = !request.available_indexes.is_empty();
    let output_rows = (request.table_rows as f64 * request.selectivity) as u64;

    let (access_method, index, cost, reasoning) = if request.selectivity > 0.2 {
        // High selectivity - full scan better
        let cost = request.table_rows as f64;
        (
            "Full Table Scan",
            None,
            cost,
            "High selectivity (>20%) makes full scan more efficient than index access",
        )
    } else if index_exists && request.clustering_factor < 5.0 {
        // Low selectivity + good clustering - use index
        let cost = output_rows as f64 * request.clustering_factor;
        (
            "Index Scan",
            Some(request.available_indexes[0].clone()),
            cost,
            "Low selectivity with good clustering factor favors index scan",
        )
    } else if index_exists {
        // Poor clustering - index might still work for very low selectivity
        let cost = output_rows as f64 * request.clustering_factor * 2.0;
        let table_scan_cost = request.table_rows as f64;
        if cost < table_scan_cost {
            (
                "Index Scan",
                Some(request.available_indexes[0].clone()),
                cost,
                "Index scan still cheaper despite poor clustering",
            )
        } else {
            (
                "Full Table Scan",
                None,
                table_scan_cost,
                "Poor clustering makes index scan more expensive than full scan",
            )
        }
    } else {
        // No index available
        let cost = request.table_rows as f64;
        (
            "Full Table Scan",
            None,
            cost,
            "No suitable index available",
        )
    };

    Json(IndexSelectionResponse {
        access_method: access_method.to_string(),
        index_used: index,
        estimated_cost: cost,
        rows_accessed: if access_method == "Index Scan" {
            output_rows
        } else {
            request.table_rows
        },
        reasoning: reasoning.to_string(),
    })
}

/// Demo 6: Subquery Unnesting
#[derive(Debug, Serialize, Deserialize)]
pub struct SubqueryUnnestingRequest {
    /// Subquery type.
    pub subquery_type: String,
    /// Outer table rows.
    pub outer_rows: u64,
    /// Inner table rows.
    pub inner_rows: u64,
    /// Subquery returns multiple rows.
    pub multi_row: bool,
}

#[derive(Debug, Serialize)]
pub struct SubqueryUnnestingResponse {
    /// Original plan.
    pub original_plan: String,
    /// Unnested plan.
    pub unnested_plan: String,
    /// Can be unnested.
    pub can_unnest: bool,
    /// Estimated speedup.
    pub speedup: f64,
    /// Explanation.
    pub explanation: String,
}

#[post("/api/demos/subquery-unnesting", data = "<request>")]
pub fn demo_subquery_unnesting(
    request: Json<SubqueryUnnestingRequest>,
) -> Json<SubqueryUnnestingResponse> {
    let (can_unnest, unnested_plan, speedup, explanation) = match request.subquery_type.as_str() {
        "exists" => {
            let speedup = (request.outer_rows as f64 / 10.0).max(2.0);
            (
                true,
                "Semi Join",
                speedup,
                "EXISTS subquery can be unnested into semi-join",
            )
        }
        "in" if !request.multi_row => {
            let speedup = (request.outer_rows as f64 / 5.0).max(3.0);
            (
                true,
                "Inner Join",
                speedup,
                "IN subquery with single column can be unnested into inner join",
            )
        }
        "scalar" => {
            let speedup = 1.2;
            (
                true,
                "Join with Aggregation",
                speedup,
                "Scalar subquery can be converted to join with aggregation",
            )
        }
        "not_exists" => {
            let speedup = (request.outer_rows as f64 / 8.0).max(2.5);
            (
                true,
                "Anti Join",
                speedup,
                "NOT EXISTS subquery can be unnested into anti-join",
            )
        }
        _ => (
            false,
            "Correlated Subquery",
            1.0,
            "Complex subquery cannot be safely unnested",
        ),
    };

    Json(SubqueryUnnestingResponse {
        original_plan: format!("Nested Loop with {} Subquery", request.subquery_type),
        unnested_plan: unnested_plan.to_string(),
        can_unnest,
        speedup,
        explanation: explanation.to_string(),
    })
}

/// Demo 7: Parallel Query Execution
#[derive(Debug, Serialize, Deserialize)]
pub struct ParallelQueryRequest {
    /// Data size in bytes.
    pub data_size_bytes: u64,
    /// Available cores.
    pub available_cores: u32,
    /// Query complexity (1-10).
    pub complexity: u32,
}

#[derive(Debug, Serialize)]
pub struct ParallelQueryResponse {
    /// Recommended parallelism.
    pub parallel_workers: u32,
    /// Estimated speedup.
    pub speedup: f64,
    /// Coordination overhead (percentage).
    pub coordination_overhead_pct: f64,
    /// Explanation.
    pub explanation: String,
}

#[post("/api/demos/parallel-query", data = "<request>")]
pub fn demo_parallel_query(
    request: Json<ParallelQueryRequest>,
) -> Json<ParallelQueryResponse> {
    let min_size_for_parallel = 10_000_000;

    if request.data_size_bytes < min_size_for_parallel {
        return Json(ParallelQueryResponse {
            parallel_workers: 1,
            speedup: 1.0,
            coordination_overhead_pct: 0.0,
            explanation: "Data size too small for parallel execution".to_string(),
        });
    }

    let ideal_workers = ((request.data_size_bytes / 100_000_000) as u32)
        .max(2)
        .min(request.available_cores);

    let coordination_overhead = match request.complexity {
        1..=3 => 5.0,
        4..=7 => 15.0,
        _ => 25.0,
    };

    let theoretical_speedup = ideal_workers as f64;
    let speedup = theoretical_speedup * (1.0 - coordination_overhead / 100.0);

    let explanation = format!(
        "Using {} workers provides {:.1}x speedup with {:.0}% coordination overhead",
        ideal_workers, speedup, coordination_overhead
    );

    Json(ParallelQueryResponse {
        parallel_workers: ideal_workers,
        speedup,
        coordination_overhead_pct: coordination_overhead,
        explanation,
    })
}

/// Demo 8: GPU Offloading Decision
#[derive(Debug, Serialize, Deserialize)]
pub struct GpuOffloadingRequest {
    /// Operator type.
    pub operator: String,
    /// Data size in bytes.
    pub data_size_bytes: u64,
    /// GPU memory available (bytes).
    pub gpu_memory_bytes: u64,
    /// PCIe bandwidth (GB/s).
    pub pcie_bandwidth_gbps: f64,
}

#[derive(Debug, Serialize)]
pub struct GpuOffloadingResponse {
    /// Use GPU or not.
    pub use_gpu: bool,
    /// Transfer time (ms).
    pub transfer_time_ms: f64,
    /// GPU execution time (ms).
    pub gpu_exec_time_ms: f64,
    /// CPU execution time (ms).
    pub cpu_exec_time_ms: f64,
    /// Recommendation.
    pub recommendation: String,
}

#[post("/api/demos/gpu-offloading", data = "<request>")]
pub fn demo_gpu_offloading(
    request: Json<GpuOffloadingRequest>,
) -> Json<GpuOffloadingResponse> {
    let data_gb = request.data_size_bytes as f64 / 1_000_000_000.0;
    let transfer_time_ms = (data_gb / request.pcie_bandwidth_gbps) * 1000.0 * 2.0; // Round trip

    let (gpu_speedup, min_size) = match request.operator.as_str() {
        "scan" => (5.0, 10_000_000),
        "join" => (8.0, 50_000_000),
        "aggregation" => (6.0, 20_000_000),
        "sort" => (4.0, 30_000_000),
        _ => (3.0, 100_000_000),
    };

    let fits_in_gpu = request.data_size_bytes < request.gpu_memory_bytes;
    let large_enough = request.data_size_bytes > min_size;

    let cpu_exec_time_ms = (request.data_size_bytes / 100_000) as f64;
    let gpu_exec_time_ms = cpu_exec_time_ms / gpu_speedup;
    let total_gpu_time = gpu_exec_time_ms + transfer_time_ms;

    let use_gpu = fits_in_gpu && large_enough && total_gpu_time < cpu_exec_time_ms;

    let recommendation = if !fits_in_gpu {
        "Data doesn't fit in GPU memory".to_string()
    } else if !large_enough {
        "Data too small, transfer overhead dominates".to_string()
    } else if use_gpu {
        format!(
            "GPU execution ({:.1}ms + {:.1}ms transfer) faster than CPU ({:.1}ms)",
            gpu_exec_time_ms, transfer_time_ms, cpu_exec_time_ms
        )
    } else {
        "CPU execution faster due to transfer overhead".to_string()
    };

    Json(GpuOffloadingResponse {
        use_gpu,
        transfer_time_ms,
        gpu_exec_time_ms,
        cpu_exec_time_ms,
        recommendation,
    })
}

/// Demo 9: Distributed Query Planning
#[derive(Debug, Serialize, Deserialize)]
pub struct DistributedQueryRequest {
    /// Number of nodes.
    pub num_nodes: u32,
    /// Data distribution.
    pub distribution: String,
    /// Join type.
    pub join_type: String,
    /// Network bandwidth (GB/s).
    pub network_bandwidth_gbps: f64,
}

#[derive(Debug, Serialize)]
pub struct DistributedQueryResponse {
    /// Chosen strategy.
    pub strategy: String,
    /// Data movement (bytes).
    pub data_movement_bytes: u64,
    /// Estimated time (ms).
    pub estimated_time_ms: f64,
    /// Explanation.
    pub explanation: String,
}

#[post("/api/demos/distributed-query", data = "<request>")]
pub fn demo_distributed_query(
    request: Json<DistributedQueryRequest>,
) -> Json<DistributedQueryResponse> {
    let (strategy, data_movement, time_factor, explanation) =
        match (request.distribution.as_str(), request.join_type.as_str()) {
            ("hash", "hash_join") => (
                "Co-located Join",
                0,
                1.0,
                "Data already hash-partitioned on join key, no shuffling needed",
            ),
            (_, "broadcast_join") => {
                let data_movement = 10_000_000 * (request.num_nodes - 1) as u64;
                (
                    "Broadcast Join",
                    data_movement,
                    1.5,
                    "Small table broadcast to all nodes",
                )
            }
            (_, "hash_join") => {
                let data_movement = 100_000_000 * request.num_nodes as u64;
                (
                    "Shuffle Join",
                    data_movement,
                    2.5,
                    "Both tables need repartitioning on join key",
                )
            }
            _ => (
                "Local Join with Gather",
                50_000_000,
                2.0,
                "Join locally then gather results",
            ),
        };

    let transfer_time_ms = (data_movement as f64 / 1_000_000_000.0)
        / request.network_bandwidth_gbps
        * 1000.0;
    let total_time_ms = transfer_time_ms * time_factor;

    Json(DistributedQueryResponse {
        strategy: strategy.to_string(),
        data_movement_bytes: data_movement,
        estimated_time_ms: total_time_ms,
        explanation: explanation.to_string(),
    })
}

/// Demo 10: Cost Model Calibration
#[derive(Debug, Serialize, Deserialize)]
pub struct CostModelCalibrationRequest {
    /// Statistics profile.
    pub stats_profile: String,
    /// Workload pattern.
    pub workload: String,
    /// Historical accuracy (0.0 to 1.0).
    pub historical_accuracy: f64,
}

#[derive(Debug, Serialize)]
pub struct CostModelCalibrationResponse {
    /// Recommended profile.
    pub recommended_profile: String,
    /// Confidence in estimates.
    pub confidence: f64,
    /// Calibration suggestions.
    pub suggestions: Vec<String>,
}

#[post("/api/demos/cost-calibration", data = "<request>")]
pub fn demo_cost_calibration(
    request: Json<CostModelCalibrationRequest>,
) -> Json<CostModelCalibrationResponse> {
    let current_profile = StatisticsProfile::by_name(&request.stats_profile)
        .unwrap_or_else(|| StatisticsProfile::standard());

    let (recommended_name, confidence, suggestions) = if request.historical_accuracy < 0.5 {
        (
            "RealTime".to_string(),
            0.6,
            vec![
                "Statistics are too stale".to_string(),
                "Enable auto-ANALYZE".to_string(),
                "Reduce statistics gathering threshold".to_string(),
            ],
        )
    } else if request.workload == "oltp" && current_profile.name != "RealTime" {
        (
            "RealTime".to_string(),
            0.8,
            vec![
                "OLTP workload benefits from fresh statistics".to_string(),
                "Consider incremental statistics updates".to_string(),
            ],
        )
    } else if request.workload == "olap" && current_profile.name == "Lazy" {
        (
            "Analytical".to_string(),
            0.85,
            vec![
                "OLAP queries need comprehensive statistics".to_string(),
                "Enable multi-column statistics".to_string(),
            ],
        )
    } else {
        (
            current_profile.name.clone(),
            0.9,
            vec!["Current profile is appropriate".to_string()],
        )
    };

    Json(CostModelCalibrationResponse {
        recommended_profile: recommended_name,
        confidence,
        suggestions,
    })
}
