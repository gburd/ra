use egg::{Id, Language};

use crate::egraph::RelLang;

/// Cost function for plan extraction from the e-graph.
///
/// Assigns a numeric cost to each node type based on hardware characteristics.
/// Costs are adjusted based on CPU speed, cache size, storage bandwidth,
/// and available SIMD instructions.
#[derive(Debug)]
pub struct RelCostFn {
    hardware: ra_hardware::HardwareProfile,
}

impl RelCostFn {
    /// Create a new cost function with the given hardware profile.
    #[must_use]
    pub fn new(hardware: ra_hardware::HardwareProfile) -> Self {
        Self { hardware }
    }
}

impl egg::CostFunction<RelLang> for RelCostFn {
    type Cost = f64;

    fn cost<C>(&mut self, enode: &RelLang, mut costs: C) -> Self::Cost
    where
        C: FnMut(Id) -> Self::Cost,
    {
        let base_cost = match enode {
            RelLang::Scan([table_id]) => {
                // Scan cost depends on storage bandwidth
                // Higher bandwidth = lower cost
                let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
                return costs(*table_id) + (100.0 * storage_factor);
            }
            RelLang::ScanAlias([table_id, alias_id]) => {
                let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
                return costs(*table_id) + costs(*alias_id) + (100.0 * storage_factor);
            }
            RelLang::IndexOnlyScan([table_id, _index_id, cols_id, pred_id]) => {
                // Index-only scan: O(log n) -- much cheaper than full table scan.
                // Models B-tree traversal to first/last key.
                let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
                return costs(*table_id)
                    + costs(*cols_id)
                    + costs(*pred_id)
                    + (5.0 * storage_factor);
            }
            RelLang::Filter(_) | RelLang::Project(_) => {
                // Filter/project cost depends on SIMD width
                // Wider SIMD = lower per-row cost
                let simd_factor = 256.0 / f64::from(self.hardware.simd_width_bits);
                1.0 * simd_factor
            }
            RelLang::Join(_) => {
                // Join cost depends on cache size and memory bandwidth
                // Larger cache = better hash table performance
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb; // Normalize to 16 MB baseline
                500.0 * cache_factor
            }
            RelLang::Aggregate(_) => {
                // Aggregate cost depends on cache and parallelism
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb;
                200.0 * cache_factor
            }
            RelLang::Sort(_) => {
                // Sort cost depends on CPU cores (parallel sort)
                let parallelism_factor = 8.0 / f64::from(self.hardware.cpu_cores);
                150.0 * parallelism_factor.max(0.5) // Don't over-penalize many-core systems
            }
            RelLang::IncrementalSort(_) => {
                // Incremental sort is cheaper than full sort: only sorts
                // within prefix groups, so cost is proportional to
                // group_size * log(group_size) instead of n * log(n).
                // Model as 40% of full sort cost (conservative estimate
                // assuming moderate prefix selectivity).
                let parallelism_factor = 8.0 / f64::from(self.hardware.cpu_cores);
                60.0 * parallelism_factor.max(0.5)
            }
            RelLang::Limit(_) => 0.5,
            RelLang::Union(_) | RelLang::Intersect(_) | RelLang::Except(_) => 50.0,
            RelLang::RecursiveCTE(_) => 1000.0,
            RelLang::CTE(_) => 10.0,
            RelLang::Window(_) => {
                let parallelism_factor = 8.0 / f64::from(self.hardware.cpu_cores);
                200.0 * parallelism_factor.max(0.5)
            }
            RelLang::DistinctRel(_) => {
                let cache_mb = self.hardware.l3_cache_bytes as f64 / (1024.0 * 1024.0);
                let cache_factor = 16.0 / cache_mb;
                150.0 * cache_factor
            }
            RelLang::Values(_) => 1.0,
            RelLang::MetadataLookup(_) => {
                // O(1) metadata lookup, much cheaper than any scan
                return 1.0;
            }
            RelLang::MvScan(_) => {
                // MV scan reads pre-computed, pre-joined data.
                let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
                return costs(enode.children()[0]) + (15.0 * storage_factor);
            }
            RelLang::BitmapIndexScan(_) => {
                // Bitmap index scan: random IO to build bitmap from index,
                // comparable to a full sequential scan without selectivity info.
                let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
                120.0 * storage_factor
            }
            RelLang::BitmapHeapScan(_) => {
                // Heap access after bitmap: sequential IO at a fraction of full scan
                let storage_factor = 100.0 / self.hardware.storage_bandwidth_gbps;
                50.0 * storage_factor
            }
            RelLang::Cast(_) => {
                // Type casts are typically very cheap (often free at runtime)
                0.01
            }
            _ => 0.1,
        };

        let child_cost: f64 = enode.children().iter().map(|child| costs(*child)).sum();

        base_cost + child_cost
    }
}
