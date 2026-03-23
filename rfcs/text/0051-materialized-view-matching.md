# RFC 0051: Materialized View Matching and Query Rewriting

- Start Date: 2026-03-22
- Author: Ra Optimizer Team
- Status: Draft
- Tracking Issue: TBD

## Summary

Enable the Ra optimizer to automatically detect when a query can be answered using existing materialized views instead of re-computing from base tables, significantly improving query performance for repeated analytical patterns.

## Motivation

Materialized views (MVs) store pre-computed query results and are a cornerstone of performance optimization in analytical databases. However, their effectiveness depends entirely on the query optimizer's ability to:

1. **Detect opportunities**: Recognize when an incoming query matches or is subsumed by an existing MV
2. **Rewrite queries**: Transform the original query to use the MV with compensation predicates
3. **Choose optimally**: Select the best MV when multiple candidates exist

Without automatic MV matching, users must manually rewrite queries to reference MVs, defeating the purpose of transparent optimization.

### Real-World Impact

**TPC-H Query 5** (revenue by region):
```sql
-- Original query (30+ seconds on 100GB dataset)
SELECT n_name, SUM(l_extendedprice * (1 - l_discount))
FROM customer, orders, lineitem, supplier, nation, region
WHERE ... -- 6-way join
GROUP BY n_name;

-- With MV (0.1 seconds)
CREATE MATERIALIZED VIEW revenue_by_nation AS
SELECT n_name, o_orderdate, SUM(l_extendedprice * (1 - l_discount)) as revenue
FROM customer, orders, lineitem, supplier, nation
WHERE ... -- pre-joined
GROUP BY n_name, o_orderdate;

-- Query automatically rewritten to:
SELECT n_name, SUM(revenue)
FROM revenue_by_nation
WHERE o_orderdate BETWEEN '1994-01-01' AND '1995-01-01'
GROUP BY n_name;
```

**Result:** 300x speedup by avoiding expensive joins.

### Current State

Ra currently has no MV matching capability. Every query starts from base tables, even when perfectly matching MVs exist. This RFC proposes a comprehensive MV matching and rewriting system.

## Guide-level explanation

### How It Works for Users

Users create materialized views as usual:

```sql
CREATE MATERIALIZED VIEW customer_order_summary AS
SELECT
    c_custkey,
    c_name,
    COUNT(*) as order_count,
    SUM(o_totalprice) as total_spent,
    AVG(o_totalprice) as avg_order
FROM customer
JOIN orders ON c_custkey = o_custkey
GROUP BY c_custkey, c_name;
```

Ra automatically detects and uses this MV for matching queries:

```sql
-- Query 1: Exact match
SELECT c_custkey, c_name, COUNT(*), SUM(o_totalprice)
FROM customer JOIN orders ON c_custkey = o_custkey
GROUP BY c_custkey, c_name;
-- Rewritten to: SELECT c_custkey, c_name, order_count, total_spent FROM customer_order_summary;

-- Query 2: Subsumption with compensation predicate
SELECT c_custkey, c_name, COUNT(*), SUM(o_totalprice)
FROM customer JOIN orders ON c_custkey = o_custkey
WHERE o_orderdate >= '2024-01-01'
GROUP BY c_custkey, c_name;
-- Rewritten to:
-- SELECT c_custkey, c_name, COUNT(*), SUM(o_totalprice)
-- FROM orders
-- WHERE o_orderdate >= '2024-01-01'
-- GROUP BY c_custkey, c_name;
-- (Uses MV + delta computation for recent orders)

-- Query 3: Rollup aggregation
SELECT c_custkey, SUM(o_totalprice)
FROM customer JOIN orders ON c_custkey = o_custkey
GROUP BY c_custkey;
-- Rewritten to: SELECT c_custkey, total_spent FROM customer_order_summary;
```

No code changes required. Ra handles detection, rewriting, and cost-based selection automatically.

### Matching Strategies

Ra uses three matching strategies:

1. **Exact Match**: Query identical to MV definition -> direct scan
2. **Query Subsumption**: Query is more general than MV (fewer filters) -> use MV + compensation
3. **View Subsumption**: Query is more specific than MV (extra filters) -> use MV + additional filtering

### Cost-Based Selection

When multiple MVs match, Ra chooses based on:
- **Scan cost**: MV size vs base table size
- **Join elimination**: MVs with pre-joined tables
- **Aggregation savings**: Pre-aggregated data
- **Freshness penalty**: Staleness impacts accuracy

Example:
```sql
-- Two candidate MVs
MV1: SELECT c_custkey, COUNT(*) FROM customer JOIN orders ... (10MB, refreshed hourly)
MV2: SELECT c_custkey, COUNT(*), o_orderdate FROM customer JOIN orders ... (100MB, refreshed daily)

-- Query: SELECT c_custkey, COUNT(*) FROM customer JOIN orders WHERE o_orderdate > '2024-01-01' ...

-- Ra chooses MV1 (smaller, fresher) even though it requires delta computation
```

## Reference-level explanation

### Architecture

```
,----------------------------------------------------,
|          Incoming Query (RelExpr)                |
`-----------------+-------------------------------------'
               |
               v
,----------------------------------------------------,
|      MV Catalog (metadata provider)              |
|  - MV definitions (RelExpr)                      |
|  - Statistics (row count, freshness)             |
|  - Dependencies (base tables, columns)           |
`-----------------+-------------------------------------'
               |
               v
,----------------------------------------------------,
|       Pattern Matcher                            |
|  - Structural equivalence check                  |
|  - Subsumption detection (lattice)               |
|  - Compensation predicate extraction             |
`-----------------+-------------------------------------'
               |
               v
,----------------------------------------------------,
|      Rewrite Candidates                          |
|  - Original plan                                 |
|  - MV-based plan(s)                              |
|  - Hybrid plans (MV + delta)                     |
`-----------------+-------------------------------------'
               |
               v
,----------------------------------------------------,
|       Cost Model                                 |
|  - Estimate each candidate                       |
|  - Account for staleness penalty                 |
|  - Choose lowest-cost plan                       |
`-----------------+-------------------------------------'
               |
               v
        Optimized Plan
```

### MV Metadata

Ra extends the `Statistics` provider with MV metadata:

```rust
pub struct MaterializedViewInfo {
    pub name: String,
    pub definition: RelExpr,  // Original query defining the MV
    pub base_tables: Vec<String>,
    pub row_count: u64,
    pub freshness: SystemTime,  // Last refresh time
    pub staleness_bound: Option<Duration>,  // Max acceptable staleness
    pub is_incremental: bool,  // Supports incremental updates
}

pub trait MaterializedViewProvider {
    fn list_mvs(&self) -> Vec<MaterializedViewInfo>;
    fn get_mv(&self, name: &str) -> Option<MaterializedViewInfo>;
}
```

### Pattern Matching

Matching uses structural comparison of `RelExpr` trees:

```rust
pub enum MatchType {
    Exact,           // Query == MV
    QuerySubsumes,   // Query $\supseteq$ MV (query more general)
    ViewSubsumes,    // MV $\supseteq$ Query (MV more general)
    NoMatch,
}

pub struct Match {
    pub mv_name: String,
    pub match_type: MatchType,
    pub compensation: Option<RelExpr>,  // Additional operations needed
}

pub fn match_query_with_mv(
    query: &RelExpr,
    mv: &MaterializedViewInfo,
) -> Option<Match> {
    // 1. Check structural compatibility
    if !is_structurally_compatible(query, &mv.definition) {
        return None;
    }

    // 2. Compare predicates
    let pred_comparison = compare_predicates(
        extract_predicates(query),
        extract_predicates(&mv.definition),
    );

    // 3. Compare projections and aggregations
    let proj_comparison = compare_projections(query, &mv.definition);

    // 4. Determine match type and compensation
    match (pred_comparison, proj_comparison) {
        (PredicateMatch::Exact, ProjectionMatch::Exact) => {
            Some(Match {
                mv_name: mv.name.clone(),
                match_type: MatchType::Exact,
                compensation: None,
            })
        }
        (PredicateMatch::QueryStronger, _) => {
            // Query has extra filters; use MV + apply filters
            Some(Match {
                mv_name: mv.name.clone(),
                match_type: MatchType::ViewSubsumes,
                compensation: Some(build_filter_compensation(query, &mv.definition)),
            })
        }
        (PredicateMatch::ViewStronger, _) => {
            // MV has extra filters; need delta computation
            Some(Match {
                mv_name: mv.name.clone(),
                match_type: MatchType::QuerySubsumes,
                compensation: Some(build_delta_compensation(query, &mv.definition)),
            })
        }
        _ => None,
    }
}
```

### Rewrite Rules

MVmatching is implemented as egg rewrite rules:

```rust
// MV exact match
"mv-exact-match" => {
    (Aggregate ?groupby ?aggs (Join ?join_type ?cond ?left ?right))
    =>
    (Scan (mv-scan ?mv_name))
    if (mv-exactly-matches ?query ?mv_name)
}

// MV with compensation filter
"mv-compensate-filter" => {
    (Filter ?pred (Aggregate ?groupby ?aggs ?input))
    =>
    (Filter ?compensation (Scan (mv-scan ?mv_name)))
    if (mv-view-subsumes ?input ?mv_name ?compensation)
}

// MV with rollup aggregation
"mv-rollup-aggregate" => {
    (Aggregate ?coarse_groupby ?coarse_aggs ?input)
    =>
    (Aggregate ?coarse_groupby ?rollup_aggs (Scan (mv-scan ?mv_name)))
    if (mv-supports-rollup ?input ?mv_name ?coarse_groupby)
}
```

### Cost Model Integration

MVs affect cost estimation:

```rust
pub fn estimate_mv_cost(
    mv: &MaterializedViewInfo,
    compensation: Option<&RelExpr>,
    query_card: f64,
) -> f64 {
    let base_cost = mv.row_count as f64 * SCAN_COST;

    let compensation_cost = match compensation {
        Some(comp) => estimate_cost(comp),
        None => 0.0,
    };

    let staleness_penalty = if let Some(bound) = mv.staleness_bound {
        let age = SystemTime::now().duration_since(mv.freshness).unwrap();
        if age > bound {
            // Penalize stale MVs
            (age.as_secs() as f64 / bound.as_secs() as f64) * base_cost * 0.2
        } else {
            0.0
        }
    } else {
        0.0
    };

    base_cost + compensation_cost + staleness_penalty
}
```

### Incremental MV Updates

For incrementally maintainable MVs, Ra can use **delta queries**:

```sql
-- MV definition
CREATE MATERIALIZED VIEW revenue_summary AS
SELECT product_id, SUM(amount) as total_revenue
FROM sales GROUP BY product_id;

-- Query needs data from last hour
SELECT product_id, SUM(amount)
FROM sales
WHERE sale_time >= NOW() - INTERVAL '1 hour'
GROUP BY product_id;

-- Ra rewrites as:
-- (MV scan) UNION ALL (delta computation for recent data)
SELECT product_id, total_revenue
FROM revenue_summary
UNION ALL
SELECT product_id, SUM(amount)
FROM sales
WHERE sale_time >= (SELECT MAX(last_refresh) FROM mv_metadata WHERE mv_name = 'revenue_summary')
GROUP BY product_id;
```

## Drawbacks

1. **Complexity**: MV matching adds significant complexity to the optimizer
2. **Correctness risk**: Incorrect rewrites can produce wrong results
3. **Performance overhead**: Pattern matching can be expensive for large MV catalogs
4. **Staleness tracking**: Requires metadata about MV freshness

## Rationale and alternatives

### Why This Design?

1. **Separation of concerns**: MV metadata is separate from core optimizer
2. **Extensibility**: Pattern matcher can be extended with new strategies
3. **Cost-based**: Multiple candidates are compared, not just first match

### Alternatives Considered

**Alternative 1: Heuristic-based matching (no cost model)**
- Simpler but suboptimal when multiple MVs match
- Chosen approach: Always estimate cost

**Alternative 2: User hints (`/*+ USE_MV(name) */`)**
- Gives users control but defeats transparency
- Chosen approach: Automatic + optional hints

**Alternative 3: PostgreSQL's rule-based rewrite**
- PostgreSQL uses query rewrite rules (pg_rewrite)
- Limited to exact matches, no subsumption
- Chosen approach: Lattice-based subsumption + egg rules

## Prior art

### PostgreSQL
- `pg_rewrite` rules for simple view expansion
- No automatic MV matching
- Users manually rewrite queries

### Oracle
- `QUERY_REWRITE_ENABLED` parameter
- Extensive MV rewriting with compensation predicates
- Cost-based selection among multiple MVs

### Apache Calcite
- `AbstractMaterializedViewRule` for lattice-based matching
- Supports rollup, drill-down, and predicate compensation
- Used by Apache Drill, Apache Flink

### Research Papers
- **"Answering Queries Using Views"** (Halevy, 2001) - Subsumption algorithms
- **"Optimizing Queries Using Materialized Views"** (Chaudhuri & Shim, 1999) - Compensation predicates
- **"Self-Tuning Database Systems"** (Chaudhuri & Narasayya, 2007) - Automatic MV selection

## Unresolved questions

1. **Multi-query optimization**: Can we create MVs on-the-fly for repeated patterns?
2. **Distributed MVs**: How do we handle MVs partitioned across nodes?
3. **MV invalidation**: How do we detect when base tables change?
4. **Partial matches**: Should we support rewriting part of a query with an MV?

## Future possibilities

1. **Automatic MV recommendation**: Analyze query workload and suggest MVs
2. **Hybrid execution**: Compute part of query from MV, part from base tables
3. **MV consolidation**: Merge multiple small MVs into one large MV
4. **Adaptive refresh**: Adjust MV refresh frequency based on query patterns

## Implementation plan

### Phase 1: MV Metadata (2 weeks)
- Extend `Statistics` with `MaterializedViewProvider`
- Implement PostgreSQL MV catalog reader
- Add MV metadata to facts provider

### Phase 2: Exact Matching (2 weeks)
- Implement structural equivalence checker
- Add exact-match rewrite rule
- Test with simple aggregation queries

### Phase 3: Subsumption (3 weeks)
- Implement predicate lattice comparison
- Add compensation filter rewrite
- Add rollup aggregation rewrite
- Test with TPC-H queries

### Phase 4: Cost Model (1 week)
- Integrate MV cost estimation
- Add staleness penalty
- Benchmark against PostgreSQL

### Phase 5: Delta Queries (2 weeks)
- Implement incremental maintenance detection
- Add delta computation rewrite
- Test with streaming workloads

**Total:** 10 weeks

## Success Metrics

- **Coverage**: 80% of TPC-H queries use MVs when available
- **Performance**: 10x average speedup on MV-eligible queries
- **Correctness**: 100% result accuracy (verified by integration tests)
- **Overhead**: <5% optimizer time increase when no MVs exist
