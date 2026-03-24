# RFC 0061: PostgreSQL Extension-Aware Optimization

- Start Date: 2026-03-24
- Author: Ra Development Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

Ra should detect installed PostgreSQL extensions at planning time and activate
extension-specific optimization rules, cost model adjustments, and index
recommendations. Extensions like PostGIS, TimescaleDB, Citus, pg_trgm, hstore,
ltree, citext, bloom, btree_gin, btree_gist, pg_partman, pg_cron, and
pg_stat_statements add types, operators, index access methods, and execution
strategies that fundamentally change optimal query plans. This RFC defines an
extension detection API, a capability registry, and per-extension optimization
strategies that integrate with the existing planner hook in
`crates/ra-pg-extension/src/planner_hook.rs`.

## Motivation

PostgreSQL extensions are not optional add-ons -- they define how production
workloads operate. PostGIS powers every major mapping application. TimescaleDB
runs time-series infrastructure at scale. Citus distributes queries across
shards. These extensions introduce custom types, operators, index access
methods, and planner strategies that the standard PostgreSQL optimizer partially
understands but does not fully exploit.

Ra currently treats extension-provided types as opaque (`DataType::Other(String)`
in `ra-core/src/facts.rs`). The planner hook in
`crates/ra-pg-extension/src/planner_hook.rs:130` converts queries to `RelExpr`
and optimizes them, but it has no awareness of extension capabilities. The
`supports_feature` method in `planner_hook.rs:864` lists only core PostgreSQL
features.

This creates three problems:

**1. Missed index opportunities.** PostGIS provides GiST and SP-GiST spatial
indexes, but Ra cannot recommend `CREATE INDEX ... USING GIST (geom)` because
it does not know the column is a geometry type. The `IndexType::Gist` variant
exists in `ra-core/src/facts.rs:117` but is never recommended by the advisor
for spatial workloads. Similarly, pg_trgm enables GIN indexes for fuzzy text
search (`LIKE '%pattern%'`), but Ra has no trigram awareness.

**2. Incorrect cost estimation.** TimescaleDB stores data in compressed
hypertable chunks with different I/O characteristics than regular heap tables.
A sequential scan on a TimescaleDB hypertable may decompress chunks on the fly,
costing 3-5x more CPU than a standard heap scan. The cost model in
`crates/ra-pg-extension/src/cost_mapper.rs` does not account for this.
Citus distributes data across worker nodes, making network I/O the dominant
cost factor -- but `CostCalibration::network_factor` at `cost_mapper.rs:22` is
set to a generic 0.5 heuristic.

**3. Suboptimal join and scan strategies.** Citus requires shard-aware join
planning: co-located joins (where both tables are distributed on the same key)
are local, while cross-shard joins require data redistribution. Without this
knowledge, Ra may choose join orders that cause unnecessary network transfers.
TimescaleDB benefits from chunk exclusion (analogous to partition pruning) for
time-range predicates, but Ra does not generate chunk-aware plans.

**Expected impact by extension:**

| Extension    | Optimization type        | Estimated gain     |
|--------------|--------------------------|--------------------|
| PostGIS      | Spatial index selection   | 10-1000x           |
| TimescaleDB  | Chunk-aware joins         | 5-50x              |
| Citus        | Shard-aware planning      | 10-100x            |
| pg_trgm      | GIN trigram indexes       | 10-100x            |
| hstore       | GIN key existence         | 10-50x             |
| ltree         | GiST hierarchy indexes    | 5-20x              |
| bloom        | Multi-column filter       | 2-10x              |
| btree_gin    | Multicolumn GIN scans     | 2-5x               |
| pg_partman   | Partition-aware planning  | 2-10x              |

## Guide-level explanation

### Extension detection

When Ra's planner hook fires (`planner_hook.rs:46`), the first step after
the existing fast-path checks is to query `pg_extension` for installed
extensions:

```sql
SELECT extname, extversion
FROM pg_extension
WHERE extname IN (
    'postgis', 'timescaledb', 'citus',
    'hstore', 'ltree', 'pg_trgm', 'citext',
    'bloom', 'btree_gin', 'btree_gist',
    'pg_partman', 'pg_cron', 'pg_stat_statements'
);
```

This query runs once per backend connection (not per query) and is cached in
`ExtensionState`. The result enables or disables extension-specific rules
for all subsequent queries in that session.

### PostGIS: spatial index selection

Given this query on a PostGIS-enabled database:

```sql
SELECT name, ST_AsText(geom)
FROM buildings
WHERE ST_DWithin(geom, ST_MakePoint(-73.97, 40.77)::geography, 500);
```

Without extension awareness, Ra treats `geom` as an opaque column and
`ST_DWithin` as a generic function call. With PostGIS awareness:

1. Ra recognizes `geom` as `geography` type (from `pg_attribute.atttypid`
   resolved through `pg_type`).
2. The PostGIS rule set activates, recognizing `ST_DWithin` as a spatial
   predicate that benefits from GiST indexes.
3. If no spatial index exists, Ra recommends:
   `CREATE INDEX idx_buildings_geom ON buildings USING GIST (geom);`
4. The cost model applies spatial index cost factors: GiST lookup cost of
   5.0 (R-tree traversal) plus a recheck cost for the precise distance
   calculation.

### TimescaleDB: chunk-aware optimization

```sql
SELECT time_bucket('1 hour', time) AS bucket,
       device_id,
       avg(temperature)
FROM sensor_data
WHERE time > now() - interval '7 days'
GROUP BY bucket, device_id;
```

With TimescaleDB awareness:

1. Ra detects that `sensor_data` is a hypertable (via
   `_timescaledb_catalog.hypertable`).
2. The time predicate `time > now() - interval '7 days'` triggers chunk
   exclusion: only chunks covering the last 7 days are scanned.
3. If compression is enabled on older chunks, the cost model adjusts:
   compressed chunks have lower I/O cost but higher CPU cost for
   decompression.
4. The `time_bucket` function is recognized as a TimescaleDB aggregate
   optimization opportunity -- if a continuous aggregate exists, Ra
   recommends using it.

### Citus: distributed join planning

```sql
SELECT o.order_id, c.name, sum(oi.quantity)
FROM orders o
JOIN customers c ON o.customer_id = c.customer_id
JOIN order_items oi ON o.order_id = oi.order_id
WHERE o.region = 'US'
GROUP BY o.order_id, c.name;
```

With Citus awareness:

1. Ra queries `pg_dist_partition` to determine distribution columns:
   - `orders` distributed on `customer_id`
   - `customers` distributed on `customer_id`
   - `order_items` distributed on `order_id`
2. The join `orders.customer_id = customers.customer_id` is co-located
   (same distribution key) -- this is a local join on each shard.
3. The join `orders.order_id = order_items.order_id` crosses distribution
   boundaries -- this requires a repartition or broadcast.
4. Ra recommends the join order that maximizes co-located joins first,
   then applies the cross-shard join on a reduced result set.

## Reference-level explanation

### Extension detection API

Extension detection uses direct syscache lookups (no SPI), consistent with
the approach in `crates/ra-pg-extension/src/stats_bridge.rs`. The detection
runs once per backend and is cached.

```rust
use std::collections::HashMap;

/// Detected PostgreSQL extension with version.
#[derive(Debug, Clone)]
pub struct DetectedExtension {
    pub name: String,
    pub version: String,
    pub capabilities: Vec<ExtensionCapability>,
}

/// Capabilities that an extension provides to the optimizer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ExtensionCapability {
    /// Custom data types (e.g., geometry, geography, hstore).
    CustomTypes(Vec<String>),
    /// Custom index access methods (e.g., GiST for spatial).
    IndexMethods(Vec<String>),
    /// Custom operators (e.g., @>, &&, <->).
    Operators(Vec<String>),
    /// Distributed query execution (Citus).
    DistributedExecution,
    /// Time-series chunk management (TimescaleDB).
    ChunkManagement,
    /// Partition management (pg_partman).
    PartitionManagement,
    /// Query statistics tracking (pg_stat_statements).
    QueryStatistics,
    /// Scheduled execution (pg_cron).
    ScheduledExecution,
}

/// Cache of detected extensions per backend.
pub struct ExtensionRegistry {
    extensions: HashMap<String, DetectedExtension>,
    detection_complete: bool,
}

impl ExtensionRegistry {
    pub fn new() -> Self {
        Self {
            extensions: HashMap::new(),
            detection_complete: false,
        }
    }

    /// Detect installed extensions from pg_extension catalog.
    ///
    /// Uses syscache lookup, safe to call from planner hooks.
    ///
    /// # Safety
    ///
    /// Must be called within a PostgreSQL backend process.
    pub unsafe fn detect(&mut self) {
        if self.detection_complete {
            return;
        }
        // Scan pg_extension catalog (similar to stats_bridge
        // catalog access pattern)
        self.extensions = scan_pg_extension();
        self.detection_complete = true;
    }

    pub fn has_extension(&self, name: &str) -> bool {
        self.extensions.contains_key(name)
    }

    pub fn get(&self, name: &str) -> Option<&DetectedExtension> {
        self.extensions.get(name)
    }

    pub fn has_capability(
        &self,
        cap: &ExtensionCapability,
    ) -> bool {
        self.extensions.values().any(|ext| {
            ext.capabilities.contains(cap)
        })
    }
}
```

**Integration with existing code:** The `ExtensionRegistry` is stored
alongside `HARDWARE_PROFILE` in `extension_state.rs` as a `OnceLock`
static. The `_PG_init` function at `lib.rs:31` calls
`extension_state::init_extension_registry()` during shared library load.
The planner hook accesses it via `extension_state::extensions()`.

The `supports_feature` implementation in `planner_hook.rs:864` is extended
to include extension-provided features:

```rust
fn supports_feature(&self, feature: &str) -> bool {
    let extensions = extension_state::extensions();
    match feature {
        // Existing core features
        "lateral_join" | "cte_recursive" | "window_functions"
        | "partial_index" | "index_only_scan" | "bitmap_scan"
        | "parallel_query" | "hash_join" | "merge_join"
        | "nested_loop" => true,

        // Extension-provided features
        "spatial_index" | "spatial_join" =>
            extensions.has_extension("postgis"),
        "hypertable" | "chunk_exclusion" | "continuous_aggregate" =>
            extensions.has_extension("timescaledb"),
        "distributed_query" | "shard_aware_join" =>
            extensions.has_extension("citus"),
        "trigram_index" | "fuzzy_search" =>
            extensions.has_extension("pg_trgm"),
        "hstore_index" =>
            extensions.has_extension("hstore"),
        "ltree_index" | "hierarchy_query" =>
            extensions.has_extension("ltree"),
        "bloom_index" =>
            extensions.has_extension("bloom"),
        "query_stats" =>
            extensions.has_extension("pg_stat_statements"),

        _ => false,
    }
}
```

### Per-extension optimization rules

#### PostGIS (spatial/geographic types and indexes)

**Types:** `geometry`, `geography`, `raster`, `topology`

**Index types:**
- GiST: default spatial index, supports `&&` (bounding box overlap),
  `@` (containment), `~` (contains), `<->` (KNN distance)
- SP-GiST: better for point data with skewed distributions (quad-tree)
- BRIN: effective for spatially sorted data (e.g., sequential inserts
  from a moving sensor)

**Optimization rules:**

Rule 1: **Spatial index recommendation.** When a query uses
`ST_Intersects`, `ST_DWithin`, `ST_Contains`, `ST_Within`, or `ST_Covers`
on a geometry/geography column without a spatial index, recommend GiST.

```
IF column.type IN (geometry, geography)
   AND predicate uses spatial function
   AND no GiST index exists on column
THEN recommend:
  CREATE INDEX idx_{table}_{col}_gist
    ON {table} USING GIST ({col});
```

For geography types specifically, recommend:
```sql
CREATE INDEX idx_{table}_{col}_gist
  ON {table} USING GIST ({col} geography_ops);
```

Rule 2: **GiST vs SP-GiST selection.** For point-only columns
(`geometry(Point, 4326)`), SP-GiST quad-tree indexes outperform GiST
R-trees by 20-40% on KNN queries. Detect point-only columns via
`geometry_typmod_out`:

```
IF column.type = geometry
   AND geometry_subtype = Point
   AND workload is KNN-heavy (uses <-> operator)
THEN recommend SP-GiST over GiST
```

Rule 3: **Spatial function pushdown.** PostGIS functions like
`ST_Distance` are CPU-intensive (coordinate transformation, geodesic
calculations). The cost model assigns:

| Function           | Cost multiplier | Notes                        |
|--------------------|-----------------|------------------------------|
| `ST_Intersects`    | 10.0            | Bounding box + exact check   |
| `ST_DWithin`       | 12.0            | Distance + comparison        |
| `ST_Distance`      | 15.0            | Full geodesic calculation    |
| `ST_Area`          | 8.0             | Polygon area computation     |
| `ST_Transform`     | 20.0            | Coordinate system transform  |
| `ST_Buffer`        | 25.0            | Geometry construction        |

These multipliers apply to the per-row CPU cost in
`estimate_relexpr_cost` at `planner_hook.rs:363`.

Rule 4: **Bounding box pre-filter.** For `ST_Intersects(a, b)`,
PostgreSQL's GiST index performs a bounding box check (`&&`) first,
then a precise intersection test. Ra should model this two-phase cost:

```
spatial_predicate_cost =
    cardinality * bbox_cost                    -- index filter
  + selectivity * cardinality * exact_cost    -- recheck
```

Where `bbox_cost = 0.5` and `exact_cost` depends on the function
(see table above).

#### hstore (key-value store)

**Type:** `hstore` -- stores key-value pairs as a single column value.

**Index types:**
- GIN with `gin_hstore_ops`: supports `@>` (contains), `?` (key exists),
  `?&` (all keys exist), `?|` (any key exists)
- GiST with `gist_hstore_ops`: supports `@>` only, but smaller index

**Optimization rules:**

Rule 1: **Key existence index.** When queries use `?` (key exists) or
`->` (key access) on hstore columns, recommend GIN index:

```sql
CREATE INDEX idx_{table}_{col}_gin
  ON {table} USING GIN ({col});
```

Rule 2: **hstore-to-JSONB migration advisory.** When hstore columns are
used with patterns that JSONB handles better (nested access, array values,
numeric comparisons), emit an advisory. This aligns with RFC 0055
Rule 10 (HSTORE to JSONB migration).

**Cost model:** hstore operations are cheaper than JSONB (flat key-value
vs tree structure). Per-row extraction cost: 0.5x the JSONB equivalent.

#### ltree (hierarchical tree structures)

**Type:** `ltree` -- label tree path like `'Top.Science.Biology'`.

**Index types:**
- GiST with `gist_ltree_ops`: supports `@>` (ancestor), `<@`
  (descendant), `~` (lquery match), `?` (ltxtquery match)

**Optimization rules:**

Rule 1: **Hierarchy index recommendation.** When queries use ltree
operators (`@>`, `<@`, `~`) without a GiST index:

```sql
CREATE INDEX idx_{table}_{col}_gist
  ON {table} USING GIST ({col});
```

Rule 2: **Ancestor/descendant query optimization.** For
`WHERE path @> 'Top.Science'` (find descendants), the GiST index
provides O(log n) lookup. Without the index, this requires a sequential
scan with prefix matching. Cost model assigns GiST lookup cost of 5.0
(same as other GiST indexes).

Rule 3: **Depth-limited queries.** For `WHERE nlevel(path) = 3`, suggest
a functional B-tree index on `nlevel(path)` if the predicate appears
frequently:

```sql
CREATE INDEX idx_{table}_{col}_depth
  ON {table} ((nlevel({col})));
```

#### pg_trgm (trigram matching for fuzzy text search)

**Index types:**
- GIN with `gin_trgm_ops`: supports `LIKE`, `ILIKE`, `~` (regex),
  `%` (similarity), `<->` (word similarity)
- GiST with `gist_trgm_ops`: supports same operators but with
  KNN distance ordering

**Optimization rules:**

Rule 1: **Trigram index for LIKE patterns.** The most common
optimization: `WHERE name LIKE '%smith%'` cannot use a B-tree index
but CAN use a GIN trigram index. When a query uses `LIKE` with a
leading wildcard on a text column:

```
IF column.type IN (text, varchar, citext)
   AND predicate uses LIKE/ILIKE with leading %
   AND pg_trgm is installed
   AND no GIN(gin_trgm_ops) index exists
THEN recommend:
  CREATE INDEX idx_{table}_{col}_trgm
    ON {table} USING GIN ({col} gin_trgm_ops);
```

Rule 2: **Similarity search optimization.** For `WHERE similarity(name, 'john') > 0.3`,
recommend GiST with `gist_trgm_ops` for KNN ordering:

```sql
CREATE INDEX idx_{table}_{col}_trgm_gist
  ON {table} USING GIST ({col} gist_trgm_ops);
```

**Cost model:** Trigram index lookups are more expensive than B-tree
(posting list intersection) but far cheaper than sequential scan with
pattern matching:

| Operation                | Cost without index | Cost with GIN trgm |
|--------------------------|-------------------|---------------------|
| `LIKE '%pattern%'`       | N * 0.01          | log(N) * 3.0        |
| `similarity(col, x) > t` | N * 0.05          | log(N) * 5.0        |

#### citext (case-insensitive text)

**Type:** `citext` -- text type with case-insensitive comparison
semantics.

**Optimization rules:**

Rule 1: **Avoid redundant lower() calls.** When citext columns are
compared with `lower(col) = lower(value)`, the `lower()` calls are
redundant. Ra rewrites to direct comparison:

```
Input:  lower(citext_col) = lower('Value')
Output: citext_col = 'Value'
```

This enables index usage on the citext column directly.

Rule 2: **Index type awareness.** citext supports B-tree indexes with
case-insensitive ordering. No special index type is needed -- standard
B-tree works. Ra should NOT recommend expression indexes on `lower()`
for citext columns (a common but unnecessary pattern).

#### bloom (Bloom filter indexes)

**Index type:** bloom -- probabilistic index supporting equality
comparisons on multiple columns simultaneously.

**Optimization rules:**

Rule 1: **Multi-column equality filter.** When a query filters on
3+ columns simultaneously with equality predicates and no single
B-tree index covers the combination, a bloom index may be appropriate:

```
IF query filters on >= 3 columns with equality
   AND no covering B-tree index exists
   AND bloom extension is installed
   AND table has > 100K rows
THEN recommend:
  CREATE INDEX idx_{table}_bloom
    ON {table} USING BLOOM ({col1}, {col2}, {col3})
    WITH (length=80, col1=2, col2=2, col3=2);
```

**Cost model:** Bloom indexes have false positives (configurable via
`length` parameter). The cost includes a recheck phase:

```
bloom_scan_cost =
    false_positive_rate * cardinality * tuple_fetch_cost
  + cardinality * bloom_check_cost
```

Where `bloom_check_cost = 0.001` (hash computation) and
`false_positive_rate` depends on the `length` parameter (default ~1-2%).

Rule 2: **Bloom vs B-tree tradeoff.** Bloom indexes are smaller than
multi-column B-tree indexes but have false positives and do not support
range queries. Ra recommends bloom only when:
- All predicates are equality checks
- The combination of columns is not frequently queried in subset
  (B-tree prefix scans would be more valuable)
- Table size justifies the overhead

#### btree_gin and btree_gist (additional operator classes)

These extensions add B-tree-equivalent operator classes for GIN and GiST
indexes, enabling multi-column indexes that mix standard types with
extension types.

**btree_gin** supports: int2, int4, int8, float4, float8, numeric, text,
varchar, char, bytea, date, timestamp, timestamptz, time, timetz, money,
oid, uuid, macaddr, inet, cidr, bool.

**btree_gist** supports the same types plus range exclusion constraints.

**Optimization rules:**

Rule 1: **Mixed-type multicolumn index.** When a query filters on both a
standard type and a GIN-indexed type (e.g., JSONB `@>` and `status = 'active'`),
btree_gin enables a single composite GIN index:

```sql
-- With btree_gin installed:
CREATE INDEX idx_users_data_status
  ON users USING GIN (data, status);
-- Single index scan for: data @> '{"key":"val"}' AND status = 'active'
```

Without btree_gin, this requires two separate index scans (GIN on data,
B-tree on status) and a bitmap AND.

Rule 2: **Exclusion constraint support.** btree_gist enables exclusion
constraints on standard types:

```sql
-- Prevent overlapping reservations
ALTER TABLE reservations ADD CONSTRAINT no_overlap
  EXCLUDE USING GIST (room WITH =, period WITH &&);
```

Ra should recognize exclusion constraints and model their enforcement
cost in INSERT/UPDATE planning.

#### Citus (distributed query planning)

**Capabilities:** Distributed tables, reference tables, local tables,
shard-aware join planning, distributed transactions.

**Detection:** Query `pg_dist_partition` for distribution column and
method:

```sql
SELECT logicalrelid::text AS table_name,
       partmethod,        -- 'h' (hash), 'a' (append), 'n' (none)
       partkey            -- distribution column expression
FROM pg_dist_partition;
```

**Optimization rules:**

Rule 1: **Co-located join detection.** When both tables in a join are
distributed on the same column and joined on that column, the join
executes locally on each shard (no network transfer):

```
IF left.distribution_col = right.distribution_col
   AND join_condition includes
       left.distribution_col = right.distribution_col
THEN mark join as co-located (network_cost = 0)
```

Rule 2: **Reference table broadcast.** Citus reference tables are
replicated to all workers. Joins against reference tables are always
local:

```
IF any side is a reference table (partmethod = 'n', replicated)
THEN mark join as local (network_cost = 0)
```

Rule 3: **Repartition join cost.** When tables have different
distribution columns, Citus must repartition one side. Cost model:

```
repartition_cost =
    smaller_side_rows * network_transfer_cost_per_row
  + smaller_side_rows * hash_computation_cost
```

Where `network_transfer_cost_per_row = 0.1` (calibrated per deployment)
and `hash_computation_cost = 0.001`.

Rule 4: **Join order for distribution.** Ra should prefer join orders
that execute co-located joins first, reducing the data volume before
any repartition step. This integrates with the existing join reordering
rules but adds distribution awareness as a tiebreaker.

Rule 5: **Filter pushdown to shards.** Predicates on the distribution
column should be pushed down before the join, enabling shard pruning:

```
IF filter references distribution_column
THEN push filter below join (standard predicate pushdown)
     AND mark as shard-pruning (reduces worker count)
```

#### TimescaleDB (time-series optimizations)

**Capabilities:** Hypertables (auto-partitioned by time), compression,
continuous aggregates, data retention policies.

**Detection:** Query `_timescaledb_catalog.hypertable`:

```sql
SELECT h.table_name,
       d.column_name AS time_column,
       d.partitioning_func
FROM _timescaledb_catalog.hypertable h
JOIN _timescaledb_catalog.dimension d
  ON h.id = d.hypertable_id
WHERE d.column_type = 'timestamptz'::regtype;
```

**Optimization rules:**

Rule 1: **Chunk exclusion.** Time-range predicates prune chunks
(analogous to partition pruning, RFC 0019). Ra models this by reducing
the effective cardinality:

```
IF table is hypertable
   AND predicate on time_column restricts range
THEN effective_rows = total_rows * (query_range / total_range)
     effective_chunks = total_chunks * (query_range / total_range)
```

Rule 2: **Compression-aware cost model.** Compressed chunks have
different I/O characteristics:

| Metric              | Uncompressed | Compressed    |
|---------------------|-------------|---------------|
| Storage size        | 1x          | 5-20x smaller |
| Sequential scan I/O | 1x          | 5-20x less    |
| CPU (decompression) | 0x          | 3-5x more     |
| Random access       | Supported   | Not supported |

Cost adjustment for compressed chunks:

```
compressed_scan_cost =
    io_cost / compression_ratio
  + cpu_cost * decompression_multiplier
```

Rule 3: **Continuous aggregate recommendation.** When Ra detects a
query pattern matching `time_bucket() + aggregate`, and the query
runs frequently (detected via pg_stat_statements if available), emit
an advisory:

```
IF query uses time_bucket(interval, time_col)
   AND query uses aggregate functions
   AND query appears in pg_stat_statements with calls > 100
THEN recommend:
  CREATE MATERIALIZED VIEW {mv_name}
    WITH (timescaledb.continuous) AS
    SELECT time_bucket('{interval}', {time_col}),
           {aggregate_expressions}
    FROM {table}
    GROUP BY 1;
```

Rule 4: **Time-bucket join optimization.** When joining a hypertable
with a regular table on a time column, Ra should prefer nested loop
with chunk index scan over hash join, because chunk exclusion makes
the inner side very selective.

#### pg_partman (partition management)

**Capabilities:** Automated range/list partition creation and
maintenance. Not a query optimizer -- but Ra should recognize
pg_partman-managed partition hierarchies.

**Optimization rule:**

Rule 1: **Partition-aware planning.** When pg_partman manages a table,
the partition structure follows a predictable pattern (time-based
ranges or ID ranges). Ra can use this to predict which partitions
a query will touch:

```
IF table is managed by pg_partman (check part_config table)
   AND predicate restricts partition key
THEN estimate partition count from interval + predicate range
```

This improves cardinality estimation when `pg_class.reltuples` for
individual partitions is stale.

#### pg_cron (scheduled execution)

pg_cron does not affect query planning directly. However, Ra can
recommend pg_cron for maintenance tasks:

- Schedule `ANALYZE` on tables with stale statistics
- Schedule refresh of materialized views (including TimescaleDB
  continuous aggregates)
- Schedule `VACUUM` on tables with high dead tuple ratios
  (detected by `stats_bridge::PostgresMvccStats::needs_vacuum()`
  at `stats_bridge.rs:123`)

**Advisory rule:**

```
IF table statistics are stale (mvcc.is_stale() == true)
   AND pg_cron is installed
THEN recommend:
  SELECT cron.schedule('analyze-{table}',
    '0 2 * * *',
    'ANALYZE {schema}.{table}');
```

#### pg_stat_statements (query performance tracking)

pg_stat_statements provides actual query execution statistics that
Ra can use for feedback-driven optimization (RFC 0026).

**Integration:**

Query `pg_stat_statements` to gather:

```sql
SELECT query,
       calls,
       mean_exec_time,
       stddev_exec_time,
       rows,
       shared_blks_hit,
       shared_blks_read
FROM pg_stat_statements
WHERE dbid = current_database()::oid
ORDER BY total_exec_time DESC
LIMIT 100;
```

This data feeds into:

1. **Cost calibration** (`cost_mapper.rs`): Compare predicted cost vs
   actual execution time to refine `cpu_factor` and `io_factor`.
2. **Workload analysis**: Identify frequently executed queries for
   index recommendation and continuous aggregate suggestions.
3. **Plan regression detection**: When `mean_exec_time` increases
   significantly for a known query, flag it for re-optimization.

### Cost model adjustments

The cost model in `planner_hook.rs:363` (`estimate_relexpr_cost`) is
extended with extension-aware adjustments:

```rust
fn adjust_cost_for_extensions(
    cost: &mut ra_core::Cost,
    table: &str,
    extensions: &ExtensionRegistry,
    table_metadata: &TableMetadata,
) {
    // TimescaleDB: adjust for compression
    if extensions.has_extension("timescaledb") {
        if let Some(compression) = table_metadata.compression_ratio {
            cost.io /= compression;
            cost.cpu *= 3.5; // decompression overhead
        }
    }

    // Citus: add network cost for distributed tables
    if extensions.has_extension("citus") {
        if let Some(dist_info) = table_metadata.distribution {
            if !dist_info.is_local_query {
                cost.network +=
                    cost.io * 0.5; // network transfer estimate
            }
        }
    }

    // PostGIS: adjust for spatial function costs
    // (applied per-predicate, not per-table)
}
```

### Index type cost factors

Extending the cost model with extension-specific index types:

```rust
pub fn index_cost_factors(
    index_type: IndexType,
    extensions: &ExtensionRegistry,
) -> IndexCostFactors {
    match index_type {
        IndexType::BTree => IndexCostFactors {
            lookup_cost: 2.0,
            range_scan_cost: 0.1,
            tuple_fetch_cost: 1.5,
            covering: true,
        },
        IndexType::Gin => IndexCostFactors {
            lookup_cost: 3.0,
            range_scan_cost: 0.5,
            tuple_fetch_cost: 2.0,
            covering: false,
        },
        IndexType::Gist => IndexCostFactors {
            lookup_cost: 5.0,     // R-tree traversal
            range_scan_cost: 1.0,
            tuple_fetch_cost: 2.5, // heap fetch + recheck
            covering: false,
        },
        IndexType::SpGist => IndexCostFactors {
            lookup_cost: 4.0,     // Quad-tree traversal
            range_scan_cost: 0.8,
            tuple_fetch_cost: 2.5,
            covering: false,
        },
        IndexType::Brin => IndexCostFactors {
            lookup_cost: 1.0,     // Range summary scan
            range_scan_cost: 0.05,
            tuple_fetch_cost: 1.0, // May fetch entire range
            covering: false,
        },
        // Bloom (from bloom extension)
        IndexType::Bitmap => IndexCostFactors {
            lookup_cost: 1.5,     // Hash + bloom check
            range_scan_cost: 0.0, // No range scan support
            tuple_fetch_cost: 2.0, // Includes recheck
            covering: false,
        },
        _ => IndexCostFactors::default(),
    }
}
```

### Integration with existing planner hook

The extension detection integrates into the planner hook at
`planner_hook.rs:130` (`ra_planner_hook_inner`). After the existing
relation count check and statistics gathering, the hook consults the
extension registry:

```rust
unsafe fn ra_planner_hook_inner(
    parse: *mut pg_sys::Query,
    query_string: *const std::ffi::c_char,
    cursor_options: i32,
    bound_params: *mut pg_sys::ParamListInfoData,
) -> *mut pg_sys::PlannedStmt {
    // ... existing code: sql extraction, state init,
    //     relation count check, stats gathering ...

    // NEW: Extension-aware optimization context
    let extensions = extension_state::extensions();
    let table_metadata = gather_extension_metadata(
        &table_names, &extensions,
    );

    // Run optimization with extension context
    let result = try_optimize_query(
        parse, &sql, &stats,
        &calibration, &extensions, &table_metadata,
    );

    // ... existing code: confidence check, plan application ...
}
```

### Error handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum ExtensionError {
    #[error(
        "Extension {name} detected but version {version} \
         is below minimum {minimum} for {feature}"
    )]
    VersionTooOld {
        name: String,
        version: String,
        minimum: String,
        feature: String,
    },

    #[error(
        "Extension {name} catalog table {catalog} not found; \
         skipping extension-specific optimization"
    )]
    CatalogMissing {
        name: String,
        catalog: String,
    },

    #[error(
        "Distributed table metadata unavailable for {table}; \
         treating as local table"
    )]
    DistributionUnknown {
        table: String,
    },
}
```

All errors are non-fatal. When extension detection or metadata
gathering fails, Ra falls back to treating tables as standard
PostgreSQL heap tables. Errors are logged at `warn!` level.

## Drawbacks

**Catalog query overhead.** Detecting extensions and gathering
extension-specific metadata (hypertable info, distribution columns)
adds catalog lookups per session. Mitigation: cache aggressively,
detect once per backend, use syscache (not SPI).

**Extension version fragility.** Extensions change catalog schemas
between major versions. TimescaleDB 2.x has a different catalog
structure than 1.x. Citus 12 changed `pg_dist_partition` columns.
Ra must handle version differences or risk crashes on catalog access.

**Maintenance burden.** Each supported extension adds optimization
rules, cost model adjustments, and catalog queries that must be
updated when the extension releases new versions. The initial set
of 12 extensions creates substantial surface area.

**Risk of stale extension cache.** Extensions can be installed or
removed mid-session (`CREATE EXTENSION` / `DROP EXTENSION`). The
per-backend cache does not detect this. Mitigation: honor a GUC
(`ra_planner.refresh_extensions = on`) that forces re-detection,
and auto-refresh on catalog invalidation callbacks if available in
the PostgreSQL version.

**Testing complexity.** Integration tests require running PostgreSQL
instances with each extension installed. CI must maintain Docker images
or pg_regress environments for PostGIS, TimescaleDB, Citus, etc.

## Rationale and alternatives

### Why detect at runtime, not configuration

An alternative is requiring users to declare extensions in a
configuration file. This was rejected because:
- Extensions can be installed per-database, not per-cluster
- Users forget to update configuration when adding extensions
- Runtime detection via `pg_extension` is authoritative and free

### Why per-extension rules, not a generic framework

An alternative is a generic "custom type + custom operator + custom
index" framework where extension authors register capabilities.
This was rejected for the initial implementation because:
- The set of extensions is small and well-known
- Each extension has unique optimization strategies (Citus
  distribution, TimescaleDB chunks) that do not generalize
- A registration framework adds indirection without reducing code

A plugin system can be added later (see Future Possibilities).

### Why not rely on PostgreSQL's built-in extension awareness

PostgreSQL's optimizer already knows about GiST, GIN, and other
index types provided by extensions. However:
- It does NOT rewrite queries to use extension-indexable forms
  (e.g., converting `LIKE '%x%'` to use pg_trgm GIN)
- It does NOT recommend index creation
- It does NOT understand Citus distribution semantics
- It does NOT adjust costs for TimescaleDB compression
- Ra adds value by performing these higher-level optimizations

### Impact of not doing this

- PostGIS users get no spatial index recommendations
- TimescaleDB users get inaccurate cost estimates (compression ignored)
- Citus users get join orders that cause unnecessary network transfers
- pg_trgm users miss GIN index opportunities for LIKE queries
- Ra provides less value than a human DBA who knows these extensions

## Prior art

### Apache Calcite adapter architecture

Calcite provides a pluggable adapter system where each data source
(Cassandra, Elasticsearch, MongoDB, JDBC databases) registers its
own rules, cost model, and physical operators. This is the most
general approach: adapters are self-contained modules that plug into
the optimizer framework.

Ra's approach is less general (PostgreSQL-specific) but more
targeted: we optimize for a small set of well-known extensions
rather than providing a generic adapter interface.

### Presto/Trino connectors

Presto connectors provide metadata about each data source's
capabilities (supports predicates, supports aggregation, supports
LIMIT pushdown). The optimizer uses these capabilities to decide
what operations to push down to the connector vs execute in Presto.

Ra's `ExtensionCapability` enum serves a similar purpose: declaring
what each extension enables so the optimizer can activate the
right rules.

### CockroachDB's geographic awareness

CockroachDB has built-in support for spatial types and indexes
(inspired by PostGIS). Its optimizer natively understands spatial
predicates and can recommend spatial indexes. Ra achieves similar
functionality through extension detection rather than built-in
types.

### Citus optimizer integration

Citus replaces PostgreSQL's planner with its own distributed
planner that understands shard placement. Ra's approach is
complementary: rather than replacing the planner, Ra advises
on join order and filter placement that works well with Citus's
execution model.

### TimescaleDB chunk exclusion

TimescaleDB implements chunk exclusion in its custom scan node
(`ChunkAppend`). Ra's chunk-aware cost model complements this by
providing accurate cardinality estimates to the upstream optimizer,
improving join planning and aggregate strategy selection.

## Unresolved questions

**Design questions:**

1. **Extension version handling.** Should Ra maintain a compatibility
   matrix mapping extension versions to catalog schemas? Or should it
   probe catalog tables and handle `relation not found` errors
   gracefully? The probe approach is more resilient but slower.

2. **Citus coordinator vs worker.** Ra runs as a PostgreSQL extension,
   which means it runs on the coordinator in a Citus cluster. Should
   Ra also run on workers, or is coordinator-only optimization
   sufficient?

3. **TimescaleDB continuous aggregate freshness.** When recommending
   a continuous aggregate, how should Ra handle the freshness tradeoff?
   Continuous aggregates may lag behind the hypertable by the refresh
   interval.

4. **Extension interaction effects.** PostGIS + TimescaleDB (spatial
   time-series) and PostGIS + Citus (distributed spatial) create
   interaction effects where both extensions' rules apply. How should
   rule priority work when multiple extensions are active?

**Implementation questions:**

1. Should extension metadata be stored in `RaOptimizerState`
   (`extension_state.rs:107`) or in a separate per-backend cache?

2. How to test extension-specific rules without installing every
   extension? Options: mock catalog responses, use pg_regress with
   extension-enabled images, or snapshot-based testing.

3. Should the extension registry be exposed to SQL users via a
   diagnostic function (`SELECT * FROM ra_detected_extensions()`)?

**Out of scope:**

- Foreign Data Wrappers (FDW) -- separate RFC
- pg_repack, pg_squeeze (storage optimization) -- maintenance, not planning
- Citus MX (multi-coordinator) -- complex topology beyond initial scope
- PL/pgSQL, PL/Python, PL/Rust extensions -- language extensions
  do not affect query planning

## Future possibilities

### Plugin architecture for third-party extensions

Define a trait that extension authors can implement to register their
capabilities with Ra:

```rust
pub trait ExtensionPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn capabilities(&self) -> Vec<ExtensionCapability>;
    fn custom_rules(&self) -> Vec<Box<dyn OptimizationRule>>;
    fn cost_adjustments(&self) -> Vec<Box<dyn CostAdjustment>>;
    fn index_recommendations(
        &self,
        query: &RelExpr,
    ) -> Vec<IndexRecommendation>;
}
```

This allows extensions outside Ra's built-in set to integrate with
the optimizer.

### Cross-extension optimization

When multiple extensions are active, Ra could detect interaction
patterns:
- PostGIS + TimescaleDB: time-filtered spatial queries with chunk
  exclusion before spatial index scan
- Citus + pg_trgm: distributed fuzzy text search with trigram
  indexes on each shard
- TimescaleDB + pg_stat_statements: use query statistics to
  recommend continuous aggregates for the most expensive time-series
  queries

### Adaptive extension cost calibration

Use pg_stat_statements execution times to calibrate extension-specific
cost factors. For example, if TimescaleDB decompression consistently
takes 5x CPU (not 3.5x as modeled), adjust the multiplier
automatically. This integrates with RFC 0026 (Adaptive Cost
Calibration).

### Extension-aware index advisor

The index advisor (RFC 0021) currently recommends B-tree, GIN, and
GiST indexes. With extension awareness, it can recommend:
- PostGIS SP-GiST for point data
- pg_trgm GIN for fuzzy search patterns
- bloom indexes for multi-column equality filters
- btree_gin composite indexes mixing standard and extension types

This RFC provides the extension detection and capability registry;
the index advisor integration is a follow-on implementation.

### Integration with other RFCs

- **RFC 0002 (pgrx Extension)**: Foundation infrastructure for the
  PostgreSQL extension; this RFC extends its capabilities.
- **RFC 0021 (Automatic Index Advisor)**: Extension-aware index
  recommendations for PostGIS, pg_trgm, bloom, btree_gin.
- **RFC 0026 (Adaptive Cost Calibration)**: Runtime feedback from
  pg_stat_statements to calibrate extension-specific costs.
- **RFC 0055 (RDBMS-Specific Type Support)**: Type definitions for
  PostGIS geometry/geography, hstore, ltree types.
- **RFC 0056 (PostgreSQL Type Optimizations)**: JSONB, TOAST, and
  array optimizations that complement extension-specific rules.
- **RFC 0060 (Genetic Fingerprinting)**: Query fingerprints should
  normalize extension-specific operators.

## Implementation strategy

### Phase 1: Detection and PostGIS

- Implement `ExtensionRegistry` with `pg_extension` catalog scan
- Add PostGIS type detection (geometry, geography)
- Implement spatial index recommendation (GiST)
- Add spatial function cost multipliers
- Integrate with `planner_hook.rs` optimization pipeline
- Test with PostGIS-enabled PostgreSQL instance

### Phase 2: TimescaleDB and Citus

- Detect hypertables and distribution metadata
- Implement chunk exclusion cost model
- Implement co-located join detection for Citus
- Add compression-aware cost adjustments
- Add repartition join cost model

### Phase 3: Text search extensions

- pg_trgm: trigram index recommendation for LIKE queries
- hstore: GIN index recommendation for key operations
- ltree: GiST index recommendation for hierarchy queries
- citext: redundant lower() elimination

### Phase 4: Index extensions and feedback

- bloom: multi-column bloom index recommendation
- btree_gin, btree_gist: composite index recommendations
- pg_stat_statements: cost calibration feedback loop
- pg_partman: partition-aware cardinality estimation
- pg_cron: maintenance scheduling advisories
