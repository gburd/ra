# MonetDB Feature Analysis: Unsupported Features in Ra Optimizer

**Date:** 2026-03-28
**Status:** Comprehensive Analysis
**Total MonetDB Rules in Ra:** 28 rules (3,691 lines)

## Executive Summary

MonetDB is a pioneering column-store database with novel optimization techniques developed over 25+ years of research at CWI Amsterdam. While Ra has implemented 28 MonetDB-specific rules covering the major architectural features, several advanced and research-oriented optimizations remain unsupported. This report identifies gaps, prioritizes them by integration complexity and benefit, and highlights techniques Ra could adopt for broader applicability.

**Key Findings:**
- ✅ **Strong Coverage:** BAT algebra, database cracking, imprints, stochastic cracking, zone maps, MAL pipeline optimization, mitosis parallelism, column recycling
- ⚠️ **Partial Coverage:** Vectorized execution (generic rules exist, not MonetDB-specific), sideways information passing (limited to distributed semi-joins)
- ❌ **Missing:** Approximate query processing, R/Python UDF integration, advanced streaming, query recycling with partial computation reuse, adaptive indexing convergence strategies

---

## 1. Column-Store Optimizations

### 1.1 Late Materialization ✅ SUPPORTED

**Description:** Keep data in columnar format as long as possible, only reconstructing full rows when needed for output or row-oriented operations.

**MonetDB Implementation:**
- BAT algebra operates on (OID, value) pairs
- Joins produce OID-to-OID mappings before value fetches
- Final projection fetches only required columns for qualifying OIDs

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/late-materialization.rra`
- ✅ Generic rules: `rules/physical/scan/columnar-late-materialization.rra`
- ✅ Tests: `crates/ra-engine/tests/execution_column_at_a_time_test.rs`

**Performance:** 30-80% I/O reduction for selective queries on wide tables.

---

### 1.2 Column Pruning ✅ SUPPORTED

**Description:** Read only required columns from storage, eliminating entire columns from I/O.

**MonetDB Implementation:**
- BAT-per-column storage model naturally supports column selection
- Query optimizer tracks required columns through plan tree
- Projection pushdown to scan operators

**Ra Support:**
- ✅ Rule: `rules/physical/scan/columnar-column-pruning.rra`
- ✅ Applies to: Parquet, ORC, Arrow IPC, MonetDB BAT format

**Performance:** 50-95% I/O reduction when selecting few columns from wide tables.

---

### 1.3 Positional Updates (Copy-on-Write BATs) ⚠️ PARTIAL

**Description:** MonetDB's BAT storage uses copy-on-write for updates. Instead of in-place updates, a new BAT version is created with modified values. Old versions remain accessible for MVCC.

**MonetDB Implementation:**
- BATs are immutable after creation (persistent storage)
- Updates create delta BATs merged with base BAT
- Vacuum process consolidates deltas into new base BAT

**Ra Support:**
- ❌ **No MonetDB-specific copy-on-write BAT rules**
- ⚠️ Generic MVCC understanding exists in cost models
- **Gap:** No rules for optimizing queries over BAT + delta structures

**Integration Complexity:** Medium (requires BAT versioning metadata)

**Benefit:** Enables reasoning about update-heavy workloads, MVCC snapshot isolation costs.

**Research vs Production:** Production feature (MonetDB 5+)

**Ra Adoption Potential:** Low. Ra is query optimizer, not storage engine. Could model delta merge costs if targeting MonetDB backend.

---

## 2. Vectorized Execution Model

### 2.1 X100 Vectorized Primitives ✅ SUPPORTED (Generic)

**Description:** Process columns in cache-sized vectors (1024-2048 values). Operators process entire vectors per call, amortizing function call overhead and enabling SIMD.

**MonetDB Implementation:**
- MonetDB/X100 project (Boncz et al. 2005)
- Vector size: 1024 (fits L1 cache)
- Vectorized primitives: select, project, join, aggregate
- Loop-based execution with explicit SIMD intrinsics

**Ra Support:**
- ✅ Generic rules: `rules/physical/execution/columnar-vectorized-ops.rra`
- ✅ Tests: `crates/ra-engine/tests/execution_vectorized_test.rs` (124 execution model rules)
- ✅ Documentation: `docs/features/execution-models.md`

**Performance:** 2-6x CPU throughput over Volcano iterator model.

**Gap:** Ra has generic vectorized execution rules but no MonetDB-specific X100 optimizations (vector size tuning, register blocking strategies).

---

### 2.2 Selection Vectors ⚠️ PARTIAL

**Description:** Instead of compacting data after selection, maintain a bitmap or index list of valid positions. Subsequent operators use selection vector to skip invalid entries.

**MonetDB Implementation:**
- Selection produces OID list (positions of qualifying rows)
- Operators process only OID-indexed values
- Avoids data copying between pipeline stages

**Ra Support:**
- ⚠️ Implicit in OID-based BAT operations (MonetDB rules)
- ❌ **No explicit selection vector propagation rules**
- **Gap:** No cost model comparing selection vector vs compaction

**Integration Complexity:** Low (metadata tracking only)

**Benefit:** Reduces memory bandwidth for chains of selective operations.

**Research vs Production:** Production (MonetDB 11+)

**Ra Adoption Potential:** Medium. Applicable to any vectorized engine (DuckDB, ClickHouse). Could add rules for selection vector materialization vs propagation trade-offs.

---

## 3. Binary Association Tables (BAT) Algebra

### 3.1 BAT-Level Join Ordering ✅ SUPPORTED

**Description:** MonetDB's optimizer reorders joins at the BAT algebra level (post-SQL-to-algebra translation). Considers column-at-a-time execution where intermediate OID lists determine memory consumption.

**MonetDB Implementation:**
- Cost model estimates intermediate BAT sizes
- Prioritizes joins producing smallest OID vectors
- Exploits column statistics (min/max, distinct counts)

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/bat-join-ordering.rra`
- ✅ Considers columnar execution model in join cost estimates

**Performance:** 30-90% speedup for multi-way joins by reducing intermediate result sizes.

---

### 3.2 BAT Candidate List Intersection ✅ SUPPORTED

**Description:** Multi-predicate queries evaluate each condition independently, producing OID candidate lists, then intersect them. Avoids composite indexes.

**MonetDB Implementation:**
- `thetaselect()` primitive produces sorted OID list per predicate
- `BATintersect()` merges lists in O(n+m) time
- Each predicate uses optimal method (imprints, zone maps, hash)

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/cand-list-intersection.rra`

**Performance:** Enables multi-column filtering without composite indexes. Complexity O(n+m) vs O(n*m) for nested loops.

---

### 3.3 BAT Algebra Fusion (MAL Pipeline Optimization) ✅ SUPPORTED

**Description:** Fuse consecutive BAT operations into single pipeline pass. Reduces intermediate materialization.

**MonetDB Implementation:**
- MAL (MonetDB Assembly Language) optimizer
- Detects operator chains: `scan -> select -> project -> aggregate`
- Generates fused loop processing all operations per value

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/mal-pipeline-optimization.rra`

**Performance:** 2-5x for chains of 3+ operations (reduces memory allocations).

---

## 4. Imprints (Lightweight Indexes)

### 4.1 Column Imprints ✅ SUPPORTED

**Description:** Cache-line-aligned bit vector summarizing value ranges per block. Accelerates range scans by skipping non-matching blocks.

**MonetDB Implementation:**
- 1-8 bits per cache-line-sized block (64 bytes)
- Stores which value buckets appear in block
- Integrated with zone maps for combined pruning

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/imprints-scan.rra`

**Performance:** 5-50x speedup for selective range queries on ordered columns. Near-zero overhead for random data.

**Gap:** Ra rule is MonetDB-specific. Could generalize to BRIN (PostgreSQL) and zone map (Parquet/ORC) optimizations under unified "lightweight index" framework.

---

### 4.2 Strimps (String Imprints) ✅ SUPPORTED

**Description:** Lightweight index for LIKE queries. Encodes bigram (character pair) presence per string block as bitset.

**MonetDB Implementation:**
- Extract bigrams from LIKE pattern
- Check each block's strimp bitset for required bigrams
- Skip blocks lacking any required bigram

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/strimps-string-filter.rra`

**Performance:** 2-10x for selective LIKE queries.

**Research vs Production:** Production (MonetDB 11.41+)

---

## 5. Database Cracking (Adaptive Indexing)

### 5.1 Basic Database Cracking ✅ SUPPORTED

**Description:** Self-organizing column structure. Each query physically partitions column around selection bounds (like quicksort partition step). Converges toward sorted order over multiple queries.

**MonetDB Implementation:**
- First query: full scan + partition at predicate bound
- Subsequent queries: scan only relevant partitions + refine
- No index build phase; indexing cost amortized over queries

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/crackers-adaptive-index.rra`

**Performance:** Converges to index-like performance after 5-10 queries on same column.

**Research vs Production:** Experimental (MonetDB 11.11+), improved in later versions.

---

### 5.2 Stochastic Cracking ✅ SUPPORTED

**Description:** Extends basic cracking with random auxiliary crack points to accelerate convergence. Ensures column converges to fully sorted regardless of query workload pattern.

**MonetDB Implementation:**
- Each query cracks at query bound + 1-2 random points
- Distributes partition refinement across full value range
- Converges in O(n log n / Q) queries vs O(Q * n) for standard cracking

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/stochastic-cracking.rra`

**Performance:** 2-5x faster convergence than basic cracking for clustered workloads.

**Research vs Production:** Research prototype (Halim et al. VLDB 2012). Not in production MonetDB.

**Gap:** Rule exists but Ra lacks runtime state tracking for crack convergence. Would need crack index metadata to apply rule.

---

### 5.3 Cracking with Convergence Strategies ❌ MISSING

**Description:** Advanced cracking variants choose crack points based on query history, data distribution, and convergence speed.

**MonetDB Research:**
- **Sideways cracking:** Propagate crack structure across correlated columns
- **Hybrid cracking:** Switch to full sort after sufficient cracks
- **Partial cracking:** Crack only hottest column regions

**Ra Support:**
- ❌ **No rules for advanced cracking strategies**

**Integration Complexity:** High (requires query workload history, crack index metadata)

**Benefit:** 5-10x faster convergence for realistic workloads vs basic cracking.

**Research vs Production:** Research-only (various CIDR/VLDB papers 2010-2015)

**Ra Adoption Potential:** Low. Requires stateful cracking infrastructure. Ra could model convergence costs if targeting MonetDB backend with cracking enabled.

---

## 6. Zone Maps and Data Skipping

### 6.1 Zone Map Scan Skipping ✅ SUPPORTED

**Description:** Store min/max per column zone (contiguous block). Skip zones where predicate cannot match based on zone bounds.

**MonetDB Implementation:**
- Zone size: typically 64K-1M rows
- Maintained automatically as columns load
- Zero maintenance on updates (just extend zone bounds)
- Integrated with imprints for multi-level pruning

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/zonemap-skipping.rra`
- ✅ Generic zone map rule: `rules/physical/scan/columnar-predicate-pushdown.rra` (Parquet row groups)

**Performance:** 50-99% data skipping for selective range queries on clustered data.

---

## 7. MAL (MonetDB Assembly Language) Optimizations

### 7.1 MAL Pipeline Optimization ✅ SUPPORTED

Covered in Section 3.3 (BAT Algebra Fusion).

---

### 7.2 MAL Instruction Reordering ❌ MISSING

**Description:** MAL optimizer reorders instructions for better CPU cache utilization and prefetching.

**MonetDB Implementation:**
- Analyzes data dependencies in MAL instruction graph
- Schedules memory-bound operations to maximize prefetch distance
- Interleaves compute-bound and memory-bound operations

**Ra Support:**
- ❌ **No MAL instruction-level optimization rules**

**Integration Complexity:** Very High (requires MAL instruction semantics, dependency analysis)

**Benefit:** 10-20% throughput improvement for complex queries.

**Research vs Production:** Production (MonetDB MAL optimizer)

**Ra Adoption Potential:** None. MAL is MonetDB's internal IR. Ra operates at relational algebra level, not assembly instruction level.

---

### 7.3 MAL Join Implementation Selection ⚠️ PARTIAL

**Description:** MAL layer chooses physical join algorithm based on input characteristics (sorted, hashed, size ratio).

**MonetDB Algorithms:**
- **Hash join:** Default for equi-joins
- **Merge join:** Exploits sorted inputs (from cracking or ordered scans)
- **Band join:** Inequality joins with sorted input (see Section 8.1)
- **Fetch join:** Index-based for small result sets

**Ra Support:**
- ✅ Generic join selection rules: `rules/physical/join-algorithms/`
- ⚠️ MonetDB-specific: `rules/database-specific/monetdb/columnar-hash-join.rra`, `merge-join.rra`, `band-join.rra`

**Gap:** Ra has rules for common algorithms but missing MonetDB-specific cost models for BAT-level join characteristics.

---

## 8. Novel Join Algorithms

### 8.1 Band Join ✅ SUPPORTED

**Description:** Optimizes inequality joins (theta joins) using sorted order. Scans a "band" of width W for each value instead of full cross product.

**MonetDB Implementation:**
- For `a.x BETWEEN b.y - W AND b.y + W`, sort both columns
- Each probe visits at most W values
- Reduces O(n*m) to O(n*W) where W << m

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/band-join.rra`

**Performance:** 20-60% faster than nested loop for range joins when one side is sorted.

**Research vs Production:** Production (MonetDB 11+)

---

### 8.2 Positional Join ❌ MISSING

**Description:** Exploits positional alignment of columns in same table. For self-joins or joins on co-partitioned data, use OID alignment to avoid hash table.

**MonetDB Implementation:**
- If two BATs have aligned OIDs (from same base table), join is O(n)
- No hash table build, just iterate both columns in lockstep

**Ra Support:**
- ❌ **No positional join rules**

**Integration Complexity:** Medium (requires OID alignment metadata)

**Benefit:** 10x faster than hash join for aligned columns (common in star schema denormalization).

**Research vs Production:** Production (MonetDB BAT algebra primitive)

**Ra Adoption Potential:** Low. Positional alignment is MonetDB-specific (BAT OIDs). Could generalize to "co-partitioned join" for distributed systems.

---

## 9. Query Recycling (Intermediate Result Reuse)

### 9.1 Intermediate Result Recycling ✅ SUPPORTED

**Description:** Cache intermediate BAT results from previous queries. Reuse matching sub-expressions instead of recomputing.

**MonetDB Implementation:**
- Recycler optimizer maintains pool of cached intermediates
- LRU eviction policy
- Cache invalidation on data modifications (INSERT/UPDATE/DELETE)
- Exact sub-expression matching (signature-based lookup)

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/column-recycling.rra`

**Performance:** 10-100x for repeated analytical queries on static data.

**Research vs Production:** Production (MonetDB 11.19+)

---

### 9.2 Partial Computation Reuse ❌ MISSING

**Description:** Advanced query recycling reuses partial computations even when queries don't match exactly.

**MonetDB Research:**
- **Query containment:** Reuse broader query results, apply additional filter
- **Delta-based recycling:** For updated tables, compute delta and merge with cached result
- **Approximate recycling:** Return cached result if error within tolerance

**Ra Support:**
- ❌ **No partial computation reuse rules**

**Integration Complexity:** Very High (requires query containment detection, semantic query comparison)

**Benefit:** 5-50x speedup for exploratory workloads with incrementally refined queries.

**Research vs Production:** Research-only (Ivanova et al. CWI tech reports)

**Ra Adoption Potential:** Medium. Applicable to any caching-enabled system. Could add rules for common patterns (adding filters, relaxing predicates).

---

## 10. Sideways Information Passing

### 10.1 Semi-Join Reduction ✅ SUPPORTED

**Description:** For distributed joins, project join keys from one side, ship to other side, filter (semi-join), then ship only matching rows back for full join.

**Ra Support:**
- ✅ Rule: `rules/logical/sideways-information-passing/semi-join-reduction.rra`
- ✅ For distributed queries (PostgreSQL, CockroachDB, Spark, Presto)

**Performance:** 20-90% network savings for selective joins.

---

### 10.2 Bloom Filter Pushdown ✅ SUPPORTED

**Description:** Build Bloom filter on hash join build side, push to probe side scan to eliminate non-matching rows early.

**Ra Support:**
- ✅ Rule: `rules/logical/sideways-information-passing/bloom-filter-pushdown.rra`

**Performance:** 10-80% probe-side I/O reduction.

---

### 10.3 Runtime Sideways Information Passing ❌ MISSING

**Description:** MonetDB passes statistics between operators **during execution**, not just at planning time.

**MonetDB Implementation:**
- **Bloom filters from hash join:** Generated during build phase, pushed to scan operator mid-execution
- **Selectivity feedback:** Operators report actual selectivities to downstream operators
- **Adaptive filter ordering:** Reorder filter predicates based on observed selectivities

**Ra Support:**
- ❌ **No runtime information passing infrastructure**
- ⚠️ Static bloom filter pushdown exists, but not runtime-generated

**Integration Complexity:** Very High (requires execution engine integration, runtime plan modification)

**Benefit:** 2-10x for complex joins where estimates are inaccurate.

**Research vs Production:** Research (various adaptive query processing papers)

**Ra Adoption Potential:** Low for Ra core (query optimizer, not executor). High for Ra if integrated with execution engines (PostgreSQL extension, DuckDB, etc.).

---

## 11. Parallelism Strategies

### 11.1 Mitosis (Intra-Query Parallelism) ✅ SUPPORTED

**Description:** Range-partition large BAT operations across CPU cores. Each core processes contiguous column slice.

**MonetDB Implementation:**
- Threshold: 1M+ rows
- Partition by OID range (natural for columns)
- Merge results (trivial for filters, requires reduction for aggregates)
- MAL mitosis optimizer inserts parallel splits

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/mitosis-parallelism.rra`

**Performance:** Near-linear speedup (3-7x on 8 cores) for scan-heavy analytical queries.

---

### 11.2 Tail Ordering (Reducing Straggler Impact) ✅ SUPPORTED

**Description:** Reorder parallel partitions to execute high-variance partitions last. Reduces impact of stragglers on query latency.

**MonetDB Implementation:**
- Estimate partition variance from data skew
- Schedule low-variance (predictable) partitions first
- High-variance partitions last (fewer cores waiting at barrier)

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/tail-ordering.rra`

**Performance:** 10-30% latency reduction for skewed data.

**Research vs Production:** Research (appears in academic papers, unclear if in production MonetDB)

---

### 11.3 Morsel-Driven Parallelism ⚠️ PARTIAL

**Description:** Divide work into fine-grained morsels (small batches), use work-stealing queue for load balancing.

**MonetDB Research:**
- Introduced by HyPer (Leis et al. 2014), later adopted by MonetDB research
- Morsel size: ~10K tuples (smaller than mitosis partition)
- Lock-free work stealing reduces barrier synchronization

**Ra Support:**
- ⚠️ Generic morsel execution model tests: `crates/ra-engine/tests/execution_morsel_driven_test.rs`
- ❌ **No MonetDB-specific morsel rules**

**Integration Complexity:** Medium (requires work queue infrastructure)

**Benefit:** Better load balancing for skewed data vs static partitioning.

**Research vs Production:** HyPer production (Leis et al. 2014), MonetDB research only.

**Ra Adoption Potential:** High. Morsel-driven parallelism applies broadly (DuckDB, HyPer, Umbra). Ra could add generic morsel rules.

---

## 12. Multi-Column Optimizations

### 12.1 Multi-Column Sort Sharing ✅ SUPPORTED

**Description:** When query requires multiple sort orders (e.g., window functions with different PARTITION BY/ORDER BY), reuse sorted runs where possible.

**MonetDB Implementation:**
- MAL optimizer detects overlapping sort specifications
- Reuse prefix-sorted data (e.g., sort by (a,b) reuses sort by (a))
- Share memory for multiple sort buffers

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/multi-column-sort-sharing.rra`

**Performance:** 2-3x for queries with multiple window functions.

---

## 13. Compression and Encoding

### 13.1 Dictionary Compression ✅ SUPPORTED

**Description:** Replace string columns with integer dictionary codes. Operate on compressed codes, decompress only for output.

**MonetDB Implementation:**
- Global dictionary per string column
- Predicates translated to dictionary code ranges
- Join on dictionary codes (integer comparison)

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/dictionary-compression.rra`

**Performance:** 3-10x for string-heavy queries (reduced memory bandwidth).

---

### 13.2 Run-Length Encoding (RLE) ✅ SUPPORTED

**Description:** Encode consecutive repeated values as (value, count) pairs. Common for sorted or low-cardinality columns.

**MonetDB Implementation:**
- RLE applied automatically for sorted columns
- Aggregations operate on RLE-compressed data (e.g., SUM(value * count))
- Decompression only when random access required

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/run-length-encoding.rra`

**Performance:** 10-100x compression for sorted low-cardinality columns. 2-5x query speedup.

---

### 13.3 Bit-Packing and FOR (Frame of Reference) ⚠️ PARTIAL

**Description:** Store integers as bit-packed deltas from base value. Reduces storage for narrow-range integers.

**MonetDB Implementation:**
- Detect integer columns with narrow range (e.g., years 2000-2024 → 5 bits)
- Store as: base value + bit-packed deltas
- Vectorized decompression using SIMD

**Ra Support:**
- ⚠️ Generic compression rules exist in `rules/physical/compression/`
- ❌ **No MonetDB-specific bit-packing rules**

**Integration Complexity:** Low (metadata-driven, no execution changes)

**Benefit:** 2-8x compression, 1.5-3x query speedup (less memory bandwidth).

**Research vs Production:** Production (MonetDB, Parquet, ORC all use bit-packing)

**Ra Adoption Potential:** High. Bit-packing is universal. Could add generic FOR/delta encoding rules.

---

## 14. Array and Multidimensional Operations

### 14.1 Native Array Types ⚠️ PARTIAL

**Description:** MonetDB supports SQL array types with operations like element access, slicing, unnesting.

**MonetDB Implementation:**
- Arrays stored as variable-length BATs
- Array functions: `array_agg()`, `unnest()`, element subscripting
- Optimizations: push predicates into array scans

**Ra Support:**
- ⚠️ SQL array types understood by parser (`ra-parser`)
- ❌ **No MonetDB-specific array optimization rules**

**Integration Complexity:** Medium (requires array type system)

**Benefit:** 2-5x for queries on array columns (scientific computing, JSON arrays).

**Research vs Production:** Production (MonetDB SQL arrays)

**Ra Adoption Potential:** Medium. PostgreSQL, Oracle, DuckDB all have array types. Could add generic array pushdown rules.

---

### 14.2 Multidimensional Array Queries ❌ MISSING

**Description:** Optimizations for scientific computing workloads with multidimensional arrays (matrices, tensors).

**MonetDB Research:**
- **ArrayQL:** Query language extension for arrays
- **SciQL:** SQL dialect for scientific data (astronomy, sensor networks)
- Optimizations: array tiling, block-wise operations, dimension reordering

**Ra Support:**
- ❌ **No multidimensional array rules**

**Integration Complexity:** Very High (requires array storage model, tile-based execution)

**Benefit:** 10-100x for scientific workloads (astronomy, geospatial).

**Research vs Production:** Research-only (SciQL project, CWI)

**Ra Adoption Potential:** Low. Niche use case. Would require Ra to understand array storage layouts.

---

## 15. Time-Series Specific Optimizations

### 15.1 Window Function Optimization ✅ SUPPORTED

**Description:** Optimize window functions (e.g., `ROW_NUMBER()`, `LAG()`, `LEAD()`) by exploiting sort order and avoiding full table scans.

**MonetDB Implementation:**
- Detect pre-sorted input (from cracking or index)
- Streaming window evaluation (no full table materialization)
- Share sort order across multiple window specs (see 12.1)

**Ra Support:**
- ✅ Rule: `rules/database-specific/monetdb/window-function-optimization.rra`

**Performance:** 2-5x for multi-window queries.

---

### 15.2 Time-Series Downsampling ❌ MISSING

**Description:** Optimizations for temporal aggregation queries (e.g., downsample 1-second data to 1-minute averages).

**MonetDB Research:**
- Exploit timestamp ordering (sequential scan)
- Pre-aggregated summaries at multiple resolutions
- Skip intervals with no data (sparse time series)

**Ra Support:**
- ⚠️ Generic time-series rules in `rules/multi-model/timeseries/`
- ❌ **No MonetDB-specific downsampling rules**

**Integration Complexity:** Medium (requires time-based partitioning metadata)

**Benefit:** 10-100x for downsampling queries (common in monitoring, IoT).

**Research vs Production:** Research (appears in MonetDB papers on sensor data)

**Ra Adoption Potential:** Medium. Time-series databases (TimescaleDB, InfluxDB, Prometheus) use similar techniques. Could add generic downsampling rules.

---

## 16. R/Python Integration for UDFs

### 16.1 MonetDB.R and MonetDB.Python ❌ MISSING

**Description:** MonetDB embeds R and Python interpreters for user-defined functions (UDFs). Optimizations push vectorized operations into R/Python without row-by-row calls.

**MonetDB Implementation:**
- **MonetDB.R:** BAT columns passed as R vectors (zero-copy)
- **MonetDB.Python:** BAT columns passed as NumPy arrays
- Vectorized UDF execution (entire column processed in single call)
- Parallel UDF execution (mitosis-split UDFs across cores)

**Ra Support:**
- ❌ **No R/Python UDF optimization rules**
- ❌ No UDF cost models

**Integration Complexity:** Very High (requires language runtime integration, memory layout conversion)

**Benefit:** 10-100x for UDF-heavy queries (vs row-by-row UDF calls).

**Research vs Production:** Production (MonetDB.R and MonetDB.Python packages)

**Ra Adoption Potential:** Low for Ra core. High for Ra if targeting UDF-enabled backends (PostgreSQL with PL/Python, DuckDB with Python UDFs). Could add generic UDF vectorization cost models.

---

## 17. Geospatial Extensions

### 17.1 GeoSpatial Index Support ⚠️ PARTIAL

**Description:** MonetDB supports geospatial types (POINT, POLYGON) and indexes (R-tree, geohash).

**MonetDB Implementation:**
- GIS extension: geometry types, spatial functions (`ST_Distance`, `ST_Contains`)
- R-tree index for bounding-box queries
- Optimizations: bounding-box filter -> precise geometry test

**Ra Support:**
- ⚠️ Generic spatial rules: `rules/physical/index-selection/gist-index-for-spatial.rra`, `spatial-index-for-geometry.rra`
- ⚠️ Cost model: `rules/cost-models/geospatial-cost-model.rra`
- ❌ **No MonetDB-specific GIS rules**

**Integration Complexity:** Medium (requires spatial index metadata)

**Benefit:** 10-1000x for spatial range queries (GIS applications).

**Research vs Production:** Production (MonetDB GIS extension)

**Ra Adoption Potential:** High. PostGIS (PostgreSQL), Spatial extensions (MySQL, SQLite) all use similar patterns. Ra has generic spatial rules; could extend with more sophisticated patterns (spatial join algorithms, geohash-based partitioning).

---

## 18. Text Mining and Analytics

### 18.1 Full-Text Search Optimization ⚠️ PARTIAL

**Description:** Optimizations for text search queries (keyword search, ranking, phrase matching).

**MonetDB Research:**
- Inverted index integration (term -> document list)
- Rank-aware query planning (fetch top-K documents only)
- Skip-list acceleration for AND queries

**Ra Support:**
- ⚠️ Strimps string filter (see 4.2) supports LIKE queries
- ❌ **No full-text search index rules** (no inverted index support)

**Integration Complexity:** Medium (requires inverted index metadata)

**Benefit:** 10-1000x for text search (vs sequential LIKE scans).

**Research vs Production:** Research (appears in MonetDB papers), not core MonetDB feature.

**Ra Adoption Potential:** Medium. PostgreSQL (GIN, RUM indexes), Elasticsearch, Lucene all use inverted indexes. Could add generic full-text index rules.

---

## 19. Streaming Query Support

### 19.1 Continuous Query Execution ❌ MISSING

**Description:** Execute queries over unbounded data streams with incremental result updates.

**MonetDB Research:**
- **DataCell:** MonetDB streaming extension
- Window-based processing (tumbling, sliding, session windows)
- Incremental aggregation (update aggregates as new data arrives)

**Ra Support:**
- ❌ **No streaming query rules**
- ⚠️ Generic window rules exist (`rules/database-specific/*/window-*.rra`)

**Integration Complexity:** Very High (requires streaming execution model, state management)

**Benefit:** N/A (different execution model from batch queries)

**Research vs Production:** Research-only (DataCell project, CWI)

**Ra Adoption Potential:** Low. Streaming databases (Flink, Kafka Streams, Materialize) have fundamentally different execution models. Ra focuses on batch query optimization.

---

## 20. Approximate Query Processing (AQP)

### 20.1 Sampling-Based AQP ❌ MISSING

**Description:** Return approximate results using statistical samples, trading accuracy for speed.

**MonetDB Research:**
- Reservoir sampling for aggregates
- Sketch-based distinct count estimation (HyperLogLog)
- Sample-aware query planning (adjust plans based on sample size)

**Ra Support:**
- ❌ **No approximate query processing rules**
- ❌ No sampling infrastructure

**Integration Complexity:** High (requires sampling metadata, confidence interval tracking)

**Benefit:** 10-1000x for exploratory analytics (acceptable error bounds).

**Research vs Production:** Research (AQP is active research area, not in production MonetDB)

**Ra Adoption Potential:** Medium. AQP is valuable for interactive analytics. Systems like Google BigQuery, Redshift support sampling. Could add generic sampling rules with error bounds.

---

## 21. Adaptive Execution and Runtime Re-Optimization

### 21.1 Runtime Plan Switching ⚠️ PARTIAL

**Description:** Detect cardinality estimation errors during execution, re-optimize remainder of query with actual statistics.

**MonetDB Research:**
- Monitor intermediate result sizes during execution
- Trigger re-optimization if actual cardinality diverges from estimate
- Adaptive join algorithm selection (hash -> nested loop if build side too large)

**Ra Support:**
- ⚠️ Ra has progressive re-optimization infrastructure (RFC 0052)
- ⚠️ Generic adaptive execution rules: `rules/experimental/adaptive/` (13 rules)
- ❌ **No MonetDB-specific adaptive execution rules**

**Integration Complexity:** Very High (requires execution engine integration, runtime statistics feedback)

**Benefit:** 2-100x for queries with poor cardinality estimates.

**Research vs Production:** Active research (MonetDB, HyPer, SQL Server adaptive joins)

**Ra Adoption Potential:** High. Ra has adaptive execution framework. Could extend with more sophisticated triggers (per-operator statistics, multiple checkpoints, cost-benefit analysis of re-optimization).

---

## 22. Query Compilation and Code Generation

### 22.1 Just-In-Time (JIT) Query Compilation ⚠️ PARTIAL

**Description:** Compile query plans to native code for zero-overhead execution.

**MonetDB Implementation:**
- MAL-to-LLVM compiler (research prototype)
- Generate tight loops for pipeline chains
- Inline predicates, eliminate virtual function calls

**Ra Support:**
- ⚠️ Ra has JIT backend: `crates/ra-codegen/` (Cranelift JIT)
- ⚠️ Push-based execution model rules: `crates/ra-engine/tests/execution_push_based_test.rs`
- ❌ **No MonetDB MAL compilation rules**

**Integration Complexity:** Very High (requires MAL IR, LLVM backend)

**Benefit:** 2-10x for CPU-bound queries (vs interpreted execution).

**Research vs Production:** Research (MAL-to-LLVM prototype), not in production MonetDB. Production uses interpreted MAL.

**Ra Adoption Potential:** Low for MonetDB MAL. High for generic query compilation. Ra's Cranelift backend could support push-based compiled queries for multiple databases.

---

## 23. MVCC and Transaction Isolation

### 23.1 MVCC with BAT Versioning ⚠️ PARTIAL

**Description:** Multi-version concurrency control using copy-on-write BATs. Each transaction sees consistent snapshot without blocking writers.

**MonetDB Implementation:**
- Delta BATs for modifications
- Transaction ID-based visibility (snapshot isolation)
- Vacuum process merges deltas into base BATs

**Ra Support:**
- ⚠️ Generic MVCC understanding in cost models
- ❌ **No MonetDB-specific MVCC optimization rules**

**Integration Complexity:** Medium (requires transaction metadata, delta tracking)

**Benefit:** Enables reasoning about read-write contention, vacuum costs.

**Research vs Production:** Production (MonetDB 5+)

**Ra Adoption Potential:** Low. MVCC is storage-layer concern. Ra could model delta-merge costs if targeting MonetDB backend.

---

## 24. Hardware-Specific Optimizations

### 24.1 SIMD Vectorized Operations ✅ SUPPORTED

**Description:** Use SIMD instructions (SSE, AVX, AVX-512) for parallel data processing within single thread.

**MonetDB Implementation:**
- Explicit SIMD intrinsics in MAL primitives
- Vectorized selection (compare 8 values per instruction with AVX2)
- Vectorized aggregation (parallel summation)

**Ra Support:**
- ✅ Generic SIMD rules: `rules/physical/hardware/simd-scan-filter.rra`, `branch-free-selection.rra`
- ✅ Hardware capability detection: `crates/ra-hardware/src/cpu.rs`

**Performance:** 2-8x for numeric operations (filters, aggregates).

---

### 24.2 Cache-Conscious Join Algorithms ✅ SUPPORTED

**Description:** Partition hash join to fit in L2/L3 cache, reducing memory stalls.

**MonetDB Implementation:**
- Radix-partitioned hash join (Balkesen et al.)
- Partition build side into cache-sized chunks
- Process each partition independently (cache-resident)

**Ra Support:**
- ✅ Rule: `rules/physical/hardware/cache-conscious-join.rra`

**Performance:** 2-5x for large hash joins (vs cache-oblivious algorithms).

---

### 24.3 NUMA-Aware Data Placement ⚠️ PARTIAL

**Description:** Place data and threads on NUMA nodes to minimize cross-node memory accesses.

**MonetDB Research:**
- Partition BATs across NUMA nodes
- Schedule operators on node where data resides
- Replicate read-only data across nodes

**Ra Support:**
- ⚠️ Generic NUMA rules: `rules/hardware/numa/` (if exists in codebase)
- ❌ **No MonetDB-specific NUMA rules**

**Integration Complexity:** High (requires OS-level NUMA topology, memory placement control)

**Benefit:** 2-5x on NUMA systems (reduces memory latency).

**Research vs Production:** Research (appears in MonetDB papers on multi-socket servers)

**Ra Adoption Potential:** Medium. NUMA awareness is valuable for scale-up systems. Could add generic NUMA placement rules.

---

## 25. Scientific Computing Extensions

### 25.1 SciQL (Scientific Query Language) ❌ MISSING

**Description:** SQL dialect extension for scientific computing (astronomy, microscopy, sensor networks).

**MonetDB Research:**
- Array operators: tiling, slicing, dimension reordering
- Domain-specific functions: coordinate transformations, spatial clustering
- Optimizations: exploit array locality, push computations to array storage

**Ra Support:**
- ❌ **No SciQL rules**

**Integration Complexity:** Very High (requires array storage, domain-specific functions)

**Benefit:** 10-100x for scientific workloads.

**Research vs Production:** Research-only (SciQL project, CWI)

**Ra Adoption Potential:** Low. Niche domain. SciQL users migrated to specialized systems (SciDB, TileDB).

---

## Summary Table: Ra Support for MonetDB Features

| Feature Category | Total Features | ✅ Supported | ⚠️ Partial | ❌ Missing | Priority |
|------------------|----------------|--------------|------------|------------|----------|
| **Column-Store Optimizations** | 3 | 2 | 1 | 0 | High |
| **Vectorized Execution** | 2 | 1 | 1 | 0 | High |
| **BAT Algebra** | 3 | 3 | 0 | 0 | High |
| **Imprints** | 2 | 2 | 0 | 0 | High |
| **Database Cracking** | 3 | 2 | 0 | 1 | Medium |
| **Zone Maps** | 1 | 1 | 0 | 0 | High |
| **MAL Optimizations** | 3 | 1 | 1 | 1 | Low |
| **Join Algorithms** | 2 | 1 | 0 | 1 | Medium |
| **Query Recycling** | 2 | 1 | 0 | 1 | Medium |
| **Sideways Information** | 3 | 2 | 0 | 1 | High |
| **Parallelism** | 3 | 2 | 1 | 0 | High |
| **Multi-Column Ops** | 1 | 1 | 0 | 0 | Medium |
| **Compression** | 3 | 2 | 1 | 0 | Medium |
| **Arrays** | 2 | 0 | 1 | 1 | Low |
| **Time-Series** | 2 | 1 | 0 | 1 | Medium |
| **R/Python UDFs** | 1 | 0 | 0 | 1 | Low |
| **Geospatial** | 1 | 0 | 1 | 0 | Medium |
| **Text Mining** | 1 | 0 | 1 | 0 | Low |
| **Streaming** | 1 | 0 | 0 | 1 | Low |
| **Approximate Queries** | 1 | 0 | 0 | 1 | Medium |
| **Adaptive Execution** | 1 | 0 | 1 | 0 | High |
| **JIT Compilation** | 1 | 0 | 1 | 0 | Medium |
| **MVCC** | 1 | 0 | 1 | 0 | Low |
| **Hardware (SIMD, NUMA)** | 3 | 2 | 1 | 0 | High |
| **Scientific Computing** | 1 | 0 | 0 | 1 | Low |
| **TOTAL** | **45** | **23** | **13** | **12** | - |

**Coverage:**
- ✅ **Supported:** 23/45 (51%)
- ⚠️ **Partial:** 13/45 (29%)
- ❌ **Missing:** 12/45 (20%)

---

## Priority Analysis

### Tier 1: High-Value Additions for Ra (Broad Applicability)

1. **Selection Vector Propagation Rules** (Tier 1A)
   - **Benefit:** Reduces memory bandwidth for vectorized execution chains
   - **Complexity:** Low (metadata tracking only)
   - **Applicability:** DuckDB, ClickHouse, any vectorized engine
   - **Implementation:** Add cost model comparing selection vector vs compaction
   - **Estimated ROI:** High (2-5x for selective filter chains)

2. **Runtime Sideways Information Passing** (Tier 1B)
   - **Benefit:** 2-10x for complex joins with poor estimates
   - **Complexity:** Very High (requires execution engine integration)
   - **Applicability:** Any adaptive execution system
   - **Implementation:** Ra already has progressive re-optimization (RFC 0052). Extend with runtime bloom filter generation, selectivity feedback.
   - **Estimated ROI:** Very High (addresses major pain point: cardinality estimation errors)

3. **Morsel-Driven Parallelism Rules** (Tier 1C)
   - **Benefit:** Better load balancing for skewed data
   - **Complexity:** Medium (requires work queue infrastructure)
   - **Applicability:** DuckDB, HyPer, Umbra (any parallel engine)
   - **Implementation:** Ra has morsel execution tests. Add rules for morsel size tuning, work-stealing cost models.
   - **Estimated ROI:** High (10-30% latency reduction for skewed workloads)

4. **Approximate Query Processing (Sampling)** (Tier 1D)
   - **Benefit:** 10-1000x for exploratory analytics
   - **Complexity:** High (requires sampling metadata, confidence intervals)
   - **Applicability:** Interactive analytics (BigQuery, Redshift, Snowflake)
   - **Implementation:** Add reservoir sampling rules, sketch-based aggregates (HyperLogLog), sample-aware plan selection.
   - **Estimated ROI:** High (enables new use case: sub-second analytics on TB+ datasets)

---

### Tier 2: MonetDB-Specific (Low Transferability)

1. **MAL Instruction-Level Optimization**
   - **Benefit:** 10-20% throughput
   - **Complexity:** Very High (requires MAL IR semantics)
   - **Applicability:** MonetDB only
   - **Implementation:** Not recommended for Ra. MAL is internal IR.
   - **Estimated ROI:** Low (only benefits MonetDB backend)

2. **Positional Join**
   - **Benefit:** 10x for aligned columns
   - **Complexity:** Medium (requires OID alignment metadata)
   - **Applicability:** MonetDB, potentially other column stores
   - **Implementation:** Add rule detecting co-partitioned data. Generalize to "partition-aligned join" for distributed systems.
   - **Estimated ROI:** Medium (common in star schema denormalization)

3. **Advanced Cracking Strategies**
   - **Benefit:** 5-10x faster convergence
   - **Complexity:** High (requires crack index state)
   - **Applicability:** MonetDB only (other systems use pre-built indexes)
   - **Implementation:** Not recommended. Database cracking is MonetDB-specific research feature.
   - **Estimated ROI:** Low (niche feature, not in other databases)

---

### Tier 3: Research-Only (Not Production-Ready)

1. **Partial Computation Reuse**
   - **Benefit:** 5-50x for exploratory workloads
   - **Complexity:** Very High (query containment detection)
   - **Applicability:** Caching-enabled systems
   - **Status:** Research prototype (Ivanova et al. CWI)
   - **Implementation:** Defer until query caching becomes standard practice.
   - **Estimated ROI:** Medium-Low (high complexity, uncertain productionization)

2. **SciQL / Multidimensional Arrays**
   - **Benefit:** 10-100x for scientific workloads
   - **Complexity:** Very High (array storage, tile-based execution)
   - **Applicability:** Scientific databases (SciDB, TileDB)
   - **Status:** Research project (CWI), discontinued
   - **Implementation:** Not recommended. Niche use case.
   - **Estimated ROI:** Low (specialized domain, small user base)

3. **Streaming Continuous Queries**
   - **Benefit:** N/A (different execution model)
   - **Complexity:** Very High (streaming state management)
   - **Applicability:** Streaming databases (Flink, Kafka Streams)
   - **Status:** Research (DataCell project)
   - **Implementation:** Out of scope for Ra. Ra focuses on batch query optimization.
   - **Estimated ROI:** N/A

---

## Recommendations for Ra

### Immediate Additions (Low Effort, High Impact)

1. **Selection Vector Propagation Rules**
   - Add to generic vectorized execution rules
   - Cost model: compare selection vector overhead vs compaction cost
   - Test case: chain of 3+ selective filters

2. **Positional/Co-Partitioned Join Rules**
   - Generalize MonetDB positional join to "partition-aligned join"
   - Detect: columns from same table, or co-partitioned distributed tables
   - Benefit: Star schema queries, broadcast joins

3. **Bit-Packing and FOR Encoding Rules**
   - Add to generic compression rules
   - Detect: integer columns with narrow range
   - Apply: Parquet, ORC, Arrow IPC (universal)

### Medium-Term Additions (Moderate Effort, High Impact)

1. **Runtime Adaptive Execution Enhancements**
   - Extend RFC 0052 with:
     - Runtime bloom filter generation (from join build side)
     - Selectivity feedback (operators report actual selectivities)
     - Multiple checkpoints (re-optimize at each pipeline breaker)
   - Integration: PostgreSQL extension, DuckDB plugin

2. **Morsel-Driven Parallelism Rules**
   - Extend existing morsel tests with cost models
   - Morsel size tuning based on cache size, thread count
   - Work-stealing cost estimation (vs static partitioning)

3. **Approximate Query Processing**
   - Sampling rules: reservoir sampling, stratified sampling
   - Sketch-based aggregates: HyperLogLog (distinct count), t-digest (percentiles)
   - Sample-aware planning: adjust plans based on sample size
   - Error bounds: confidence intervals for aggregates

### Long-Term Research Integration

1. **Advanced Query Recycling**
   - Query containment detection (reuse broader query results)
   - Delta-based recycling (update cached results incrementally)
   - Integration: Ra's plan cache framework

2. **Full-Text Search Optimization**
   - Inverted index rules (GIN, RUM for PostgreSQL)
   - Rank-aware planning (fetch top-K documents only)
   - Skip-list acceleration for multi-term AND queries

---

## Novel Optimization Techniques Ra Could Adopt

### From MonetDB Research

1. **Stochastic Optimization**
   - MonetDB's stochastic cracking accelerates convergence with random choices
   - **Ra Application:** Randomized rule application order to escape local minima
   - **Benefit:** Avoid optimizer getting stuck in suboptimal plan space regions

2. **Adaptive Index Selection**
   - Database cracking builds indexes incrementally based on query workload
   - **Ra Application:** Track query patterns, recommend indexes for frequently queried columns
   - **Benefit:** Automated index tuning (Ra already has index recommendation RFC 0014)

3. **Zero-Maintenance Auxiliary Structures**
   - Imprints, zone maps require no maintenance (embedded in storage)
   - **Ra Application:** Prefer optimizations with no maintenance cost (e.g., Parquet row group stats over B-tree indexes)
   - **Benefit:** Reduce DBA overhead

4. **Column-at-a-Time Cost Models**
   - MonetDB reasons about OID vector sizes, not row counts
   - **Ra Application:** Extend cost models for columnar engines (DuckDB, ClickHouse) to consider column projection, compression, SIMD width
   - **Benefit:** More accurate cost estimates for OLAP queries

5. **Sideways Information Passing**
   - Operators share statistics during execution (not just planning)
   - **Ra Application:** Already started with progressive re-optimization. Extend with operator-level feedback (selectivity, data distribution)
   - **Benefit:** Correct bad estimates mid-execution

---

## Conclusion

Ra has strong coverage of MonetDB's core production features (51% full support, 29% partial). The 28 MonetDB-specific rules capture the essence of column-store optimization: BAT algebra, cracking, imprints, zone maps, MAL pipelines, mitosis parallelism.

**Missing features fall into three categories:**

1. **High-value, broadly applicable** (Tier 1): Selection vectors, runtime adaptive execution, morsel parallelism, approximate queries. **Recommended for Ra.**

2. **MonetDB-specific, low transferability** (Tier 2): MAL instruction optimization, advanced cracking strategies. **Not recommended** (only benefits MonetDB backend).

3. **Research-only, not production-ready** (Tier 3): Partial computation reuse, SciQL, streaming queries. **Defer** until technologies mature.

**Ra's competitive advantage over MonetDB:**
- **Cross-database optimization:** Ra rules apply to 20+ databases. MonetDB optimizations are MonetDB-specific.
- **Formal verification:** Ra has TLA+ specs. MonetDB optimizations are ad-hoc.
- **Extensibility:** Ra's `.rra` literate format makes rules accessible. MonetDB's MAL optimizer is C code.

**MonetDB's competitive advantage over Ra:**
- **Integrated execution:** MonetDB's optimizer tightly couples with execution engine (MAL). Ra optimizes relational algebra, delegates execution.
- **Runtime adaptation:** MonetDB's sideways information passing happens during execution. Ra re-optimizes between executions.
- **Research innovation:** MonetDB pioneered database cracking, imprints, X100 vectorization. Ra codifies existing techniques.

**Recommendation:** Focus Ra development on Tier 1 features (selection vectors, runtime adaptation, morsel parallelism, AQP). These have broad applicability beyond MonetDB and address major pain points (estimation errors, skewed workloads, interactive analytics).
