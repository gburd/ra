# Databricks/Spark SQL Features - Executive Summary

**Date**: 2026-03-28
**Full Report**: DATABRICKS_SPARK_FEATURES_ANALYSIS.md

---

## Overview

Ra optimizer provides extensive SQL optimization capabilities with 1,327+ transformation rules, but lacks native support for Databricks-specific and Spark SQL extensions. This summary identifies key feature gaps and integration priorities.

---

## Feature Categories and Support Status

### ✅ Fully Supported (Ra Compatible)

1. **Dynamic Partition Pruning** - Runtime partition filtering, Spark-compatible
2. **Bloom Filter Pushdown** - Runtime join filters with probabilistic data structures
3. **Parquet Optimization** - Row group filtering, min/max statistics, column pruning
4. **Distributed Joins** - Broadcast, shuffle, colocated strategies with skew handling
5. **Multi-Phase Aggregations** - Two and three-phase distributed aggregation
6. **Adaptive Execution** - Progressive reoptimization with runtime plan switching

### ⚠️ Partially Supported

1. **Materialized Views** - MV matching and rewriting exist, missing Databricks refresh
2. **Constraints** - PostgreSQL constraint optimization, not Delta Lake constraints
3. **Unnesting** - UnnestExecutor exists, missing LATERAL VIEW syntax
4. **Query Hints** - Join/aggregation strategies exist, missing hint parsing

### ❌ Not Supported (Major Gaps)

#### Delta Lake Core (13 features)
- MERGE INTO upsert operations
- Time travel (AS OF queries, RESTORE)
- OPTIMIZE and Z-ORDER compaction
- Liquid clustering
- Change data feed (CDF)
- Generated columns
- Identity columns
- Delta-specific constraints
- VACUUM, CLONE operations
- UniForm (multi-format support)

#### Language Extensions (15+ features)
- Higher-order functions (transform, filter, aggregate)
- Lambda expressions
- PIVOT/UNPIVOT operations
- LATERAL VIEW (deprecated but still used)
- QUALIFY clause
- SQL scripting (IF, FOR, WHILE)
- Named parameters and defaults
- COPY INTO bulk loading

#### Execution Engines
- Photon vectorized engine (proprietary)
- Vectorized UDFs (Pandas UDFs)
- Streaming tables

#### Unity Catalog
- Three-level namespace (catalog.schema.table)
- Row/column-level security
- External locations and credentials

---

## Integration Complexity Matrix

| Feature | Value | Complexity | Priority |
|---------|-------|------------|----------|
| PIVOT/UNPIVOT | High | Low | **High** |
| QUALIFY Clause | Medium | Low | **High** |
| Named Parameters | Medium | Low | **High** |
| Hint Parsing | High | Low-Med | **High** |
| MERGE INTO | Very High | High | Medium |
| Time Travel | High | Medium | Medium |
| Generated Columns | Medium | Medium | Medium |
| Unity Catalog Security | High | High | Medium |
| Higher-Order Functions | Medium | Very High | Low |
| Photon Costs | High | Very High | Low |
| Liquid Clustering | Medium | Very High | Low |
| SQL Scripting | Low | Very High | Low |

---

## Quick Wins (High Value, Low Complexity)

### 1. PIVOT/UNPIVOT Rewriting
**Benefit**: Common operation in analytics workloads
**Approach**: Rewrite to GROUP BY + filtered aggregations
**Effort**: 2-3 weeks

```sql
-- Before (PIVOT)
SELECT * FROM sales PIVOT (SUM(amount) FOR quarter IN ('Q1', 'Q2', 'Q3', 'Q4'))

-- After (Rewrite)
SELECT
  SUM(CASE WHEN quarter = 'Q1' THEN amount END) AS Q1,
  SUM(CASE WHEN quarter = 'Q2' THEN amount END) AS Q2,
  SUM(CASE WHEN quarter = 'Q3' THEN amount END) AS Q3,
  SUM(CASE WHEN quarter = 'Q4' THEN amount END) AS Q4
FROM sales
GROUP BY ...
```

### 2. QUALIFY Clause
**Benefit**: Cleaner window function queries
**Approach**: Rewrite to subquery with WHERE clause
**Effort**: 1 week

```sql
-- Before (QUALIFY)
SELECT name, RANK() OVER (ORDER BY salary DESC) as rank
FROM employees
QUALIFY rank <= 10

-- After (Rewrite)
SELECT * FROM (
  SELECT name, RANK() OVER (ORDER BY salary DESC) as rank
  FROM employees
) WHERE rank <= 10
```

### 3. Query Hints
**Benefit**: Better control over join strategies
**Approach**: Parse hints, map to existing distribution strategies
**Effort**: 2 weeks

**Ra already has**:
- Broadcast join rules
- Shuffle join strategies
- Skew handling

**Just needs**: Hint parser and enforcement layer

### 4. Named Parameters
**Benefit**: Improved readability and API compatibility
**Approach**: Parser extension for function calls
**Effort**: 1 week

---

## Medium-Term Opportunities

### MERGE INTO Optimization
**Challenge**: Requires ACID transaction support
**Approach**:
1. Model as multi-operation plan with cost estimation
2. Add preconditions for partition-based filtering
3. Implement source deduplication rule
4. Create cost model: `incremental_cost vs full_rewrite_cost`

**Benefit**: Core Delta Lake feature with high demand

### Time Travel Cost Modeling
**Challenge**: Requires versioned metadata
**Approach**:
1. Add version operator to RelExpr
2. Extend cost model for historical scans
3. Optimize metadata-only queries
4. Add retention-aware pruning

**Benefit**: Valuable for auditing and debugging

---

## Ra Architecture Strengths

Ra's design provides excellent foundation for Databricks integration:

1. **E-Graph Optimization** - Can represent multiple equivalent plans
2. **Rule-Based Rewrites** - Extensible for Delta Lake transformations
3. **Cost Modeling** - Supports custom cost functions per feature
4. **Precondition System** - Can model Delta Lake constraints
5. **Distributed Rules** - Already Spark-compatible (partition pruning, bloom filters)

---

## Strategic Recommendations

### Phase 1: Quick Wins (1-2 months)
- ✅ PIVOT/UNPIVOT rewriting
- ✅ QUALIFY clause support
- ✅ Query hint parsing
- ✅ Named parameters

**Impact**: Improved Databricks SQL compatibility with minimal effort

### Phase 2: Delta Lake Core (3-6 months)
- MERGE INTO optimization
- Time travel cost modeling
- Generated column support
- Delta constraint integration

**Impact**: Core Delta Lake optimization capability

### Phase 3: Advanced Features (6-12 months)
- Higher-order function IR
- Lambda expression optimization
- Unity Catalog adapter
- Adaptive clustering research

**Impact**: Full Databricks feature parity

### Research Track (Ongoing)
- ML-driven data layout optimization
- Multi-format query optimization (Delta/Iceberg/Hudi)
- Automatic hint learning from workload
- CDF-driven incremental maintenance

---

## Databricks-Specific Optimization Opportunities

### 1. Z-ORDER Locality Modeling
Extend Ra's cost model to account for Z-ORDER data co-location:
```rust
fn scan_cost_with_zorder(
    rows: u64,
    filter_columns: &[String],
    zorder_columns: &[String],
) -> f64 {
    let overlap = filter_columns.intersection(zorder_columns);
    let locality_factor = 0.3_f64.powi(overlap.len());
    rows as f64 * locality_factor
}
```

### 2. Liquid Clustering Adaptation
Research opportunity: Learn optimal clustering keys from query workload
- Track filter and join columns
- Weight by query frequency
- Automatically recommend clustering changes

### 3. MERGE Partition Pruning
Optimize MERGE operations using time-window filters:
```sql
-- Original (expensive)
MERGE INTO large_table target
USING updates source
ON target.id = source.id
WHEN MATCHED THEN UPDATE SET *

-- Optimized (with precondition)
MERGE INTO large_table target
USING updates source
ON target.id = source.id
  AND target.date >= current_date() - 7  -- Prune partitions
WHEN MATCHED THEN UPDATE SET *
```

**Ra Rule**:
```rust
rw!("merge-partition-prune";
    "(merge ?target ?source ?on ?actions)" =>
    "(merge ?target ?source
       (and ?on (partition_filter ?target)) ?actions)"
    if can_add_partition_filter("?target", "?source")
)
```

---

## Performance Benchmarks to Add

Once Databricks features are integrated, add these benchmarks:

1. **MERGE Operations**
   - Small update vs full refresh
   - Partition-pruned merge vs full scan
   - Deduplication overhead

2. **Time Travel**
   - Recent version (hot cache) vs old version
   - Metadata-only vs data-reading queries

3. **Higher-Order Functions**
   - transform() vs explode+map+aggregate
   - filter() vs WHERE clause equivalent

4. **PIVOT Performance**
   - Native PIVOT vs rewritten GROUP BY
   - Column count impact (5 vs 50 vs 500 columns)

---

## Conclusion

Ra provides strong foundation for Databricks/Spark SQL optimization with existing distributed query capabilities and adaptive execution. Key priorities:

**Immediate** (1-2 months):
- PIVOT/UNPIVOT, QUALIFY, hints, named parameters
- Low complexity, high compatibility impact

**Near-term** (3-6 months):
- MERGE optimization, time travel, generated columns
- Core Delta Lake capabilities

**Long-term** (6-12+ months):
- Higher-order functions, Unity Catalog, streaming
- Research: adaptive clustering, multi-format optimization

**Estimated Total Effort**: 6-9 person-months for Phase 1-2, ongoing for Phase 3 and research.

---

## References

- **Full Analysis**: See DATABRICKS_SPARK_FEATURES_ANALYSIS.md for detailed feature descriptions
- **Ra Codebase**: Existing distributed rules in `/home/gburd/ws/ra/rules/distributed/`
- **Databricks Docs**: https://docs.databricks.com/
- **Apache Spark SQL**: https://spark.apache.org/docs/latest/sql-ref-syntax.html
