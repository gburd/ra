# RFC 0100: Time Travel Queries

- Start Date: 2026-03-28
- Author: Ra Research Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should provide comprehensive optimization for temporal queries that access historical table states using Time Travel clauses. This RFC adds support for Snowflake's `AT`/`BEFORE` syntax and Delta Lake's `VERSION AS OF`/`TIMESTAMP AS OF` syntax, enabling audit trails, compliance reporting, debugging, and A/B testing scenarios. Ra will model temporal dimensions in scan operators, implement versioned statistics, optimize time range queries, and provide cost models for historical data access.

## Motivation

Time Travel is essential for cloud data warehouses, enabling queries like "What did this table look like yesterday?" or "Show all changes in the last hour." This capability is critical for:

**Audit and Compliance**:
- Financial regulations requiring historical transaction reconstruction
- GDPR right-to-be-forgotten verification
- SOX compliance auditing

**Debugging and Recovery**:
- Identifying when incorrect data was introduced
- Restoring accidentally deleted rows
- Comparing before/after states for schema migrations

**Temporal Analysis**:
- A/B testing by comparing query results across versions
- Time-series analysis of changing data
- Incremental ETL by computing deltas between snapshots

**Key optimization gaps:**

| Gap | Impact |
|-----|--------|
| No temporal dimension in scan operators | Cannot represent AT/BEFORE clauses |
| Flat cost for historical queries | Wrong plan choice for old versions |
| No version metadata caching | Repeated metadata lookups |
| No temporal partition pruning | Full table scans for time ranges |
| No versioned statistics | Inaccurate cardinality estimates |

Without Time Travel support, Ra cannot optimize critical cloud warehouse workloads. This RFC addresses gaps identified in the Snowflake and Databricks feature analyses.

## Guide-level explanation

### Snowflake Time Travel syntax

Snowflake provides three temporal clause variants:

**AT TIMESTAMP** - Query as of specific timestamp:
```sql
SELECT * FROM orders
AT(TIMESTAMP =&gt; '2024-01-01 12:00:00'::TIMESTAMP_TZ)
WHERE region = 'WEST';
```

**AT OFFSET** - Query N seconds in the past:
```sql
SELECT * FROM inventory
AT(OFFSET =&gt; -300)  -- 5 minutes ago
WHERE stock &lt; 10;
```

**BEFORE STATEMENT** - Query before specific statement executed:
```sql
SELECT * FROM products
BEFORE(STATEMENT =&gt; '01a2b3c4-5678-90ab-cdef-1234567890ab')
WHERE category = 'electronics';
```

Snowflake retention: 1 day (Standard), up to 90 days (Enterprise).

### Delta Lake Time Travel syntax

Delta Lake provides version-based and timestamp-based access:

**VERSION AS OF** - Query specific transaction version:
```sql
SELECT * FROM sales VERSION AS OF 42
WHERE amount &gt; 1000;
```

**TIMESTAMP AS OF** - Query as of timestamp:
```sql
SELECT * FROM customers
TIMESTAMP AS OF '2024-01-01 00:00:00'
WHERE status = 'active';
```

**Shorthand notation**:
```sql
SELECT * FROM sales@v42;
SELECT * FROM sales@20240101000000000;
```

Delta retention: Configurable via `delta.logRetentionDuration` (default 30 days) and `delta.deletedFileRetentionDuration` (default 7 days).

### Cross-database variants

**MariaDB System-Versioned Tables**:
```sql
SELECT * FROM employees
FOR SYSTEM_TIME AS OF TIMESTAMP '2024-01-01 00:00:00'
WHERE dept = 'engineering';
```

**SQL Server Temporal Tables**:
```sql
SELECT * FROM Orders
FOR SYSTEM_TIME AS OF '2024-01-01 00:00:00'
WHERE customer_id = 123;
```

### Optimization examples

**Temporal partition pruning** - Push time range filters:
```sql
-- Original query
SELECT * FROM events
AT(TIMESTAMP =&gt; '2024-01-15 10:00:00')
WHERE event_date BETWEEN '2024-01-10' AND '2024-01-20';

-- Ra recognizes temporal + partition filters overlap
-- Prunes to partitions active on 2024-01-15 AND in date range
-- Avoids scanning historical partitions outside window
```

**Delta query optimization** - Efficient diff computation:
```sql
-- Original: Compute differences between two versions
SELECT * FROM orders AT(TIMESTAMP =&gt; '2024-01-02')
EXCEPT
SELECT * FROM orders AT(TIMESTAMP =&gt; '2024-01-01');

-- Ra optimizes to incremental delta scan:
-- 1. Identify changed micro-partitions between timestamps
-- 2. Scan only changed partitions (not full table twice)
-- 3. Apply set difference on changed rows only
-- Expected speedup: 10-100x for small deltas
```

**Temporal join** - Join current with historical data:
```sql
-- Compare today's inventory with last week
SELECT
    curr.product_id,
    curr.stock - hist.stock AS stock_change
FROM inventory curr
JOIN inventory AT(OFFSET =&gt; -604800) hist
  ON curr.product_id = hist.product_id;

-- Ra applies standard join optimization but accounts for
-- historical scan overhead in cost model
```

## Reference-level explanation

### Temporal dimension in RelExpr

Extend `RelExpr::Scan` with temporal clause:

```rust
pub struct Scan {
    pub table: TableId,
    pub columns: Vec&lt;ColumnId&gt;,
    pub filter: Option&lt;Expr&gt;,
    pub time_travel: Option&lt;TimeTravelClause&gt;,  // NEW
}

pub enum TimeTravelClause {
    /// Snowflake: AT(TIMESTAMP =&gt; '...')
    AtTimestamp(DateTime&lt;Utc&gt;),

    /// Snowflake: AT(OFFSET =&gt; -N)
    AtOffset(i64),

    /// Snowflake: BEFORE(STATEMENT =&gt; '...')
    BeforeStatement(String),

    /// Delta Lake: VERSION AS OF N
    AtVersion(u64),

    /// MariaDB/SQL Server: FOR SYSTEM_TIME AS OF '...'
    SystemTimeAsOf(DateTime&lt;Utc&gt;),
}
```

### Implementation approaches

**1. MVCC-Based (PostgreSQL, Oracle)**

Store multiple row versions with validity periods:

```
Table structure:
| row_id | data | version_start | version_end | deleted |
|--------|------|---------------|-------------|---------|
| 1      | v1   | 2024-01-01    | 2024-01-02  | false   |
| 1      | v2   | 2024-01-02    | NULL        | false   |
```

**Advantages**:
- Fast access to any version
- Efficient incremental updates
- Supports fine-grained versioning

**Disadvantages**:
- Storage overhead (multiple row versions)
- Garbage collection complexity
- Version chain traversal for old data

**2. Copy-on-Write (Delta Lake, Iceberg)**

Immutable data files with transaction log:

```
Transaction log:
Version 1: Add files [f1.parquet, f2.parquet]
Version 2: Add [f3.parquet], Remove [f1.parquet]
Version 3: Add [f4.parquet]

To read version 2:
  files = [f2.parquet, f3.parquet]
```

**Advantages**:
- No in-place updates
- Simple version reconstruction
- Efficient for bulk updates

**Disadvantages**:
- Version metadata growth over time
- File-level granularity only
- Retention policy management

**3. Delta Files (Incremental)**

Base snapshot + incremental change files:

```
Base snapshot (v1): All data at t=0
Delta 1-10: Changes from v1 to v10
Delta 10-20: Changes from v10 to v20

To read version 15:
  data = base + delta_1_10 + delta_10_20[up to v15]
```

**Advantages**:
- Storage efficient for small changes
- Fast recent history access
- Compaction flexibility

**Disadvantages**:
- Slow for distant past (many deltas)
- Complex compaction logic
- Delta chain traversal overhead

### Cost model for historical queries

Temporal scan cost depends on version distance and storage format:

```rust
pub fn temporal_scan_cost(
    base_cost: f64,
    time_travel: &TimeTravelClause,
    current_time: DateTime&lt;Utc&gt;,
    retention_policy: &RetentionPolicy,
) -&gt; f64 {
    let version_distance = match time_travel {
        TimeTravelClause::AtTimestamp(ts) =&gt; {
            (current_time - ts).num_seconds() as f64
        }
        TimeTravelClause::AtOffset(offset) =&gt; offset.abs() as f64,
        TimeTravelClause::AtVersion(v) =&gt; {
            // Requires metadata lookup to current version
            (current_version - v) as f64
        }
        _ =&gt; 0.0,
    };

    // Cost multiplier based on version distance
    let temporal_overhead = match retention_policy.storage_format {
        StorageFormat::MVCC =&gt; {
            // Version chain traversal: O(versions_back)
            1.0 + version_distance / 86400.0 * 0.5  // 0.5x per day
        }
        StorageFormat::CopyOnWrite =&gt; {
            // Metadata lookup + file access
            if version_distance &lt; 86400.0 {  // &lt; 1 day
                2.0  // Recent: Metadata cached
            } else {
                5.0  // Old: Cold metadata + cold files
            }
        }
        StorageFormat::DeltaFiles =&gt; {
            // Delta chain traversal: O(deltas_to_traverse)
            let deltas_per_day = 24.0;  // Assume hourly compaction
            let days_back = version_distance / 86400.0;
            2.0 + days_back * deltas_per_day * 0.1
        }
    };

    base_cost * temporal_overhead
}
```

**Example costs** (10M row table, 1000 rows selected):

| Scenario | Base Cost | Temporal Overhead | Total Cost |
|----------|-----------|-------------------|------------|
| Current data | 1000 | 1.0x | 1000 |
| 1 hour ago (MVCC) | 1000 | 1.02x | 1020 |
| 1 day ago (CoW) | 1000 | 2.0x | 2000 |
| 30 days ago (CoW) | 1000 | 5.0x | 5000 |
| 30 days ago (Delta) | 1000 | 74x | 74000 |

### Temporal partition pruning

Combine temporal predicates with partition filters:

```rust
pub fn prune_partitions_temporal(
    partitions: &[PartitionInfo],
    time_travel: &TimeTravelClause,
    filter: &Expr,
) -&gt; Vec&lt;PartitionId&gt; {
    let query_timestamp = time_travel.resolve_timestamp();

    partitions
        .iter()
        .filter(|p| {
            // Partition was active at query timestamp
            p.created_at &lt;= query_timestamp &&
            query_timestamp &lt; p.deleted_at.unwrap_or(DateTime::MAX)
        })
        .filter(|p| {
            // Partition matches query filter
            partition_matches_filter(p, filter)
        })
        .map(|p| p.id)
        .collect()
}
```

**Optimization impact**: 10-100x reduction in scanned data for queries with time range filters.

### Version metadata caching

Cache version-to-metadata mappings to avoid repeated lookups:

```rust
pub struct VersionMetadataCache {
    cache: LruCache&lt;(TableId, TimeTravelClause), VersionMetadata&gt;,
    ttl: Duration,
}

pub struct VersionMetadata {
    pub version_id: u64,
    pub timestamp: DateTime&lt;Utc&gt;,
    pub active_partitions: Vec&lt;PartitionId&gt;,
    pub statistics: TableStatistics,
    pub cached_at: DateTime&lt;Utc&gt;,
}
```

**Cache invalidation**:
- Version metadata is immutable (historical data never changes)
- No invalidation needed unless retention policy deletes old versions
- TTL: 24 hours (balance memory vs lookup cost)

**Cache hit rate impact**:
- Cold cache: 5-10ms metadata lookup per temporal query
- Warm cache: &lt;0.1ms cache lookup
- Expected hit rate: 80%+ for dashboards querying same historical points

### Versioned statistics

Maintain statistics per version for accurate cardinality estimation:

```rust
pub struct VersionedStatistics {
    pub table_id: TableId,
    pub version_stats: BTreeMap&lt;u64, TableStatistics&gt;,
    pub granularity: VersionGranularity,
}

pub enum VersionGranularity {
    /// Store statistics for every version
    PerVersion,

    /// Store statistics for daily snapshots
    Daily,

    /// Store statistics for major versions only (10, 100, 1000, ...)
    Logarithmic,
}
```

**Statistics interpolation**: For versions without stored statistics, interpolate from nearest stored versions:

```rust
pub fn interpolate_statistics(
    before: &TableStatistics,
    after: &TableStatistics,
    target_version: u64,
    before_version: u64,
    after_version: u64,
) -&gt; TableStatistics {
    let ratio = (target_version - before_version) as f64 /
                (after_version - before_version) as f64;

    TableStatistics {
        row_count: interpolate_linear(
            before.row_count,
            after.row_count,
            ratio,
        ),
        histograms: interpolate_histograms(
            &before.histograms,
            &after.histograms,
            ratio,
        ),
        // ...
    }
}
```

### Optimization rules

**Rule 1: Temporal predicate pushdown**

Push predicates into temporal scans to reduce rows before version lookup:

```
Before:
  Filter(amount &gt; 1000)
    Scan(sales AT(TIMESTAMP =&gt; '2024-01-01'))

After:
  Scan(sales AT(TIMESTAMP =&gt; '2024-01-01'))
    .with_filter(amount &gt; 1000)
```

**Rule 2: Temporal partition pruning**

Convert temporal + partition filters into partition elimination:

```
Before:
  Scan(events AT(TIMESTAMP =&gt; '2024-01-15'))
    .filter(date BETWEEN '2024-01-10' AND '2024-01-20')

After:
  Scan(events AT(TIMESTAMP =&gt; '2024-01-15'))
    .partitions(pruned to 10-day window + active on 2024-01-15)
```

**Rule 3: Delta query optimization**

Optimize EXCEPT queries between temporal versions:

```
Before:
  Except(
    Scan(orders AT(TIMESTAMP =&gt; '2024-01-02')),
    Scan(orders AT(TIMESTAMP =&gt; '2024-01-01'))
  )

After:
  DeltaScan(
    orders,
    start_time = '2024-01-01',
    end_time = '2024-01-02',
    operation = INSERT
  )
```

**Rule 4: Version metadata caching**

Reuse cached version metadata for repeated temporal queries:

```
Before: Query temporal table
  -&gt; Lookup version metadata (5-10ms)
  -&gt; Resolve partitions
  -&gt; Execute scan

After: Query temporal table
  -&gt; Check cache (0.1ms)
  -&gt; Use cached partition list
  -&gt; Execute scan
```

## Drawbacks

**Version metadata storage overhead**: Storing statistics per version increases metadata size. For a table with 1000 versions and 100 columns, versioned statistics require ~10MB per table.

**Retention policy complexity**: Different databases have different retention defaults. Ra must handle cross-database differences:
- Snowflake: 1-90 days
- Delta Lake: Configurable (default 30 days)
- MariaDB: Indefinite retention
- SQL Server: Indefinite retention with compression

**Interpolation accuracy**: For versions without stored statistics, interpolation may produce inaccurate estimates, especially for non-linear data growth patterns.

**Cost model calibration**: Historical query costs vary significantly by storage format and version distance. Initial cost model will be approximate until calibrated from production workloads.

## Rationale and alternatives

**Why add temporal dimension to Scan instead of new operator?**

Alternative: Create `TemporalScan` operator separate from `Scan`.

Decision: Extend `Scan` with optional `time_travel` field because:
1. Temporal scans have identical semantics to regular scans (filter, projection, etc.)
2. Existing scan optimization rules apply with minor cost adjustments
3. Avoids duplicating scan-related rules for temporal variant
4. Matches how databases implement Time Travel (clause on existing scan)

**Why cache version metadata instead of query result?**

Alternative: Cache entire query results like Snowflake's result cache.

Decision: Cache metadata separately because:
1. Version metadata is smaller than query results
2. Metadata applies to multiple queries with different filters
3. Query results invalidate on any data change; metadata is immutable
4. Complements existing plan cache in Ra

**Why support multiple Time Travel syntaxes?**

Alternative: Standardize on single syntax (e.g., SQL:2011 temporal syntax).

Decision: Support database-specific syntaxes because:
1. Users write queries in native syntax for their database
2. Ra's polyglot backend already handles syntax differences
3. Translating temporal clauses across databases is complex (version vs timestamp semantics differ)
4. Optimization opportunities differ by implementation (MVCC vs CoW)

## Prior art

**Snowflake Time Travel**:
- Metadata-only access for unchanged micro-partitions
- Incremental diff computation between versions
- Partition pruning with temporal predicates
- 1-90 day retention based on edition

**Databricks Delta Lake**:
- Transaction log-based versioning
- Copy-on-write file semantics
- Version AS OF and TIMESTAMP AS OF
- Configurable retention policies

**PostgreSQL MVCC**:
- Version chain traversal via heap tuple headers
- VACUUM for garbage collection
- `pg_visibility` extension for version inspection
- No native Time Travel syntax (requires extensions)

**SQL Server Temporal Tables**:
- System-versioned tables with history table
- Automatic history tracking on UPDATE/DELETE
- `FOR SYSTEM_TIME` clause with multiple variants:
  - AS OF timestamp
  - FROM start TO end
  - BETWEEN start AND end
  - CONTAINED IN (start, end)

**MariaDB System-Versioned Tables**:
- Similar to SQL Server with transaction ID or timestamp versioning
- `FOR SYSTEM_TIME AS OF` clause
- Partition-level retention policies

## Unresolved questions

1. **How to handle retention policy enforcement during optimization?**
   - Should Ra warn when queries access data outside retention window?
   - Should Ra automatically adjust queries to nearest available version?

2. **How to estimate version count without metadata lookup?**
   - For Delta query optimization, need to estimate intermediate versions
   - Could use average daily change rate from table statistics
   - Requires feedback loop from actual execution

3. **Should Ra support specifying retention policies in catalog?**
   - Would enable recommending retention based on query patterns
   - Adds complexity to catalog schema
   - May conflict with database-native retention settings

4. **How to handle temporal queries in materialized views?**
   - Should MVs cache historical results?
   - How to invalidate when retention deletes old versions?
   - Could treat as regular view (no caching) for simplicity

## Future possibilities

**Temporal indexes**: Recommend indexes on validity period columns for faster version lookup in MVCC systems.

**Smart retention recommendations**: Analyze query patterns to recommend optimal retention policies (e.g., "Queries access last 7 days 95% of time, set retention to 14 days").

**Cross-version query optimization**: Detect queries that scan multiple versions sequentially and batch version metadata lookups.

**Temporal join optimization**: Specialized join algorithms for temporal-temporal joins (e.g., interval join for overlapping validity periods).

**Approximate Time Travel**: For very old versions outside retention, approximate results from aggregated historical statistics.

## Implementation plan

**Phase 1: Core infrastructure (6-8 weeks)**

1. Add `TimeTravelClause` enum and extend `RelExpr::Scan` (1 week)
2. Implement parser support for Snowflake AT/BEFORE syntax (1 week)
3. Implement parser support for Delta Lake VERSION AS OF/TIMESTAMP AS OF (1 week)
4. Add temporal dimension to catalog metadata (1 week)
5. Implement basic temporal scan executor (2 weeks)
6. Add temporal scan cost model (1 week)
7. Write unit tests for temporal clause parsing and representation (1 week)

**Phase 2: Optimization (8-10 weeks)**

1. Implement version metadata cache (2 weeks)
   - LRU cache with TTL
   - Cache invalidation on retention expiry
   - Integration with existing plan cache

2. Implement temporal partition pruning (2 weeks)
   - Combine temporal filters with partition filters
   - Metadata-based partition elimination
   - Integration with existing partition pruning rules

3. Add versioned statistics (3 weeks)
   - Per-version statistics storage
   - Statistics interpolation algorithm
   - Granularity controls (per-version vs daily vs logarithmic)

4. Implement optimization rules (3 weeks)
   - Temporal predicate pushdown
   - Delta query optimization (EXCEPT between versions)
   - Temporal + partition filter coordination
   - Version metadata caching

**Phase 3: Cross-database support (4-6 weeks)**

1. Add MariaDB FOR SYSTEM_TIME syntax (1 week)
2. Add SQL Server temporal table support (1 week)
3. Implement storage format detection (MVCC vs CoW vs Delta) (2 weeks)
4. Add format-specific cost models (1 week)
5. Dialect translation for temporal clauses (1 week)

**Phase 4: Advanced features (6-8 weeks)**

1. Time range indexes recommendation (2 weeks)
2. Smart retention policy recommendations (2 weeks)
3. Cross-version query batching (2 weeks)
4. Temporal join optimization (2 weeks)

**Total estimated effort: 24-32 weeks**

**Phase dependencies**:
- Phase 2 depends on Phase 1 (core infrastructure)
- Phase 3 can start after Phase 1 (parallel with Phase 2)
- Phase 4 depends on Phases 1-3

**Expected impact**:
- **High**: Essential for cloud warehouse compliance and debugging use cases
- **Performance**: 2-5x speedup for recent history queries, 10-20x for distant past with delta optimization
- **Usability**: Enables audit queries, A/B testing, data recovery scenarios
- **Coverage**: Supports 80%+ of Time Travel patterns in Snowflake and Delta Lake

## Performance analysis

**Baseline: Current data query**
```
Table: 100M rows, 1GB compressed
Query: SELECT * FROM orders WHERE region = 'WEST'
  Selectivity: 10% (10M rows)
  Cost: 100 (index scan)
  Time: 2s
```

**Scenario 1: Recent history (1 day ago, MVCC)**
```
Cost: 100 * 1.02 = 102
Time: 2.04s
Overhead: 2%
Reason: Version chain traversal minimal
```

**Scenario 2: Recent history (1 day ago, CoW)**
```
Cost: 100 * 2.0 = 200
Time: 4s
Overhead: 100%
Reason: Metadata lookup + cold cache
```

**Scenario 3: Distant past (30 days ago, CoW)**
```
Cost: 100 * 5.0 = 500
Time: 10s
Overhead: 400%
Reason: Cold metadata + cold storage
```

**Scenario 4: Delta query (2 versions, 0.1% change rate)**
```
Without optimization:
  Cost: 100 * 2 * 2.0 = 400
  Time: 8s

With delta optimization:
  Changed rows: 0.1% = 100K
  Cost: 100K * 0.5 (delta scan) = 50
  Time: 1s

Speedup: 8x
```

**Scenario 5: Temporal partition pruning**
```
Table: 100M rows, 100 partitions
Query: SELECT * FROM events
       AT(TIMESTAMP =&gt; '2024-01-15')
       WHERE date BETWEEN '2024-01-10' AND '2024-01-20'

Without temporal pruning:
  Partitions scanned: 10 (date range)
  Versions checked: 100 partitions * 365 versions = 36,500
  Cost: 10,000

With temporal pruning:
  Partitions scanned: 10 (date range)
  Versions checked: 10 (only partitions active on 2024-01-15)
  Cost: 1,000

Speedup: 10x
```

## Testing strategy

**Unit tests**:
1. Parse Snowflake AT/BEFORE syntax
2. Parse Delta Lake VERSION AS OF/TIMESTAMP AS OF
3. Parse MariaDB/SQL Server FOR SYSTEM_TIME
4. Version metadata cache hit/miss
5. Statistics interpolation accuracy
6. Temporal partition pruning logic
7. Delta query detection

**Integration tests**:
1. Execute temporal scan on MVCC storage (PostgreSQL + extension)
2. Execute temporal scan on CoW storage (Delta Lake)
3. Temporal predicate pushdown optimization
4. Delta query optimization (EXCEPT between versions)
5. Temporal + partition filter coordination
6. Version metadata cache invalidation on retention expiry
7. Cross-database dialect translation

**Performance tests**:
1. Compare temporal scan overhead vs baseline (target: &lt;5x for recent history)
2. Measure version metadata cache hit rate (target: &gt;80%)
3. Measure delta query speedup (target: &gt;5x for &lt;1% change rate)
4. Measure temporal partition pruning speedup (target: &gt;10x)
5. Benchmark statistics interpolation accuracy (target: &lt;20% error)

**Correctness tests**:
1. Verify temporal scan returns exact historical state
2. Verify delta query matches EXCEPT semantics
3. Verify partition pruning doesn't skip required data
4. Verify version metadata cache consistency
5. Verify retention policy enforcement

## Success criteria

1. **Correctness**: 100% of temporal queries return identical results to database-native execution
2. **Performance**: &lt;5x overhead for recent history (&lt;1 day), 5-20x for distant past
3. **Optimization impact**: &gt;5x speedup for delta queries with &lt;1% change rate
4. **Cache efficiency**: &gt;80% version metadata cache hit rate
5. **Coverage**: Support 80%+ of Snowflake and Delta Lake Time Travel patterns

## References

- Snowflake Features Gap Analysis: `/home/gburd/ws/ra/SNOWFLAKE_FEATURES_GAP_ANALYSIS.md`
- Databricks Features Analysis: `/home/gburd/ws/ra/DATABRICKS_SPARK_FEATURES_ANALYSIS.md`
- [RFC 0061](/maintainers/rfcs/0061-postgresql-extension-aware-optimization): PostgreSQL Extension-Aware Optimization
- RFC 0026: Adaptive Cost Calibration
- Snowflake Time Travel Documentation
- Delta Lake Time Travel Documentation
- SQL:2011 Temporal Data Standard


## Referenced By

This RFC is referenced by:

- [RFC 100: Time Travel Queries](/maintainers/rfcs/0100-time-travel-queries)


## Referenced By

This RFC is referenced by:

- [RFC 100: Time Travel Queries](/maintainers/rfcs/0100-time-travel-queries)


## Referenced By

This RFC is referenced by:

- [RFC 100: Time Travel Queries](/maintainers/rfcs/0100-time-travel-queries)


## Referenced By

This RFC is referenced by:

- [RFC 100: Time Travel Queries](/maintainers/rfcs/0100-time-travel-queries)


## Referenced By

This RFC is referenced by:

- [RFC 100: Time Travel Queries](/maintainers/rfcs/0100-time-travel-queries)


## Referenced By

This RFC is referenced by:

- [RFC 100: Time Travel Queries](/maintainers/rfcs/0100-time-travel-queries)


## Referenced By

This RFC is referenced by:

- [RFC 100: Time Travel Queries](/maintainers/rfcs/0100-time-travel-queries)


## Referenced By

This RFC is referenced by:

- [RFC 100: Time Travel Queries](/maintainers/rfcs/0100-time-travel-queries)


## Referenced By

This RFC is referenced by:

- [RFC 100: Time Travel Queries](/maintainers/rfcs/0100-time-travel-queries)


## Referenced By

This RFC is referenced by:

- [RFC 100: Time Travel Queries](/maintainers/rfcs/0100-time-travel-queries)
