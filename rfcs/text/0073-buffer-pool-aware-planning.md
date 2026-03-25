# RFC 0073: Buffer Pool-Aware Planning

- **Status**: Proposed
- **Priority**: Quick Win (1-2 months)
- **Impact**: 30-50% improvement under memory contention
- **Category**: Cost Model / Cache-Aware
- **Created**: 2026-03-25

## Summary

Prefer index scans for hot tables (already in buffer pool) and sequential scans for cold tables. Addresses the problem that cost models assume all I/O hits disk, ignoring buffer pool state.

## Motivation

**Current cost model**: Assumes uniform I/O cost
- Sequential scan: `pages × seq_io_cost`
- Index scan: `rows × random_io_cost`

**Reality**: Hot tables are cached
- Sequential scan (hot): `pages × 0.01` (memory read)
- Sequential scan (cold): `pages × 1.0` (disk read)
- **100x difference!**

### Evidence

**Smooth Scan** (Boulos et al., SIGMOD 2009):
- Coordinates scans across concurrent queries
- Shares buffer pool pages
- Result: 30-50% I/O reduction under contention

## Proposal

### Buffer Pool Statistics

```rust
pub struct BufferPoolStats {
    pub total_pages: u64,
    pub used_pages: u64,
    pub table_cache_hits: HashMap<TableId, f64>,  // Hit rate per table
}

impl BufferPoolStats {
    pub fn cache_hit_rate(&self, table: &Table) -> f64 {
        self.table_cache_hits.get(&table.id).copied().unwrap_or(0.1)
    }
}
```

### Cost Model Integration

```rust
fn scan_cost(&self, table: &Table) -> f64 {
    let cache_hit_rate = self.buffer_pool.cache_hit_rate(table);
    let pages = table.page_count;

    let memory_cost = pages as f64 * self.memory_read_cost;
    let disk_cost = pages as f64 * self.sequential_io_cost;

    cache_hit_rate * memory_cost + (1.0 - cache_hit_rate) * disk_cost
}
```

### Hot Table Detection

```rust
fn is_hot_table(&self, table: &Table) -> bool {
    let cache_hit_rate = self.buffer_pool.cache_hit_rate(table);
    cache_hit_rate > 0.8  // > 80% cached
}
```

### Plan Adjustment

```rust
fn choose_scan_method(&self, table: &Table) -> ScanMethod {
    if self.is_hot_table(table) && self.has_suitable_index(table) {
        // Hot table: index scan is fast (no I/O)
        ScanMethod::IndexScan
    } else {
        // Cold table: sequential scan (amortize I/O)
        ScanMethod::SequentialScan
    }
}
```

## Implementation Plan

### Phase 1: Buffer Pool Statistics (Weeks 1-2)
1. Track cache hit rates per table
2. Expose via `BufferPoolStats` API
3. Add tests with synthetic cache states

### Phase 2: Cost Model Update (Weeks 3-4)
1. Update scan cost to use cache hit rates
2. Adjust index scan cost for hot tables
3. Validate: prefer index on hot tables, seq scan on cold

### Phase 3: Integration (Weeks 5-6)
1. Integrate with existing optimizer
2. Run JOB benchmark with hot/cold scenarios
3. Measure: 30-50% improvement under contention

## Expected Impact

**Under memory contention** (multiple concurrent queries):
- 30-50% I/O reduction via buffer pool awareness
- Prefer index scans on hot tables (avoid re-reading)

**No contention**: Minimal change (cache hit rates are uniform)

## Prior Art

- Smooth Scan (Boulos et al., SIGMOD 2009): 30-50% I/O reduction
- Oracle buffer pool statistics: Hot table detection
