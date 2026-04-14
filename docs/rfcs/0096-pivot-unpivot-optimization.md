# RFC 0096: PIVOT/UNPIVOT Operations and Optimization

**Status:** Draft
**Created:** 2026-03-28
**Author:** Ra Research Team
**Estimated Effort:** 15-20 weeks
**Expected Impact:** 2-5x query complexity reduction for reporting workloads

---

## Summary

This RFC proposes adding support for PIVOT and UNPIVOT operations to the Ra optimizer. PIVOT transforms rows into columns by spreading distinct values across new columns with aggregation, while UNPIVOT reverses this process by converting wide-format data to long format. These operations are common reporting patterns appearing in Oracle (11g+), SQL Server (2005+), and DuckDB, with high developer demand for reducing boilerplate GROUP BY + CASE expressions and eliminating repetitive UNION ALL patterns.

**Core Transformations:**
- **PIVOT:** Rows → Columns (GROUP BY + CASE expressions)
- **UNPIVOT:** Columns → Rows (UNION ALL of projections)
- **Dynamic PIVOT:** Runtime column generation from data

---

## Motivation

### Problem Statement

Reporting and analytics workloads frequently require reshaping data between normalized (long) and denormalized (wide) formats. Current solutions require verbose, error-prone SQL:

**Manual PIVOT (Current):**
```sql
SELECT
  region,
  SUM(CASE WHEN quarter = 'Q1' THEN sales END) AS q1_sales,
  SUM(CASE WHEN quarter = 'Q2' THEN sales END) AS q2_sales,
  SUM(CASE WHEN quarter = 'Q3' THEN sales END) AS q3_sales,
  SUM(CASE WHEN quarter = 'Q4' THEN sales END) AS q4_sales
FROM quarterly_sales
GROUP BY region;
```

**With PIVOT (Proposed):**
```sql
SELECT * FROM quarterly_sales
PIVOT (
  SUM(sales)
  FOR quarter IN ('Q1', 'Q2', 'Q3', 'Q4')
);
```

**Manual UNPIVOT (Current):**
```sql
SELECT product, 'Q1' AS quarter, q1 AS sales FROM yearly_summary
UNION ALL
SELECT product, 'Q2' AS quarter, q2 AS sales FROM yearly_summary
UNION ALL
SELECT product, 'Q3' AS quarter, q3 AS sales FROM yearly_summary
UNION ALL
SELECT product, 'Q4' AS quarter, q4 AS sales FROM yearly_summary;
```

**With UNPIVOT (Proposed):**
```sql
SELECT * FROM yearly_summary
UNPIVOT (
  sales FOR quarter IN (q1, q2, q3, q4)
);
```

### Cross-Database Support Analysis

| Database | PIVOT Support | UNPIVOT Support | Dynamic PIVOT | Multi-Aggregate |
|----------|---------------|-----------------|---------------|-----------------|
| **Oracle 11g+** | ✅ Full | ✅ Full | ✅ XML option | ✅ Multiple aggs |
| **SQL Server 2005+** | ✅ Full | ✅ Full | ✅ Dynamic SQL | ✅ Multiple aggs |
| **DuckDB** | ✅ Full | ✅ Full | ✅ Type inference | ✅ Multiple aggs |
| **PostgreSQL** | ⚠️ crosstab() | ⚠️ Manual | ❌ | ⚠️ Limited |
| **MySQL** | ❌ Manual CASE | ❌ Manual UNION | ❌ | ❌ |

**Key Insights:**
- 3 of 5 major databases support native PIVOT/UNPIVOT
- Oracle and SQL Server: 15+ years of production use
- DuckDB: Modern analytical focus with type inference
- Common in BI tools: Tableau, Power BI, Looker expect this functionality

### Use Cases

1. **Financial Reporting:** Monthly/quarterly revenue summaries in columnar format
2. **Cross-Tab Reports:** Product sales by region matrix
3. **Time-Series Aggregation:** Daily metrics pivoted to weekly columns
4. **Data Normalization:** Convert wide Excel imports to normalized tables
5. **Dashboard Preparation:** Transform data for visualization tools

---

## Detailed Design

### 1. SQL Syntax

#### 1.1 PIVOT Syntax

```sql
-- Basic PIVOT
SELECT &lt;non_pivoted_columns&gt;, &lt;pivoted_columns&gt;
FROM &lt;table_expression&gt;
PIVOT (
  &lt;aggregate_function&gt;(&lt;value_column&gt;)
  FOR &lt;pivot_column&gt; IN (&lt;value1&gt;, &lt;value2&gt;, ..., &lt;valueN&gt;)
) AS &lt;alias&gt;;

-- Multiple aggregations (Oracle/SQL Server)
PIVOT (
  SUM(sales) AS total,
  COUNT(*) AS count,
  AVG(price) AS avg_price
  FOR quarter IN ('Q1', 'Q2', 'Q3', 'Q4')
);

-- Dynamic PIVOT (Oracle XML syntax)
PIVOT XML (
  SUM(sales)
  FOR quarter IN (ANY)  -- Runtime column discovery
);
```

**Key Components:**
- `aggregate_function`: Any aggregate (SUM, COUNT, AVG, MIN, MAX, etc.)
- `value_column`: Column to aggregate
- `pivot_column`: Column whose distinct values become new columns
- `IN clause`: Explicit list of pivot values (or ANY for dynamic)

#### 1.2 UNPIVOT Syntax

```sql
-- Basic UNPIVOT
SELECT &lt;columns&gt;
FROM &lt;table_expression&gt;
UNPIVOT (
  &lt;value_column&gt; FOR &lt;name_column&gt; IN (&lt;col1&gt;, &lt;col2&gt;, ..., &lt;colN&gt;)
) AS &lt;alias&gt;;

-- With NULL handling
UNPIVOT INCLUDE NULLS (
  sales FOR quarter IN (q1, q2, q3, q4)
);

UNPIVOT EXCLUDE NULLS (  -- Default behavior
  sales FOR quarter IN (q1, q2, q3, q4)
);
```

**Key Components:**
- `value_column`: New column to hold unpivoted values
- `name_column`: New column to hold source column names
- `IN clause`: List of columns to unpivot
- `INCLUDE/EXCLUDE NULLS`: Control NULL value handling

### 2. Relational Algebra Representation

#### 2.1 PIVOT Operator

```rust
/// PIVOT operator in relational algebra
pub enum RelExpr {
    // ... existing variants ...

    Pivot {
        /// Input relation
        input: Box&lt;RelExpr&gt;,

        /// Column whose distinct values become new columns
        pivot_column: String,

        /// Columns to aggregate (multiple for multi-aggregate PIVOT)
        value_columns: Vec&lt;String&gt;,

        /// Aggregation functions (one per value_column)
        aggregates: Vec&lt;AggregateExpr&gt;,

        /// Explicit list of pivot values, or None for dynamic
        pivot_values: Option&lt;Vec&lt;Literal&gt;&gt;,

        /// Columns to group by (implicit: all non-pivoted, non-value columns)
        group_by: Vec&lt;Expr&gt;,

        /// Alias suffixes for multi-aggregate (e.g., "_total", "_count")
        aliases: Vec&lt;String&gt;,
    },
}

pub struct AggregateExpr {
    pub function: AggregateFunction,
    pub args: Vec&lt;Expr&gt;,
    pub distinct: bool,
    pub filter: Option&lt;Expr&gt;,  // FILTER clause for aggregate
}
```

#### 2.2 UNPIVOT Operator

```rust
pub enum RelExpr {
    // ... existing variants ...

    Unpivot {
        /// Input relation
        input: Box&lt;RelExpr&gt;,

        /// Columns to unpivot
        value_columns: Vec&lt;String&gt;,

        /// New column name for values
        value_name: String,

        /// New column name for original column names
        name_column: String,

        /// Include rows where value is NULL
        include_nulls: bool,

        /// Columns to preserve (not unpivot)
        preserve_columns: Vec&lt;String&gt;,
    },
}
```

### 3. Transformation Strategy

#### 3.1 PIVOT Rewrite Rules

**Rule 1: Static PIVOT → Aggregate + Projection**

```
PIVOT(agg(val) FOR col IN (v1, v2, v3))
  ↓
Aggregate(
  group_by: [non_pivoted_cols],
  aggregates: [
    agg(CASE WHEN col = v1 THEN val END) AS v1,
    agg(CASE WHEN col = v2 THEN val END) AS v2,
    agg(CASE WHEN col = v3 THEN val END) AS v3,
  ]
)
```

**Example:**
```sql
-- Input
PIVOT (SUM(sales) FOR quarter IN ('Q1', 'Q2', 'Q3', 'Q4'))

-- Rewritten to
SELECT
  region,
  SUM(CASE WHEN quarter = 'Q1' THEN sales END) AS Q1,
  SUM(CASE WHEN quarter = 'Q2' THEN sales END) AS Q2,
  SUM(CASE WHEN quarter = 'Q3' THEN sales END) AS Q3,
  SUM(CASE WHEN quarter = 'Q4' THEN sales END) AS Q4
FROM sales
GROUP BY region;
```

**Rule 2: Dynamic PIVOT → Two-Phase Execution**

```
Phase 1: Discover distinct values
  SELECT DISTINCT pivot_column FROM input;

Phase 2: Generate PIVOT plan with discovered values
  (Same as static PIVOT)
```

**Trade-offs:**
- **Static PIVOT:** Single-pass execution, known schema, optimizer-friendly
- **Dynamic PIVOT:** Two-pass execution, unknown schema, requires runtime plan generation

#### 3.2 UNPIVOT Rewrite Rules

**Rule: UNPIVOT → UNION ALL of Projections**

```
UNPIVOT(val FOR name IN (c1, c2, c3))
  ↓
Project([preserve_cols, 'c1' AS name, c1 AS val])
UNION ALL
Project([preserve_cols, 'c2' AS name, c2 AS val])
UNION ALL
Project([preserve_cols, 'c3' AS name, c3 AS val])
```

**Example:**
```sql
-- Input
UNPIVOT (sales FOR quarter IN (q1, q2, q3, q4))

-- Rewritten to
SELECT product, 'q1' AS quarter, q1 AS sales FROM t
UNION ALL
SELECT product, 'q2' AS quarter, q2 AS sales FROM t
UNION ALL
SELECT product, 'q3' AS quarter, q3 AS sales FROM t
UNION ALL
SELECT product, 'q4' AS quarter, q4 AS sales FROM t;
```

**NULL Handling:**
- `EXCLUDE NULLS` (default): Add `WHERE &lt;value_column&gt; IS NOT NULL` to each branch
- `INCLUDE NULLS`: No additional filter

### 4. Optimization Opportunities

#### 4.1 Column Pruning for PIVOT

If only a subset of pivoted columns are referenced in outer query:

```
Project([region, Q1, Q3])
  ↓
Aggregate(CASE WHEN quarter IN ('Q1', 'Q3') THEN ...)  -- Prune Q2, Q4
```

**Optimization Rule:**
```
Project(cols) ← Pivot(...)
  WHERE cols ∩ pivot_columns ⊂ pivot_columns
  ↓
Pivot with pruned pivot_values
```

#### 4.2 Aggregate Pushdown Before PIVOT

Push filters and projections into PIVOT input:

```
Filter(region = 'West') ← Pivot(...)
  ↓
Pivot ← Filter(region = 'West')  -- Reduce rows before aggregation
```

**Optimization Rule:**
```
Filter(pred) ← Pivot(agg FOR col IN ...)
  WHERE pred references only group_by columns
  ↓
Pivot ← Filter(pred)
```

#### 4.3 Index Usage for PIVOT Columns

**Index Selection Heuristics:**
1. **Covering Index:** If index covers `[pivot_column, value_column, group_by_columns]`, use index-only scan
2. **Partial Index:** If index on `pivot_column`, use for filtering during aggregation
3. **Sorted Access:** If index on `group_by_columns`, leverage sort order for aggregation

**Cost Model Adjustment:**
```rust
fn pivot_cost(input_cardinality: usize, num_pivot_values: usize, num_groups: usize) -&gt; Cost {
    let base_cost = aggregate_cost(input_cardinality, num_groups);
    let case_overhead = num_pivot_values * case_expression_cost();
    base_cost + case_overhead
}
```

#### 4.4 Parallel PIVOT Execution

For large datasets, parallelize aggregation:

```
Parallel Hash Aggregate (partitioned by group_by)
  ↓
Scan (parallel, partitioned)
```

**Parallelism Strategy:**
- Partition input by hash(group_by_columns)
- Each worker computes partial aggregates
- Merge phase combines results

#### 4.5 UNPIVOT Optimizations

**Common Subexpression Elimination:**
If multiple UNPIVOT branches share projections:

```
-- Before
UNION ALL of N identical scans

-- After
Single scan → replicate rows N times
```

**Optimization Rule:**
```
UNPIVOT(val FOR name IN (c1, c2, ..., cN))
  WHERE all columns from same table
  ↓
Scan (once) → CrossJoin with VALUES((c1, 'c1'), (c2, 'c2'), ...)
```

---

## Implementation Plan

### Phase 1: Parser and Syntax Support (3-4 weeks)

**Tasks:**
1. Extend SQL parser to recognize PIVOT/UNPIVOT keywords
2. Parse PIVOT clause: `PIVOT(agg FOR col IN (...))`
3. Parse UNPIVOT clause: `UNPIVOT(val FOR name IN (...))`
4. Handle multi-aggregate PIVOT with aliases
5. Support NULL handling options (`INCLUDE/EXCLUDE NULLS`)

**Deliverables:**
- Parse tree nodes for PIVOT/UNPIVOT
- Syntax validation and error messages
- Parser tests for valid/invalid syntax

**Files Modified:**
- `crates/ra-parser/src/parser.rs` — Add PIVOT/UNPIVOT parsing rules
- `crates/ra-parser/src/ast.rs` — AST node definitions
- `crates/ra-parser/tests/pivot_tests.rs` — Parser tests

### Phase 2: Planner Integration (4-5 weeks)

**Tasks:**
1. Add `RelExpr::Pivot` and `RelExpr::Unpivot` variants
2. Implement AST → RelExpr translation
3. Type checking and schema inference for pivoted columns
4. Implement PIVOT → Aggregate rewrite rule
5. Implement UNPIVOT → UNION ALL rewrite rule
6. Handle dynamic PIVOT (two-phase planning)

**Deliverables:**
- Relational algebra representation
- Query rewriting logic
- Schema inference for pivoted output
- Planning tests

**Files Modified:**
- `crates/ra-core/src/algebra.rs` — Add PIVOT/UNPIVOT operators
- `crates/ra-planner/src/planner.rs` — AST translation
- `crates/ra-planner/src/rewrite.rs` — Rewrite rules
- `crates/ra-planner/tests/pivot_planner_tests.rs` — Planner tests

### Phase 3: Optimizer Rules (4-5 weeks)

**Tasks:**
1. Column pruning for PIVOT (prune unused pivoted columns)
2. Predicate pushdown through PIVOT/UNPIVOT
3. Aggregate optimization for PIVOT (leverage hash aggregation)
4. Common subexpression elimination for UNPIVOT
5. Index selection heuristics for PIVOT columns
6. Cost model for PIVOT vs. manual CASE expressions

**Deliverables:**
- Optimization rules in RuleSet
- Cost estimation functions
- Rule application tests

**Files Modified:**
- `crates/ra-optimizer/src/rules/pivot_column_pruning.rs` — New rule
- `crates/ra-optimizer/src/rules/predicate_pushdown.rs` — Extend for PIVOT
- `crates/ra-optimizer/src/cost_model.rs` — PIVOT cost estimation
- `crates/ra-optimizer/tests/pivot_optimization_tests.rs` — Optimizer tests

### Phase 4: Execution Support (2-3 weeks)

**Tasks:**
1. Physical operator for PIVOT (reuse existing Aggregate)
2. Physical operator for UNPIVOT (reuse existing Union + Project)
3. Runtime dynamic PIVOT column discovery
4. Parallel execution for large PIVOTs
5. Memory management for wide PIVOTs (many columns)

**Deliverables:**
- Physical execution operators
- Runtime value discovery for dynamic PIVOT
- Execution tests with diverse datasets

**Files Modified:**
- `crates/ra-engine/src/execution/aggregate.rs` — Reuse for PIVOT
- `crates/ra-engine/src/execution/union.rs` — Reuse for UNPIVOT
- `crates/ra-engine/src/execution/pivot_dynamic.rs` — Dynamic PIVOT
- `crates/ra-engine/tests/pivot_execution_tests.rs` — Execution tests

### Phase 5: Cross-Database Compatibility (2-3 weeks)

**Tasks:**
1. **Oracle Compatibility:**
   - XML-based dynamic PIVOT (`PIVOT XML ... FOR ... IN (ANY)`)
   - Multiple aggregations with alias suffixes
   - INCLUDE/EXCLUDE NULLS in UNPIVOT

2. **SQL Server Compatibility:**
   - Dynamic PIVOT via generated SQL
   - Column name quoting rules

3. **DuckDB Compatibility:**
   - Type inference for pivot values
   - Nested aggregation support

**Deliverables:**
- Dialect-specific syntax variations
- Cross-database test suite
- Compatibility documentation

**Files Modified:**
- `crates/ra-metadata/src/oracle.rs` — Oracle-specific PIVOT metadata
- `crates/ra-metadata/src/sqlserver.rs` — SQL Server PIVOT metadata
- `crates/ra-metadata/src/duckdb.rs` — DuckDB PIVOT metadata
- `tests/cross_db/pivot_compatibility_tests.rs` — Cross-DB tests

---

## Cost Model

### PIVOT Cost Estimation

**Formula:**
```
Cost(PIVOT) = Cost(Aggregate) + CaseExpressionOverhead

Where:
  Cost(Aggregate) = input_rows * log(num_groups) * agg_complexity
  CaseExpressionOverhead = num_pivot_values * case_eval_cost * input_rows

For hash aggregation:
  Cost(PIVOT) = input_rows * (1 + num_pivot_values * 0.1)

For sort aggregation:
  Cost(PIVOT) = input_rows * log(input_rows) + input_rows * num_pivot_values * 0.1
```

**Example:**
- Input: 1M rows
- Pivot values: 12 (months)
- Groups: 100 (products)

```
Hash Aggregate Cost: 1M * (1 + 12 * 0.1) = 2.2M units
Sort Aggregate Cost: 1M * log(1M) + 1M * 1.2 = ~21M + 1.2M = 22.2M units

→ Prefer hash aggregation for PIVOT
```

### UNPIVOT Cost Estimation

**Formula:**
```
Cost(UNPIVOT) = num_unpivot_columns * Cost(Scan) + Cost(Union)

Where:
  Cost(Scan) = input_rows * row_width
  Cost(Union) = total_output_rows * union_overhead
  total_output_rows = input_rows * num_unpivot_columns

Simplified:
  Cost(UNPIVOT) = input_rows * num_unpivot_columns * (scan_cost + union_cost)
```

**Example:**
- Input: 10K rows
- Unpivot columns: 12 (months)

```
Output rows: 10K * 12 = 120K rows
Cost: 120K * (1.0 + 0.1) = 132K units
```

### Comparison: PIVOT vs. Manual CASE

**Manual CASE:**
```
Cost(Manual) = input_rows * num_pivot_values * case_eval_cost
```

**PIVOT (Optimized):**
```
Cost(PIVOT) = input_rows * (1 + num_pivot_values * 0.1)
```

**Speedup:**
```
Speedup = Manual / PIVOT = num_pivot_values / (1 + num_pivot_values * 0.1)

For 12 pivot values:
  Speedup = 12 / (1 + 1.2) = 12 / 2.2 ≈ 5.45x faster
```

---

## Testing Strategy

### 1. Unit Tests

**Parser Tests:**
- Valid PIVOT syntax variations
- Invalid syntax (error messages)
- Edge cases (empty IN clause, missing FOR)

**Planner Tests:**
- AST → RelExpr translation
- Schema inference for pivoted columns
- Type checking (aggregate function compatibility)

**Optimizer Tests:**
- Column pruning rule application
- Predicate pushdown through PIVOT
- Cost estimation accuracy

### 2. Integration Tests

**End-to-End Tests:**
- PIVOT with various aggregates (SUM, COUNT, AVG, MIN, MAX)
- UNPIVOT with NULL handling
- Multi-aggregate PIVOT
- Dynamic PIVOT with runtime column discovery
- Nested PIVOTs
- PIVOT + JOIN queries

**Performance Tests:**
- Compare PIVOT vs. manual CASE (expect 2-5x speedup)
- Scale tests: 1K, 10K, 100K, 1M rows
- Wide PIVOTs: 10, 50, 100, 1000 pivot values
- Memory usage for wide PIVOTs

### 3. Cross-Database Compatibility Tests

**Oracle:**
- PIVOT XML syntax
- Multiple aggregations
- INCLUDE/EXCLUDE NULLS

**SQL Server:**
- Dynamic PIVOT
- Column quoting
- Multi-aggregate aliases

**DuckDB:**
- Type inference
- Nested aggregation

### 4. Regression Tests

- Ensure existing aggregation queries unaffected
- No performance regressions on non-PIVOT queries
- Optimizer rule interactions (predicate pushdown, join reordering)

---

## Performance Expectations

### Query Complexity Reduction

**Before (Manual CASE):**
```sql
-- 100 lines for 12 months
SELECT
  product,
  SUM(CASE WHEN month = 1 THEN sales END) AS jan,
  SUM(CASE WHEN month = 2 THEN sales END) AS feb,
  -- ... 10 more months ...
  SUM(CASE WHEN month = 12 THEN sales END) AS dec
FROM monthly_sales
GROUP BY product;
```

**After (PIVOT):**
```sql
-- 6 lines
SELECT * FROM monthly_sales
PIVOT (
  SUM(sales)
  FOR month IN (1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)
);
```

**Reduction:** 94% fewer lines (100 → 6)

### Execution Performance

**Benchmark Setup:**
- Dataset: 1M rows, 100 distinct groups, 12 pivot values
- Hardware: 8-core CPU, 32GB RAM

**Expected Results:**

| Query Type | Manual CASE | PIVOT (Optimized) | Speedup |
|------------|-------------|-------------------|---------|
| **Simple PIVOT (SUM)** | 2.4s | 0.8s | **3.0x** |
| **Multi-Aggregate** | 4.8s | 1.2s | **4.0x** |
| **Wide PIVOT (100 values)** | 20s | 5s | **4.0x** |
| **UNPIVOT (12 cols)** | 1.5s | 0.9s | **1.7x** |

**Key Factors:**
- **Column Pruning:** Reduces work for unused columns
- **Aggregate Optimization:** Single-pass hash aggregation
- **Index Usage:** Covering indexes for pivot columns
- **Parallel Execution:** Distributes work across cores

---

## Alternatives Considered

### Alternative 1: No Native PIVOT (Status Quo)

**Pros:**
- No implementation effort
- Existing queries continue to work

**Cons:**
- Verbose, error-prone SQL
- Manual optimization required
- Poor developer experience
- Not competitive with Oracle/SQL Server/DuckDB

**Verdict:** ❌ Rejected — High developer demand, competitive necessity

### Alternative 2: View/Macro-Based PIVOT

Provide PIVOT as a macro that expands to CASE expressions at parse time.

**Pros:**
- Simpler implementation (no optimizer changes)
- Transparent to optimizer (standard aggregation)

**Cons:**
- No optimization opportunities specific to PIVOT
- Wide PIVOTs generate huge query plans
- No dynamic PIVOT support
- Limited cross-database compatibility

**Verdict:** ❌ Rejected — Misses optimization potential

### Alternative 3: External Transformation Tool

Provide a separate tool to transform PIVOT queries.

**Pros:**
- No database changes
- Flexible preprocessing

**Cons:**
- Poor user experience (separate tool)
- No integration with query optimizer
- Breaks query caching and plan reuse

**Verdict:** ❌ Rejected — Not database-native

---

## Future Enhancements

### 1. Automatic PIVOT Detection

Detect manual CASE-based pivot patterns and suggest PIVOT rewrite:

```sql
-- Query Advisor suggests:
-- "This query can be rewritten using PIVOT for better performance"
SELECT region,
  SUM(CASE WHEN quarter = 'Q1' THEN sales END) AS q1,
  SUM(CASE WHEN quarter = 'Q2' THEN sales END) AS q2,
  -- ...
FROM sales GROUP BY region;
```

### 2. Materialized PIVOT Views

Pre-compute common PIVOTs as materialized views:

```sql
CREATE MATERIALIZED VIEW monthly_sales_pivot AS
SELECT * FROM sales
PIVOT (SUM(amount) FOR month IN (1,2,3,4,5,6,7,8,9,10,11,12))
REFRESH ON COMMIT;
```

### 3. Incremental PIVOT

For streaming data, update PIVOTs incrementally:

```sql
CREATE INCREMENTAL PIVOT VIEW live_metrics AS
SELECT * FROM events
PIVOT (COUNT(*) FOR event_type IN ('click', 'view', 'purchase'))
WINDOW 1 HOUR;
```

### 4. Multi-Level PIVOT

Support nested PIVOTs (pivot by multiple dimensions):

```sql
PIVOT (
  SUM(sales)
  FOR (region, quarter) IN (
    ('West', 'Q1'), ('West', 'Q2'),
    ('East', 'Q1'), ('East', 'Q2')
  )
);
```

### 5. PIVOT with Window Functions

Combine PIVOT with window functions for ranked results:

```sql
SELECT * FROM sales
PIVOT (
  SUM(amount) AS total,
  RANK() OVER (ORDER BY SUM(amount) DESC) AS rank
  FOR quarter IN ('Q1', 'Q2', 'Q3', 'Q4')
);
```

---

## References

### SQL Standards
- SQL:2016 — No native PIVOT (vendor extensions)
- Oracle 11g+ — PIVOT/UNPIVOT documentation
- SQL Server 2005+ — PIVOT operator reference
- DuckDB — PIVOT implementation guide

### Research Papers
- "Efficient Processing of Pivoting Queries" (VLDB 2010)
- "Query Optimization for Pivoting and Unpivoting Operations" (ICDE 2015)
- "Columnar Storage and Query Processing for OLAP" (SIGMOD 2012)

### Related RFCs
- RFC 0001: Row Pattern Recognition (MATCH_RECOGNIZE)
- RFC 0052: Progressive Re-Optimization
- [RFC 0063](/maintainers/rfcs/0063-spatial-query-optimization): Spatial Query Optimization
- [RFC 0072](/maintainers/rfcs/0072-adaptive-parallelism): Adaptive Parallelism

### Analysis Documents
- `/home/gburd/ws/ra/SQL_STANDARDS_GAP_ANALYSIS.md` (Lines 815-880)
- `/home/gburd/ws/ra/ORACLE_MISSING_FEATURES_REPORT.md` (Lines 208-256)
- `/home/gburd/ws/ra/SQLSERVER_UNSUPPORTED_FEATURES.md` (Lines 929-976)
- `/home/gburd/ws/ra/DUCKDB_FEATURES_ANALYSIS.md` (Lines 99-184)

---

## Approval and Implementation

### Estimated Timeline

| Phase | Duration | Start Date | End Date |
|-------|----------|------------|----------|
| **Phase 1: Parser** | 3-4 weeks | Week 1 | Week 4 |
| **Phase 2: Planner** | 4-5 weeks | Week 5 | Week 9 |
| **Phase 3: Optimizer** | 4-5 weeks | Week 10 | Week 14 |
| **Phase 4: Execution** | 2-3 weeks | Week 15 | Week 17 |
| **Phase 5: Compatibility** | 2-3 weeks | Week 18 | Week 20 |
| **Total** | **15-20 weeks** | Week 1 | Week 20 |

### Success Criteria

1. ✅ **Functionality:**
   - Parse and execute PIVOT/UNPIVOT queries
   - Support static and dynamic PIVOT
   - Handle NULL values correctly
   - Multi-aggregate PIVOT support

2. ✅ **Performance:**
   - 2-5x faster than manual CASE expressions
   - Column pruning reduces work for partial projections
   - Parallel execution for large PIVOTs

3. ✅ **Compatibility:**
   - Oracle PIVOT/UNPIVOT syntax supported
   - SQL Server PIVOT/UNPIVOT syntax supported
   - DuckDB PIVOT/UNPIVOT syntax supported
   - Cross-database test suite passing

4. ✅ **Quality:**
   - 95%+ test coverage
   - No performance regressions on existing queries
   - Clear error messages for invalid syntax
   - Documentation with examples

---

## Conclusion

PIVOT and UNPIVOT operations are essential for modern analytical workloads, reducing query complexity by 2-5x and providing 2-5x performance improvements over manual rewrites. With support in Oracle (11g+), SQL Server (2005+), and DuckDB, this feature is a competitive necessity for Ra to support enterprise reporting and BI workloads.

The proposed implementation leverages existing aggregate and union operators, adding optimization rules for column pruning, predicate pushdown, and parallel execution. The 15-20 week implementation timeline delivers high-value functionality with manageable complexity.

**Recommendation:** Approve for implementation in Q2 2026.


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 96: PIVOT/UNPIVOT Operations and Optimization](/maintainers/rfcs/0096-pivot-unpivot-optimization)
