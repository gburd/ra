//! Vector similarity search cost model.
//!
//! Implements Phase 4 of RFC 0064: dimension-aware cost formulas for:
//! - Vector distance calculations (L2, `InnerProduct`, Cosine)
//! - HNSW index scans (logarithmic complexity)
//! - `IVFFlat` index scans (quantization + probe cost)
//! - Sequential vector scans (linear complexity)
//!
//! Target speedups:
//! - HNSW: 10-100x faster than sequential
//! - `IVFFlat`: 5-50x faster than sequential

use ra_core::cost::Cost;

/// Vector distance metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorMetric {
    /// Euclidean distance (L2).
    L2,
    /// Inner product (dot product).
    InnerProduct,
    /// Cosine similarity.
    Cosine,
}

/// Vector index type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorIndexType {
    /// Hierarchical Navigable Small World graph.
    HNSW,
    /// Inverted File with Flat compression.
    IVFFlat,
    /// No index (sequential scan).
    Sequential,
}

/// Recommended vector index based on workload characteristics.
#[derive(Debug, Clone)]
pub struct VectorIndexRecommendation {
    /// Recommended index type.
    pub index_type: VectorIndexType,
    /// Expected speedup vs sequential scan.
    pub speedup_factor: f64,
    /// Estimated recall at this configuration.
    pub estimated_recall: f64,
    /// Reason for recommendation.
    pub reason: String,
}

/// Cost of computing a single vector distance operation.
///
/// Formulas (per RFC 0064 Phase 4):
/// - L2: dimensions * 0.001
/// - `InnerProduct`: dimensions * 0.001
/// - Cosine: dimensions * 0.0015 (includes normalization)
#[must_use]
pub fn vector_distance_cost(dimensions: usize, metric: VectorMetric) -> f64 {
    let dims = dimensions as f64;
    match metric {
        VectorMetric::L2 | VectorMetric::InnerProduct => dims * 0.001,
        VectorMetric::Cosine => dims * 0.0015, // Higher due to normalization
    }
}

/// Cost of HNSW index search.
///
/// Formula: log2(N) * `ef_search` * `distance_cost`
///
/// Where:
/// - N = `total_vectors`
/// - `ef_search` = search breadth parameter (typically 40-200)
/// - m = max connections per node (affects log base, typically 16)
/// - k = number of nearest neighbors to return
///
/// HNSW provides logarithmic search complexity with high recall.
/// Target: 10-100x speedup vs sequential scan.
#[must_use]
pub fn hnsw_search_cost(
    dimensions: usize,
    m: usize,
    ef_search: usize,
    total_vectors: usize,
    k: usize,
    metric: VectorMetric,
) -> Cost {
    let dist_cost = vector_distance_cost(dimensions, metric);

    // Logarithmic tree traversal
    let log_n = if total_vectors > 1 {
        (total_vectors as f64).log2()
    } else {
        1.0
    };

    // Cost increases with ef_search (exploration breadth)
    let ef = ef_search as f64;

    // Number of distance computations
    let distance_ops = log_n * ef;

    // CPU cost for distance calculations
    let cpu_cost = distance_ops * dist_cost;

    // IO cost for index traversal (read M nodes per level)
    let levels = log_n;
    let nodes_per_level = m.max(1) as f64;
    let io_cost = levels * nodes_per_level * 0.01; // Index page reads

    // Memory for candidate heap (ef_search + k)
    let memory = (ef_search + k) * dimensions * 4; // float32 vectors

    Cost::new(cpu_cost, io_cost, 0.0, memory as u64)
}

/// Cost of `IVFFlat` index search.
///
/// Formula: (`quantization_cost` + probes * `vectors_per_list` * `distance_cost`)
///
/// Where:
/// - lists = number of clusters (typically sqrt(N))
/// - probes = number of clusters to search (typically 1-20)
/// - `total_vectors` / lists = `vectors_per_list`
///
/// `IVFFlat` provides constant-factor speedup with tunable recall.
/// Target: 5-50x speedup vs sequential scan.
#[must_use]
pub fn ivfflat_search_cost(
    dimensions: usize,
    lists: usize,
    probes: usize,
    total_vectors: usize,
    k: usize,
    metric: VectorMetric,
) -> Cost {
    let dist_cost = vector_distance_cost(dimensions, metric);

    // Phase 1: Quantization - find nearest clusters
    let quantization_cost = lists as f64 * dist_cost;

    // Phase 2: Search probed clusters
    let vectors_per_list = if lists > 0 {
        (total_vectors as f64 / lists as f64).ceil()
    } else {
        total_vectors as f64
    };

    let probes_actual = probes.min(lists);
    let search_cost = probes_actual as f64 * vectors_per_list * dist_cost;

    let cpu_cost = quantization_cost + search_cost;

    // IO cost for reading cluster centroids and probed vectors
    let centroid_io = lists as f64 * 0.01;
    let vector_io = probes_actual as f64 * vectors_per_list * 0.005;
    let io_cost = centroid_io + vector_io;

    // Memory for centroids + candidate heap
    let centroid_memory = lists * dimensions * 4;
    let heap_memory = k * dimensions * 4;
    let memory = centroid_memory + heap_memory;

    Cost::new(cpu_cost, io_cost, 0.0, memory as u64)
}

/// Cost of sequential vector scan (no index).
///
/// Formula: `total_vectors` * `distance_cost`
///
/// This is the baseline for comparison.
#[must_use]
pub fn vector_sequential_scan_cost(
    dimensions: usize,
    total_vectors: usize,
    metric: VectorMetric,
) -> Cost {
    let dist_cost = vector_distance_cost(dimensions, metric);
    let cpu_cost = total_vectors as f64 * dist_cost;

    // IO cost for scanning all vectors (assuming 4 bytes per dimension)
    let bytes_per_vector = dimensions * 4;
    let total_bytes = total_vectors * bytes_per_vector;
    let io_cost = total_bytes as f64 / (1024.0 * 1024.0); // Convert to MB units

    // Minimal memory (just current vector)
    let memory = dimensions * 4;

    Cost::new(cpu_cost, io_cost, 0.0, memory as u64)
}

/// Select the best vector index type for a given workload.
///
/// Decision factors:
/// - `total_vectors`: Large datasets favor HNSW (>100K), medium favor `IVFFlat` (10K-100K)
/// - dimensions: High dimensions (>100) favor HNSW
/// - `query_frequency`: High frequency favors HNSW (better amortization of build cost)
/// - `recall_requirement`: High recall (>0.95) favors HNSW
///
/// Returns recommendation with expected speedup and recall.
#[must_use]
pub fn select_vector_index_type(
    total_vectors: usize,
    dimensions: usize,
    query_frequency: QueryFrequency,
    recall_requirement: f64,
) -> VectorIndexRecommendation {
    // Very small datasets don't benefit from indexing
    if total_vectors < 1000 {
        return VectorIndexRecommendation {
            index_type: VectorIndexType::Sequential,
            speedup_factor: 1.0,
            estimated_recall: 1.0,
            reason: "Dataset too small to benefit from indexing (<1K vectors)".to_string(),
        };
    }

    // High recall requirement strongly favors HNSW
    if recall_requirement > 0.95 {
        let speedup = if total_vectors > 100_000 {
            80.0 // Large dataset, excellent speedup
        } else if total_vectors > 10_000 {
            40.0 // Medium dataset, good speedup
        } else {
            15.0 // Small dataset, modest speedup
        };

        return VectorIndexRecommendation {
            index_type: VectorIndexType::HNSW,
            speedup_factor: speedup,
            estimated_recall: 0.98,
            reason: format!(
                "High recall requirement ({recall_requirement:.2}) favors HNSW with ef_search=80"
            ),
        };
    }

    // Large datasets with high query frequency -> HNSW
    if total_vectors > 100_000 && query_frequency == QueryFrequency::High {
        return VectorIndexRecommendation {
            index_type: VectorIndexType::HNSW,
            speedup_factor: 60.0,
            estimated_recall: 0.95,
            reason: "Large dataset (>100K) with high query frequency".to_string(),
        };
    }

    // High dimensions favor HNSW (better dimensional scaling)
    if dimensions > 100 && total_vectors > 10_000 {
        return VectorIndexRecommendation {
            index_type: VectorIndexType::HNSW,
            speedup_factor: 45.0,
            estimated_recall: 0.94,
            reason: format!("High dimensionality ({dimensions}) with significant dataset size"),
        };
    }

    // Medium datasets with moderate recall -> IVFFlat
    if total_vectors > 10_000 && recall_requirement > 0.85 {
        let speedup = if total_vectors > 50_000 { 30.0 } else { 20.0 };

        return VectorIndexRecommendation {
            index_type: VectorIndexType::IVFFlat,
            speedup_factor: speedup,
            estimated_recall: 0.90,
            reason: "Medium dataset with acceptable recall tradeoff".to_string(),
        };
    }

    // Small to medium datasets with low query frequency -> IVFFlat
    if total_vectors > 5_000 && query_frequency == QueryFrequency::Low {
        return VectorIndexRecommendation {
            index_type: VectorIndexType::IVFFlat,
            speedup_factor: 12.0,
            estimated_recall: 0.88,
            reason: "Low query frequency favors simpler IVFFlat index".to_string(),
        };
    }

    // Default: Sequential scan for everything else
    VectorIndexRecommendation {
        index_type: VectorIndexType::Sequential,
        speedup_factor: 1.0,
        estimated_recall: 1.0,
        reason: "Dataset characteristics don't justify indexing overhead".to_string(),
    }
}

/// Query frequency classification for index selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryFrequency {
    /// Low frequency (< 10 queries/sec).
    Low,
    /// Medium frequency (10-100 queries/sec).
    Medium,
    /// High frequency (> 100 queries/sec).
    High,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_distance_cost_scales_with_dimensions() {
        let cost_100 = vector_distance_cost(100, VectorMetric::L2);
        let cost_1000 = vector_distance_cost(1000, VectorMetric::L2);
        assert!((cost_1000 / cost_100 - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cosine_more_expensive_than_l2() {
        let l2_cost = vector_distance_cost(100, VectorMetric::L2);
        let cosine_cost = vector_distance_cost(100, VectorMetric::Cosine);
        assert!(cosine_cost > l2_cost);
        assert!((cosine_cost / l2_cost - 1.5).abs() < 1e-10);
    }

    #[test]
    fn hnsw_logarithmic_scaling() {
        let cost_small = hnsw_search_cost(128, 16, 40, 10_000, 10, VectorMetric::L2);
        let cost_large = hnsw_search_cost(128, 16, 40, 100_000, 10, VectorMetric::L2);

        // Cost should scale roughly logarithmically
        // log2(100k) / log2(10k) ≈ 1.26
        let ratio = cost_large.cpu / cost_small.cpu;
        assert!(ratio > 1.0 && ratio < 2.0, "Ratio: {ratio}");
    }

    #[test]
    fn hnsw_faster_than_sequential_large_dataset() {
        let seq_cost = vector_sequential_scan_cost(128, 100_000, VectorMetric::L2);
        let hnsw_cost = hnsw_search_cost(128, 16, 40, 100_000, 10, VectorMetric::L2);

        let speedup = seq_cost.total() / hnsw_cost.total();
        assert!(speedup >= 10.0, "HNSW speedup {speedup} below target 10x");
    }

    #[test]
    fn ivfflat_linear_with_probes() {
        let cost_1_probe = ivfflat_search_cost(128, 100, 1, 10_000, 10, VectorMetric::L2);
        let cost_10_probes = ivfflat_search_cost(128, 100, 10, 10_000, 10, VectorMetric::L2);

        // Cost should scale roughly linearly with probes
        let ratio = cost_10_probes.cpu / cost_1_probe.cpu;
        assert!(ratio > 5.0 && ratio < 15.0, "Ratio: {ratio}");
    }

    #[test]
    fn ivfflat_faster_than_sequential_medium_dataset() {
        let seq_cost = vector_sequential_scan_cost(128, 50_000, VectorMetric::L2);
        let ivfflat_cost = ivfflat_search_cost(128, 200, 10, 50_000, 10, VectorMetric::L2);

        let speedup = seq_cost.total() / ivfflat_cost.total();
        assert!(speedup >= 5.0, "IVFFlat speedup {speedup} below target 5x");
    }

    #[test]
    fn select_index_small_dataset_prefers_sequential() {
        let rec = select_vector_index_type(500, 128, QueryFrequency::High, 0.99);
        assert_eq!(rec.index_type, VectorIndexType::Sequential);
    }

    #[test]
    fn select_index_large_dataset_high_recall_prefers_hnsw() {
        let rec = select_vector_index_type(200_000, 128, QueryFrequency::High, 0.98);
        assert_eq!(rec.index_type, VectorIndexType::HNSW);
        assert!(rec.speedup_factor >= 10.0);
    }

    #[test]
    fn select_index_medium_dataset_moderate_recall_prefers_ivfflat() {
        let rec = select_vector_index_type(30_000, 64, QueryFrequency::Medium, 0.90);
        assert_eq!(rec.index_type, VectorIndexType::IVFFlat);
        assert!(rec.speedup_factor >= 5.0);
    }

    #[test]
    fn select_index_high_dimensions_favors_hnsw() {
        let rec = select_vector_index_type(50_000, 256, QueryFrequency::Medium, 0.92);
        assert_eq!(rec.index_type, VectorIndexType::HNSW);
    }

    #[test]
    fn sequential_scan_memory_minimal() {
        let cost = vector_sequential_scan_cost(128, 10_000, VectorMetric::L2);
        // Should only store one vector at a time
        assert_eq!(cost.memory, 128 * 4);
    }

    #[test]
    fn hnsw_memory_for_candidates() {
        let ef_search = 40;
        let k = 10;
        let dimensions = 128;
        let cost = hnsw_search_cost(dimensions, 16, ef_search, 10_000, k, VectorMetric::L2);

        let expected_memory = (ef_search + k) * dimensions * 4;
        assert_eq!(cost.memory, expected_memory as u64);
    }

    #[test]
    fn ivfflat_memory_for_centroids_and_heap() {
        let lists = 100;
        let k = 10;
        let dimensions = 128;
        let cost = ivfflat_search_cost(dimensions, lists, 5, 10_000, k, VectorMetric::L2);

        let expected = (lists * dimensions * 4) + (k * dimensions * 4);
        assert_eq!(cost.memory, expected as u64);
    }
}
