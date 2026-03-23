# Rule: Runtime Filter Propagation (Impala)

**Category:** database-specific/impala
**File:** `rules/database-specific/impala/runtime-filter-propagation.rra`

## Metadata

- **ID:** `impala-runtime-filter-propagation`
- **Version:** "1.0.0"
- **Databases:** impala
- **Tags:** database-specific
- **Authors:** "RA Contributors"


# Runtime Filter Propagation (Impala)

## Metadata
- **Rule ID**: `impala-runtime-filter-propagation`
- **Category**: Database-Specific / Impala
- **Source**: Apache Impala
- **Docs**: https://impala.apache.org/docs/build/html/topics/impala_runtime_filtering.html

## Description

Impala generates Bloom filters at join build time and broadcasts them to scan nodes across the cluster, dramatically reducing network traffic and scan cost.

**Filter types:**
- Bloom filters (distributed)
- Min-max filters (lightweight)
- IN-list filters (low cardinality)

## Relational Algebra

```
Scan(R) $\bowtie$ Scan(S)
-> Scan_with_runtime_filter(R, bloom(S.key)) $\bowtie$ Scan(S)
  where bloom filter propagated across network
```

## Implementation Pattern

```cpp
// Impala RuntimeFilterBank (simplified)
class RuntimeFilterBank {
    void PublishGlobalFilter(FilterId id, BloomFilter* filter) {
        // Broadcast to all scan nodes
        for (FragmentInstance* instance : scan_instances_) {
            instance->ApplyRuntimeFilter(id, filter);
        }
    }
};

// At scan node
void HdfsScanNode::ApplyRuntimeFilter(FilterId id, BloomFilter* filter) {
    runtime_filters_[id] = filter;
    // Filter applies to Parquet/ORC readers
}
```

## Cost Model

```rust
pub fn cost_runtime_filter(
    probe_rows: u64,
    build_rows: u64,
    selectivity: f64,
    cluster_nodes: usize,
) -> Cost {
    // Build Bloom filter
    let filter_build = Cost::cpu(build_rows * 10);
    let filter_size_bytes = (build_rows as f64 * 0.125) as u64; // ~1 bit per element

    // Broadcast cost
    let broadcast_cost = Cost::network(filter_size_bytes as f64 * cluster_nodes as f64);

    // Scan savings (per node)
    let filtered_rows = (probe_rows as f64 * selectivity) as u64;
    let scan_savings_per_node = Cost::io((probe_rows - filtered_rows) * 100);
    let total_savings = scan_savings_per_node * cluster_nodes as u64;

    total_savings - filter_build - broadcast_cost
}
```

## Test Cases

### Test 1: Star schema join with filters
```sql
-- Fact table: 1B rows across 100 nodes
-- Dimension: 1000 rows with selective filter

SELECT f.*, d.name
FROM fact_table f
JOIN dim_table d ON f.dim_id = d.id
WHERE d.region = 'US';  -- Filters to 100 rows

-- Optimization:
-- 1. Build Bloom filter from 100 US dimension IDs
-- 2. Broadcast 1KB filter to all 100 nodes
-- 3. Each node filters 10M fact rows -> ~100K matching
-- 4. Network traffic: 1B rows -> 10M rows (99% reduction)
```

### Test 2: Multiple runtime filters
```sql
SELECT *
FROM lineitem l
JOIN orders o ON l.order_id = o.id
JOIN customer c ON o.customer_id = c.id
WHERE c.nation = 'USA';

-- Runtime filters:
-- 1. customer IDs -> filter orders
-- 2. order IDs -> filter lineitem
-- Cascading filters dramatically reduce scan volume
```

## References

1. **Impala Docs**: "Runtime Filtering for Joins"
   - https://impala.apache.org/docs/build/html/topics/impala_runtime_filtering.html

2. **Cloudera Blog**: "Runtime Filtering in Impala"

## Tags
`database-specific`, `impala`, `runtime-filter`, `bloom-filter`, `distributed`, `mpp`
