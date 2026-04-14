# RFC 0104: Delta Lake MERGE Optimization

- Start Date: 2026-03-28
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should optimize Delta Lake MERGE operations (UPSERT: INSERT + UPDATE + DELETE in a single atomic transaction) by leveraging Delta Lake's transaction log, file-level statistics, partition pruning, and Z-ORDER clustering. MERGE is Delta Lake's most critical operation for incremental ETL workloads, offering 10-50x speedup over traditional DELETE + INSERT patterns when properly optimized. Ra can improve MERGE performance through intelligent execution strategy selection (hash-based, sort-based, index-based, or partition-aware), aggressive file and partition pruning, predicate pushdown, and parallel per-partition execution.

## Motivation

MERGE INTO is Databricks' #1 priority feature for Delta Lake optimization. It combines INSERT, UPDATE, and DELETE operations into a single atomic transaction, essential for:

**1. Incremental ETL pipelines.** CDC (Change Data Capture) streams require efficient UPSERT operations to keep data warehouses synchronized with transactional databases. Traditional approaches require separate DELETE and INSERT passes, scanning the target table twice.

**2. Data lake deduplication.** MERGE enables efficient deduplication by matching incoming records against existing data based on business keys, updating matches and inserting new records in one pass.

**3. Slowly changing dimensions.** Type 1 and Type 2 SCD patterns require conditional updates based on change detection, naturally expressed via MERGE with multiple WHEN clauses.

**4. Real-time data synchronization.** Streaming data requires continuous UPSERT operations. MERGE with Delta Lake's ACID guarantees enables consistent incremental updates without full table rewrites.

**Traditional Approach Problems:**
```sql
-- Two full table scans, no ACID guarantees
DELETE FROM target WHERE id IN (SELECT id FROM source);
INSERT INTO target SELECT * FROM source;
```
Cost: O(n + m) with 2 scans of target, temporary tables, non-atomic

**MERGE Approach:**
```sql
MERGE INTO target USING source ON target.id = source.id
WHEN MATCHED THEN UPDATE SET *
WHEN NOT MATCHED THEN INSERT *
```
Cost: O(n + m) with 1 scan, atomic transaction, efficient file pruning

**Expected Performance Impact:**

| Pattern | Baseline (DELETE + INSERT) | Optimized MERGE | Speedup |
|---------|---------------------------|-----------------|---------|
| Small incremental update (1% of data) | Full table scan 2x | Partition pruning to 1% | 50-100x |
| Medium update (10% of data) | Full table scan 2x | File pruning to 10-20% | 10-20x |
| Large update (50% of data) | Full table scan 2x | Optimized hash join | 2-5x |
| Deduplication with unique key | Full scan + sort 2x | Hash-based MERGE | 10-30x |
| Partitioned incremental sync | Scan all partitions 2x | Process only affected partitions | 20-100x |

**Real-World Use Cases from Databricks:**

1. **E-commerce order processing**: 1M orders/day, 100K updates to existing orders
   - Without optimization: 2x full scan of 1B row table (8 hours)
   - With partition pruning on date: Scan only today's partition (5 minutes)
   - Speedup: 96x

2. **IoT sensor data**: 10M sensor readings/hour, late-arriving data corrections
   - Without optimization: Full scan for deduplication (15 minutes)
   - With Z-ORDER on sensor_id: File pruning to 0.1% of data (10 seconds)
   - Speedup: 90x

3. **Customer profile updates**: Daily batch of 500K profile changes
   - Without optimization: Full customer table scan (1 hour)
   - With hash-based MERGE: Single-pass hash join (2 minutes)
   - Speedup: 30x

## Detailed Design

### MERGE Syntax and Semantics

Full MERGE syntax with multiple WHEN clauses:

```sql
MERGE INTO target_table AS t
USING source_table AS s
ON t.id = s.id
WHEN MATCHED AND s.status = 'deleted' THEN DELETE
WHEN MATCHED AND s.updated_at &gt; t.updated_at THEN UPDATE SET *
WHEN NOT MATCHED THEN INSERT *
WHEN NOT MATCHED BY SOURCE AND t.last_active &lt; current_date() - INTERVAL 90 DAYS THEN DELETE
```

**Clause Semantics:**

1. **WHEN MATCHED**: Source and target rows match on join condition
   - Actions: UPDATE SET (columns) or DELETE
   - Optional predicate: Additional filter beyond join condition
   - Multiple clauses allowed (first match wins)

2. **WHEN NOT MATCHED**: Source row has no matching target row
   - Actions: INSERT VALUES or INSERT *
   - Optional predicate: Filter which unmatched source rows to insert

3. **WHEN NOT MATCHED BY SOURCE**: Target row has no matching source row
   - Actions: UPDATE SET or DELETE
   - Optional predicate: Filter which unmatched target rows to process
   - Warning: Full target scan if not carefully filtered

**Delta Lake Constraints:**

- Single row match enforcement: Multiple target rows matching same source row triggers error
- Atomic transaction: All operations succeed or all fail
- ACID guarantees: Isolation through MVCC (multi-version concurrency control)
- Schema evolution: Automatic schema merging if enabled

### Execution Strategies

The optimizer must choose between four fundamental execution strategies based on data characteristics, available indexes, partition layout, and cost model estimates.

#### Strategy 1: Hash-Based MERGE

**Description**: Build in-memory hash table from source (smaller side), probe with target rows.

**Algorithm**:
```
1. Build phase: Hash all source rows on join key
2. Probe phase: For each target row:
   - Lookup in hash table
   - If match found: Apply WHEN MATCHED action
   - If no match: Apply WHEN NOT MATCHED BY SOURCE action
3. Insert phase: Insert remaining unmatched source rows (WHEN NOT MATCHED)
```

**Best For**:
- Small source table (fits in memory or can be distributed)
- Non-partitioned tables or partition keys don't match join keys
- Random distribution of join keys

**Cost Model**:
```
build_cost = source_rows * hash_build_per_row  // 0.5 CPU cycles/row
probe_cost = target_rows * hash_probe_per_row  // 0.3 CPU cycles/row
total_cost = build_cost + probe_cost
```

**Optimization Opportunities**:
- Bloom filter acceleration: Build Bloom filter during hash table construction, skip target rows with no possible match
- Partition source table: Distribute hash table across workers
- Perfect hashing: If join key domain is known and small, use perfect hash

#### Strategy 2: Sort-Based MERGE

**Description**: Sort both source and target on join key, perform merge-join.

**Algorithm**:
```
1. Sort source and target on join key
2. Merge-join with three-way scan:
   - source &lt; target: WHEN NOT MATCHED (INSERT)
   - source = target: WHEN MATCHED (UPDATE/DELETE)
   - target &lt; source: WHEN NOT MATCHED BY SOURCE (DELETE/UPDATE)
```

**Best For**:
- Large datasets (both source and target)
- Join keys already sorted or can leverage indexes
- Minimal memory available (streaming merge)
- Duplicate join keys on target side (handled naturally)

**Cost Model**:
```
sort_source_cost = source_rows * log(source_rows) * sort_cost_per_row  // 2.0
sort_target_cost = target_rows * log(target_rows) * sort_cost_per_row  // 2.0
merge_cost = (source_rows + target_rows) * merge_scan_per_row          // 0.2
total_cost = sort_source_cost + sort_target_cost + merge_cost

If already sorted (index available):
total_cost = merge_cost
```

**Optimization Opportunities**:
- External sort: Spill to disk if data doesn't fit in memory
- Index exploitation: Skip sort phase if B-tree or sorted index exists
- Parallel sort: Distribute sort across workers with range partitioning

#### Strategy 3: Index-Based MERGE

**Description**: For each source row, perform index lookup into target table.

**Algorithm**:
```
1. For each source row:
   - Index lookup on join key
   - If match found: Apply WHEN MATCHED action
   - If no match: Buffer for INSERT (WHEN NOT MATCHED)
2. Optionally: Full target scan for WHEN NOT MATCHED BY SOURCE
```

**Best For**:
- Very small source table (&lt; 1% of target)
- Target has efficient index on join key (B-tree, hash index)
- No WHEN NOT MATCHED BY SOURCE clause (avoids full scan)

**Cost Model**:
```
index_lookup_cost = source_rows * index_lookup_per_row  // 5.0 (B-tree: log(n))
update_cost = matched_rows * update_per_row              // 10.0
insert_cost = unmatched_rows * insert_per_row            // 8.0

If WHEN NOT MATCHED BY SOURCE:
  total_cost += target_rows * scan_per_row  // 0.1 (full scan required)
Else:
  total_cost = index_lookup_cost + update_cost + insert_cost
```

**Optimization Opportunities**:
- Batch lookups: Group multiple source rows for single index scan
- Covering index: If index contains all needed columns, avoid table access
- Index-only scan: For WHEN MATCHED DELETE, index lookup sufficient

#### Strategy 4: Partition-Aware MERGE

**Description**: Process partition-by-partition in parallel, leveraging Delta Lake partitioning.

**Algorithm**:
```
1. Partition pruning: Identify affected partitions based on source data
2. For each partition in parallel:
   - Read partition-local source and target data
   - Apply hash or sort-based MERGE within partition
   - Write updated partition files
3. Transaction log: Record all affected files atomically
```

**Best For**:
- Tables partitioned on high-cardinality column (date, region, etc.)
- Source data affects small subset of partitions
- Parallel execution environment (Spark, distributed query engine)

**Cost Model**:
```
affected_partitions = estimate_affected_partitions(source, partition_key)
pruned_target_rows = target_rows * (affected_partitions / total_partitions)

per_partition_cost = (source_rows_in_partition + pruned_target_rows_in_partition) * merge_cost

total_cost = sum(per_partition_cost) / parallelism_factor

Benefit: pruned_target_rows &lt;&lt; target_rows (skip irrelevant partitions)
```

**Optimization Opportunities**:
- Dynamic partition pruning: Compute affected partitions at runtime
- Partition-local optimization: Choose hash vs sort per partition
- Skew handling: Redistribute hot partitions across more workers

### Delta Lake-Specific Optimizations

#### Copy-on-Write Transaction Model

Delta Lake uses copy-on-write semantics: modified files are rewritten entirely, not updated in place.

**Implications for MERGE:**

1. **File-level granularity**: MERGE rewrites all files containing matched rows
2. **File skipping**: Unchanged files are not touched (referenced in transaction log)
3. **Atomic commits**: New files written, then transaction log updated atomically
4. **Isolation**: Concurrent readers see consistent snapshot (MVCC)

**Optimization Strategy:**

```
Minimize files rewritten:
- Use partition pruning to identify affected partitions
- Use file-level statistics (min/max) to skip files with no matches
- Use Z-ORDER clustering to co-locate related records (fewer files affected)
```

**Cost Model Extension:**
```
files_affected = estimate_files_with_matches(source, target_metadata)
file_rewrite_cost = files_affected * avg_file_size * rewrite_cost_per_byte  // 0.001

If Z-ORDERed on join key:
  files_affected_reduction = 5-10x  (co-located data)

total_cost += file_rewrite_cost
```

#### Transaction Log Integration

Delta Lake maintains a JSON-based transaction log (`_delta_log/`) recording all file operations.

**MERGE Transaction Structure:**
```json
{
  "commitInfo": {
    "operation": "MERGE",
    "operationMetrics": {
      "numTargetRowsInserted": 1000,
      "numTargetRowsUpdated": 500,
      "numTargetRowsDeleted": 100,
      "numOutputRows": 1400,
      "numSourceRows": 1200,
      "numTargetFilesAdded": 5,
      "numTargetFilesRemoved": 3
    }
  },
  "remove": [
    {"path": "part-0001.parquet", "dataChange": true},
    {"path": "part-0002.parquet", "dataChange": true}
  ],
  "add": [
    {"path": "part-0100.parquet", "size": 1024000, "stats": "{...}"},
    {"path": "part-0101.parquet", "size": 2048000, "stats": "{...}"}
  ]
}
```

**Optimization Opportunities:**

1. **Pre-commit validation**: Check for conflicts before writing files
2. **Metadata-only queries**: Some MERGE operations can be detected as no-ops using metadata
3. **Statistics collection**: Record min/max/null counts during MERGE for future query optimization

#### File-Level Statistics and Pruning

Delta Lake stores min/max/null-count statistics for each data file.

**Statistics Format (per file, per column):**
```json
{
  "numRecords": 100000,
  "minValues": {"id": 1, "date": "2024-01-01", "amount": 0.01},
  "maxValues": {"id": 150000, "date": "2024-01-31", "amount": 9999.99},
  "nullCount": {"id": 0, "date": 0, "amount": 5}
}
```

**MERGE Pruning Strategy:**

```sql
-- Example: MERGE with filter on join key
MERGE INTO target USING source ON target.id = source.id
WHEN MATCHED THEN UPDATE SET *
```

**Pruning Logic:**
```
1. Compute source key range: min(source.id) = 10000, max(source.id) = 12000
2. For each target file:
   - Skip if file.minValues.id &gt; 12000 (no overlap)
   - Skip if file.maxValues.id &lt; 10000 (no overlap)
   - Process if [file.min, file.max] intersects [10000, 12000]
3. Result: Only process files with potential matches
```

**Expected Pruning Effectiveness:**

| Scenario | Files Scanned | Pruning Ratio |
|----------|---------------|---------------|
| Small incremental update (1% of key range) | 1-5% of files | 95-99% |
| Random keys (no locality) | 80-100% of files | 0-20% |
| Z-ORDERed on join key | 5-15% of files | 85-95% |

**Cost Model:**
```
file_metadata_lookup_cost = num_files * 0.001  // Fast metadata scan
files_processed = num_files * (1 - pruning_ratio)
scan_cost = files_processed * avg_file_size * scan_cost_per_byte  // 0.0001
```

#### Z-ORDER Clustering

Z-ORDER (multi-dimensional clustering) co-locates rows with similar values on multiple columns.

**How Z-ORDER Works:**

1. Interleave bits from multiple column values to create composite sort key
2. Sort data by composite key, grouping similar multi-dimensional values
3. Write data files with improved locality across multiple dimensions

**Example:**
```sql
OPTIMIZE target ZORDER BY (customer_id, product_id)
```

**Impact on MERGE:**

| MERGE Join Key | Z-ORDER Columns | File Pruning Improvement |
|----------------|-----------------|--------------------------|
| customer_id | customer_id | 10-20x fewer files scanned |
| product_id | product_id | 10-20x fewer files scanned |
| customer_id, product_id | customer_id, product_id | 50-100x fewer files scanned |
| order_date | customer_id | Minimal (no match) |

**Cost Model Adjustment:**
```
If join key matches Z-ORDER column:
  pruning_ratio += 0.7  // 70% additional pruning
  files_affected /= 10  // 10x fewer files need rewriting
```

**Recommendation**: Always Z-ORDER on commonly used join keys for MERGE operations.

### Predicate Pushdown Strategies

MERGE statements often include predicates beyond the join condition:

```sql
MERGE INTO target USING source ON target.id = source.id
WHEN MATCHED AND source.updated_at &gt; target.updated_at THEN UPDATE SET *
WHEN MATCHED AND source.status = 'deleted' THEN DELETE
WHEN NOT MATCHED AND source.date &gt;= '2024-01-01' THEN INSERT *
```

**Pushdown Opportunities:**

1. **Source-side pushdown**: `source.date &gt;= '2024-01-01'`
   - Apply filter before MERGE execution
   - Reduces source rows participating in join
   - Standard optimization, always beneficial

2. **Target-side pushdown**: More complex, depends on WHEN clauses
   - If no `WHEN NOT MATCHED BY SOURCE`: Can filter target aggressively
   - If `WHEN NOT MATCHED BY SOURCE` present: Cannot filter (need all rows)

3. **Join-side pushdown**: Use predicates to build runtime filters
   - Build Bloom filter from filtered source rows
   - Apply to target scan to skip rows with no possible match

**Optimization Rules:**

```
Rule 1: Push source predicates into source scan
  Condition: Always (standard optimization)

Rule 2: Push target predicates into target scan
  Condition: No WHEN NOT MATCHED BY SOURCE clause
  Example: target.date &gt;= '2024-01-01' (skip old data)

Rule 3: Build Bloom filter from source join keys
  Condition: Hash-based MERGE strategy
  Benefit: Skip target rows with no source match

Rule 4: Use partition key predicates for dynamic partition pruning
  Condition: Target partitioned on column referenced in predicate
  Example: source.date = target.date -&gt; prune to matching date partitions
```

### Cost Model

Comprehensive cost model comparing MERGE strategies:

```rust
struct MergeCostParams {
    // Data characteristics
    source_rows: u64,
    target_rows: u64,
    source_size_bytes: u64,
    target_size_bytes: u64,
    join_key_selectivity: f64,  // Expected match ratio (0.0-1.0)

    // Storage characteristics
    target_files: u64,
    avg_file_size_bytes: u64,
    partition_count: u64,
    affected_partitions: u64,
    z_order_columns: Vec&lt;String&gt;,  // Empty if not Z-ORDERed

    // MERGE clause flags
    has_when_not_matched_by_source: bool,
    update_columns: usize,  // How many columns updated (vs full row)

    // Hardware parameters
    memory_bytes: u64,
    cpu_cores: u64,
    disk_read_bandwidth_mbps: f64,
    disk_write_bandwidth_mbps: f64,

    // Cost coefficients (tuned per system)
    hash_build_per_row: f64,         // 0.5 microseconds
    hash_probe_per_row: f64,         // 0.3 microseconds
    sort_cost_per_row: f64,          // 2.0 microseconds
    merge_scan_per_row: f64,         // 0.2 microseconds
    index_lookup_per_row: f64,       // 5.0 microseconds (B-tree)
    scan_cost_per_byte: f64,         // 0.0001 microseconds
    rewrite_cost_per_byte: f64,      // 0.001 microseconds
    bloom_filter_build_per_row: f64, // 0.1 microseconds
    bloom_filter_probe_per_row: f64, // 0.05 microseconds
}

fn estimate_hash_merge_cost(params: &MergeCostParams) -&gt; f64 {
    let build_cost = params.source_rows as f64 * params.hash_build_per_row;
    let probe_cost = params.target_rows as f64 * params.hash_probe_per_row;

    let matched_rows = (params.source_rows as f64 * params.join_key_selectivity).min(params.target_rows as f64);
    let files_affected = estimate_files_affected(params, matched_rows);
    let rewrite_cost = files_affected * params.avg_file_size_bytes as f64 * params.rewrite_cost_per_byte;

    build_cost + probe_cost + rewrite_cost
}

fn estimate_sort_merge_cost(params: &MergeCostParams) -&gt; f64 {
    let sort_source = params.source_rows as f64 * params.source_rows.ilog2() as f64 * params.sort_cost_per_row;
    let sort_target = params.target_rows as f64 * params.target_rows.ilog2() as f64 * params.sort_cost_per_row;
    let merge_cost = (params.source_rows + params.target_rows) as f64 * params.merge_scan_per_row;

    let matched_rows = (params.source_rows as f64 * params.join_key_selectivity).min(params.target_rows as f64);
    let files_affected = estimate_files_affected(params, matched_rows);
    let rewrite_cost = files_affected * params.avg_file_size_bytes as f64 * params.rewrite_cost_per_byte;

    sort_source + sort_target + merge_cost + rewrite_cost
}

fn estimate_index_merge_cost(params: &MergeCostParams) -&gt; f64 {
    let lookup_cost = params.source_rows as f64 * params.index_lookup_per_row;

    let matched_rows = (params.source_rows as f64 * params.join_key_selectivity).min(params.target_rows as f64);
    let files_affected = estimate_files_affected(params, matched_rows);
    let rewrite_cost = files_affected * params.avg_file_size_bytes as f64 * params.rewrite_cost_per_byte;

    let mut cost = lookup_cost + rewrite_cost;

    // If WHEN NOT MATCHED BY SOURCE, must scan all target rows
    if params.has_when_not_matched_by_source {
        cost += params.target_rows as f64 * params.scan_cost_per_byte * (params.target_size_bytes as f64 / params.target_rows as f64);
    }

    cost
}

fn estimate_partition_merge_cost(params: &MergeCostParams) -&gt; f64 {
    let pruned_target_rows = params.target_rows * params.affected_partitions / params.partition_count;

    // Choose best strategy per partition (hash or sort)
    let per_partition_params = MergeCostParams {
        target_rows: pruned_target_rows / params.affected_partitions,
        ..params.clone()
    };

    let per_partition_cost = estimate_hash_merge_cost(&per_partition_params)
        .min(estimate_sort_merge_cost(&per_partition_params));

    let parallelism_factor = params.cpu_cores.min(params.affected_partitions) as f64;

    (per_partition_cost * params.affected_partitions as f64) / parallelism_factor
}

fn estimate_files_affected(params: &MergeCostParams, matched_rows: f64) -&gt; f64 {
    let base_files = (matched_rows / (params.target_rows as f64 / params.target_files as f64)).ceil();

    // Z-ORDER clustering reduces files affected
    let z_order_factor = if params.z_order_columns.is_empty() {
        1.0
    } else {
        0.1  // 10x fewer files due to clustering
    };

    base_files * z_order_factor
}

fn choose_merge_strategy(params: &MergeCostParams) -&gt; MergeStrategy {
    let strategies = vec![
        (MergeStrategy::Hash, estimate_hash_merge_cost(params)),
        (MergeStrategy::Sort, estimate_sort_merge_cost(params)),
        (MergeStrategy::Index, estimate_index_merge_cost(params)),
        (MergeStrategy::Partition, estimate_partition_merge_cost(params)),
    ];

    strategies.into_iter().min_by(|a, b| a.1.partial_cmp(&b.1).unwrap()).unwrap().0
}
```

### Cross-Database Compatibility

MERGE semantics vary across database systems:

#### Databricks Delta Lake
```sql
MERGE INTO target USING source ON target.id = source.id
WHEN MATCHED THEN UPDATE SET *
WHEN NOT MATCHED THEN INSERT *
WHEN NOT MATCHED BY SOURCE THEN DELETE
```
- Full MERGE support with multiple WHEN clauses
- ACID guarantees via Delta transaction log
- Schema evolution support
- Single row match enforcement (error on duplicates)

#### SQL Server
```sql
MERGE target
USING source ON target.id = source.id
WHEN MATCHED THEN UPDATE SET target.value = source.value
WHEN NOT MATCHED BY TARGET THEN INSERT (id, value) VALUES (source.id, source.value)
WHEN NOT MATCHED BY SOURCE THEN DELETE;
```
- Similar syntax, explicit TARGET keyword
- Full transaction support
- OUTPUT clause for changed rows
- Must end with semicolon

#### Oracle
```sql
MERGE INTO target t
USING source s ON (t.id = s.id)
WHEN MATCHED THEN UPDATE SET t.value = s.value DELETE WHERE s.status = 'deleted'
WHEN NOT MATCHED THEN INSERT (id, value) VALUES (s.id, s.value) WHERE s.status = 'active';
```
- DELETE clause can follow UPDATE
- WHERE clause per action
- No WHEN NOT MATCHED BY SOURCE

#### PostgreSQL
```sql
-- No native MERGE until PostgreSQL 15
-- Use INSERT ... ON CONFLICT as workaround
INSERT INTO target (id, value)
SELECT id, value FROM source
ON CONFLICT (id) DO UPDATE SET value = EXCLUDED.value;
```
- Limited to single key conflict
- Cannot handle DELETE or NOT MATCHED BY SOURCE
- PostgreSQL 15+ supports MERGE with similar syntax to SQL Server

**Ra Compatibility Strategy:**

1. **Parse**: Support all syntax variants
2. **Normalize**: Convert to common internal representation
3. **Optimize**: Apply Delta Lake optimizations when target is Delta table
4. **Emit**: Generate database-specific SQL based on target system

## Implementation Plan

### Phase 1: Parser and Logical Plan (Weeks 1-3)

**Deliverables:**
- Extend SQL parser to recognize MERGE syntax
- New `LogicalMerge` node in RelExpr enum
- Support for multiple WHEN clauses with predicates
- Validation: Single source, single target, valid join condition

**Files:**
- `crates/ra-engine/src/expr.rs`: Add `LogicalMerge` variant
- `crates/ra-engine/src/parser/`: MERGE syntax parsing
- `crates/ra-engine/src/validation.rs`: MERGE semantic validation

**Testing:**
- Parse various MERGE syntax variants
- Error handling for invalid MERGE statements
- Cross-database syntax compatibility

### Phase 2: Delta Lake Metadata Integration (Weeks 4-6)

**Deliverables:**
- Delta transaction log reader
- File-level statistics extraction
- Partition metadata handling
- Z-ORDER awareness

**Files:**
- `crates/ra-engine/src/delta_metadata.rs` (new, ~400 lines)
  - `DeltaTable` struct with transaction log parsing
  - `FileStatistics` struct with min/max/null counts
  - `PartitionInfo` with partition key values
  - Functions to read `_delta_log/` JSON files

**Testing:**
- Read Delta transaction logs
- Parse file statistics
- Identify affected partitions for given source data

### Phase 3: Cost Model and Strategy Selection (Weeks 7-10)

**Deliverables:**
- MERGE cost model with four strategies
- File pruning cost estimation
- Strategy selection algorithm
- Hardware-aware cost parameters

**Files:**
- `crates/ra-engine/src/merge_cost_model.rs` (new, ~600 lines)
  - `MergeCostParams` struct
  - Cost estimation functions for each strategy
  - `choose_merge_strategy()` decision function
  - File pruning benefit estimation

**Testing:**
- Unit tests for cost model with various data distributions
- Validate strategy selection for edge cases
- Performance regression tests

### Phase 4: Optimization Rules (Weeks 11-14)

**Deliverables:**
- E-graph rewrite rules for MERGE optimization
- Partition pruning for MERGE
- Predicate pushdown (source and target)
- Bloom filter integration

**Rules (new files in `rules/delta-lake/`):**
- `merge-partition-pruning.rra`: Prune partitions based on source data
- `merge-file-pruning.rra`: Skip files using min/max statistics
- `merge-predicate-pushdown.rra`: Push predicates into source/target scans
- `merge-bloom-filter.rra`: Build Bloom filter from source for target pruning
- `merge-strategy-selection.rra`: Choose hash vs sort vs index vs partition strategy
- `merge-z-order-exploitation.rra`: Leverage Z-ORDER clustering for file skipping

**Testing:**
- E-graph equivalence tests
- Cost-based rule selection
- Integration tests with Delta tables

### Phase 5: Physical Execution (Weeks 15-18)

**Deliverables:**
- Physical operators for MERGE execution
- Transaction log writing
- File rewriting logic
- ACID guarantees

**Files:**
- `crates/ra-engine/src/executors/merge_executor.rs` (new, ~800 lines)
  - `MergeExecutor` trait
  - `HashMergeExecutor`, `SortMergeExecutor`, `IndexMergeExecutor`, `PartitionMergeExecutor`
  - Transaction log integration
  - File writer integration

**Testing:**
- End-to-end MERGE execution tests
- Transaction atomicity tests
- Concurrent MERGE conflict handling
- Large-scale performance tests

### Phase 6: Cross-Database Support (Weeks 19-20)

**Deliverables:**
- SQL generation for non-Delta targets
- Fallback to DELETE + INSERT for unsupported systems
- PostgreSQL INSERT ... ON CONFLICT rewrite

**Files:**
- `crates/ra-engine/src/dialect_backends/`: Update SQL generators
  - `databricks.rs`: Native MERGE
  - `sqlserver.rs`: SQL Server MERGE syntax
  - `oracle.rs`: Oracle MERGE syntax
  - `postgres.rs`: INSERT ... ON CONFLICT rewrite

**Testing:**
- Cross-database SQL generation tests
- Semantic equivalence validation

### Phase 7: Documentation and Benchmarking (Weeks 21-22)

**Deliverables:**
- User-facing documentation
- Performance benchmarks vs baseline
- Cost model tuning guide

**Files:**
- `docs/delta-lake-merge.md`: User guide
- `docs/merge-performance-tuning.md`: Tuning recommendations
- `benches/merge_benchmark.rs`: Performance benchmarks

**Testing:**
- Benchmark suite comparing strategies
- Real-world workload validation

### Phase 8: Integration and Hardening (Weeks 23-25)

**Deliverables:**
- Integration with existing Ra optimizer pipeline
- Edge case handling
- Error message improvements
- Performance profiling and optimization

**Tasks:**
- Integrate MERGE rules into `all_rules_unsorted()`
- Stress testing with large datasets
- Memory usage optimization
- Error handling for transaction conflicts

**Testing:**
- Full integration test suite
- Stress tests (1B+ row tables)
- Concurrent MERGE tests
- Failure recovery tests

## Testing Strategy

### Unit Tests (150+ tests)

**Cost Model Tests** (30 tests):
- Hash merge cost estimation with varying source/target sizes
- Sort merge cost with pre-sorted data
- Index merge cost with different index types
- Partition merge cost with skewed partition distribution
- File pruning effectiveness with various statistics
- Z-ORDER impact on file reduction

**Strategy Selection Tests** (20 tests):
- Small source, large target → Index merge
- Equal-sized source/target → Hash or sort merge
- Partitioned table with date filter → Partition merge
- All combinations of WHEN clauses

**Metadata Parsing Tests** (25 tests):
- Delta transaction log JSON parsing
- File statistics extraction
- Partition metadata handling
- Z-ORDER column detection
- Schema evolution handling

**Optimization Rule Tests** (40 tests):
- Partition pruning correctness
- File pruning using min/max statistics
- Predicate pushdown validation
- Bloom filter construction and application
- E-graph rewrite equivalence

**Execution Tests** (35 tests):
- Hash merge execution correctness
- Sort merge execution correctness
- Index merge execution correctness
- Partition merge execution correctness
- Multiple WHEN clause handling
- Transaction atomicity

### Integration Tests (50+ tests)

**End-to-End Workflows**:
- Full MERGE on small dataset (1K rows)
- Incremental MERGE with 1% data change (100K rows)
- Large-scale MERGE with 50% data change (10M rows)
- Partitioned MERGE with date-based partitioning
- Z-ORDERed table MERGE with key locality

**Cross-Database Tests**:
- Generate Databricks Delta Lake SQL
- Generate SQL Server MERGE SQL
- Generate Oracle MERGE SQL
- Generate PostgreSQL INSERT ... ON CONFLICT
- Fallback to DELETE + INSERT for unsupported systems

**Concurrent MERGE Tests**:
- Two concurrent MERGEs on same table (conflict detection)
- MERGE with concurrent SELECT (snapshot isolation)
- MERGE with concurrent DELETE (MVCC validation)

### Performance Tests (Benchmarks)

**Baseline Comparisons**:
- MERGE vs DELETE + INSERT (2x scan baseline)
- Hash merge vs sort merge for various data distributions
- Partition-aware vs naive MERGE
- Z-ORDERed vs non-Z-ORDERed tables

**Scalability Tests**:
- 1K, 10K, 100K, 1M, 10M, 100M row tables
- 1%, 10%, 50%, 100% data change ratios
- 10, 100, 1000 partitions
- Various Z-ORDER column configurations

**Target Metrics**:
- 10-50x speedup vs DELETE + INSERT for incremental updates
- 5-20x speedup with file pruning
- 20-100x speedup with partition pruning on date-partitioned tables
- Near-linear scalability with partition count

## Performance Analysis

### Baseline: DELETE + INSERT Pattern

**Query:**
```sql
-- Traditional approach (anti-pattern)
DELETE FROM target WHERE id IN (SELECT id FROM source);
INSERT INTO target SELECT * FROM source;
```

**Cost Breakdown:**
1. **DELETE phase**:
   - Full target table scan: O(target_rows)
   - Subquery execution: O(source_rows)
   - File rewriting: O(target_files)  (all files touched)

2. **INSERT phase**:
   - Full source table scan: O(source_rows)
   - File writing: O(source_rows)

**Total Cost**: O(2 * target_rows + 2 * source_rows)

**Problems**:
- Two separate transactions (not atomic)
- Full table scan twice
- All target files rewritten (even unchanged data)
- No file-level or partition-level pruning

### Optimized: Delta Lake MERGE with Pruning

**Query:**
```sql
MERGE INTO target USING source ON target.id = source.id
WHEN MATCHED THEN UPDATE SET *
WHEN NOT MATCHED THEN INSERT *
```

**Cost Breakdown (with optimizations):**

1. **Partition Pruning**:
   - Analyze source data: O(source_rows) [one pass]
   - Identify affected partitions: O(partition_count) [metadata scan]
   - Result: target_rows reduced to affected_partition_rows

2. **File Pruning**:
   - Read file statistics: O(file_count) [metadata scan]
   - Apply min/max filtering: O(file_count)
   - Result: Only process files with potential matches

3. **MERGE Execution** (hash-based):
   - Build hash table from source: O(source_rows)
   - Probe with pruned target rows: O(affected_target_rows)
   - Rewrite only affected files: O(affected_files)

**Total Cost**: O(source_rows + affected_target_rows + affected_files)

Where:
- `affected_target_rows = target_rows * selectivity`
- `selectivity` = fraction of data affected by MERGE
- For incremental updates: `selectivity = 0.01` to `0.1` (1-10%)

**Speedup Calculation:**

| Update Size | Selectivity | Baseline Cost | Optimized Cost | Speedup |
|-------------|-------------|---------------|----------------|---------|
| 1% of data  | 0.01 | 2T + 2S | S + 0.01T | 50-100x |
| 10% of data | 0.10 | 2T + 2S | S + 0.10T | 10-20x |
| 50% of data | 0.50 | 2T + 2S | S + 0.50T | 2-4x |
| 100% of data | 1.00 | 2T + 2S | S + T | 1.5-2x |

(T = target_rows, S = source_rows, assuming S &lt;&lt; T)

### Real-World Performance Example

**Scenario**: E-commerce order table
- Target table: 1 billion orders
- Daily incremental update: 1 million new/updated orders (0.1%)
- Table partitioned by order_date (365 partitions)
- Z-ORDERed by customer_id

**Baseline (DELETE + INSERT)**:
- Scan 1B rows twice: 2000 seconds @ 1M rows/sec
- Rewrite all 10,000 files: 500 seconds
- Total: 2500 seconds (41 minutes)

**Optimized MERGE**:
- Partition pruning: Process only today's partition (1/365 of data)
- Target rows in partition: 2.7M rows
- File pruning with Z-ORDER: Process 10 files (of 10,000 total)
- Hash-based merge: 1M source + 2.7M target = 3.7M rows processed
- Rewrite 10 files: 5 seconds
- Total: 3.7 seconds + 5 seconds = 8.7 seconds

**Speedup: 287x**

## Alternatives Considered

### Alternative 1: Client-Side DELETE + INSERT

**Approach**: Application code handles MERGE logic with separate DELETE and INSERT statements.

**Rejected Because**:
- Not atomic (consistency issues)
- Two full table scans
- No file-level or partition-level pruning
- High network overhead (read all rows, send updates)

### Alternative 2: Rewrite MERGE as Standard JOIN + CASE

**Approach**: Transform MERGE into:
```sql
INSERT INTO target
SELECT
  CASE
    WHEN target.id IS NOT NULL AND source.status = 'deleted' THEN NULL  -- DELETE
    WHEN target.id IS NOT NULL THEN source.*  -- UPDATE
    ELSE source.*  -- INSERT
  END
FROM source
FULL OUTER JOIN target ON source.id = target.id
```

**Problems**:
- Full outer join scans entire target table (no pruning)
- Cannot leverage Delta Lake metadata (min/max, Z-ORDER)
- Difficult to optimize per-strategy (hash vs sort vs partition)
- Cannot handle WHEN NOT MATCHED BY SOURCE efficiently

**Rejected Because**: Loses all Delta Lake optimization opportunities.

### Alternative 3: Lazy Evaluation (Mark-and-Sweep)

**Approach**: Mark rows for update/delete in separate metadata, apply changes lazily during VACUUM.

**Problems**:
- Readers see stale data until VACUUM runs
- Complicates MVCC and transaction isolation
- VACUUM overhead becomes blocking operation
- Adds complexity to all read operations

**Rejected Because**: Breaks ACID guarantees and complicates architecture.

## Future Work

### Adaptive MERGE Strategy Selection

Learn optimal strategy from query execution history:
- Collect runtime statistics (actual cost vs estimated cost)
- Train ML model to predict best strategy
- Adjust cost model parameters per workload

### Incremental Statistics Maintenance

Currently, statistics are recomputed on OPTIMIZE. Instead:
- Update statistics incrementally during MERGE
- Track column correlation changes
- Trigger OPTIMIZE only when statistics deviate significantly

### MERGE-Driven Materialized View Maintenance

Integrate MERGE with incremental view maintenance:
- Detect changes from MERGE operations
- Propagate changes to dependent materialized views
- Use MERGE to update views efficiently (vs full recompute)

### Multi-Table MERGE

Extend to support updating multiple target tables in single MERGE:
```sql
MERGE INTO (target1, target2)
USING source ON ...
WHEN MATCHED THEN
  UPDATE target1 SET ...
  UPDATE target2 SET ...
```

Use case: Maintaining denormalized tables atomically.

### Change Data Feed Integration

Capture MERGE changes as CDC stream:
- Emit change events (insert/update/delete) from MERGE
- Feed into downstream systems (Kafka, message queues)
- Enable real-time data synchronization

## References

- [Delta Lake MERGE Documentation](https://docs.delta.io/latest/delta-update.html#upsert-into-a-table-using-merge)
- [Databricks MERGE Optimization Guide](https://docs.databricks.com/optimizations/merge.html)
- [SQL Server MERGE Statement](https://learn.microsoft.com/en-us/sql/t-sql/statements/merge-transact-sql)
- [Oracle MERGE Statement](https://docs.oracle.com/en/database/oracle/oracle-database/23/sqlrf/MERGE.html)
- [PostgreSQL MERGE (v15+)](https://www.postgresql.org/docs/15/sql-merge.html)
- [Delta Lake Transaction Protocol](https://github.com/delta-io/delta/blob/master/PROTOCOL.md)
- [Z-ORDER Clustering](https://docs.databricks.com/delta/data-skipping.html#z-ordering-multi-dimensional-clustering)

## Related RFCs

- [RFC 0059](/maintainers/rfcs/0059-statistics-based-plan-cache-invalidation): Statistics-Based Plan Cache Invalidation (statistics tracking)
- [RFC 0069](/maintainers/rfcs/0069-execution-feedback-loop): Execution Feedback Loop (adaptive cost model tuning)
- [RFC 0072](/maintainers/rfcs/0072-adaptive-parallelism): Adaptive Parallelism (parallel partition processing)
- [RFC 0085](/maintainers/rfcs/0085-platform-specific-rule-architecture): Platform-Specific Rule Architecture (Delta Lake rule organization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 104: Delta Lake MERGE Optimization](/maintainers/rfcs/0104-delta-lake-merge-optimization)
