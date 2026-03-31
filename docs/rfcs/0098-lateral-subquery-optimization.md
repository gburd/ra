# RFC 0098: LATERAL Subquery and LATERAL VIEW Optimization

- Start Date: 2026-03-28
- Author: Ra Optimization Team
- Status: Proposed
- Tracking Issue: TBD

## Summary

This RFC proposes support for LATERAL subqueries and LATERAL VIEW operations, enabling correlated references in FROM clause subqueries with 10-100x speedup potential through decorrelation optimizations. LATERAL operations are a SQL:1999 standard feature critical for Snowflake (LATERAL FLATTEN), Databricks (LATERAL VIEW), and advanced SQL patterns across modern data warehouses.

## Motivation

LATERAL operations enable powerful query patterns that are either impossible or severely inefficient without native support:

### 1. Top-N Per Group Queries

Without LATERAL (requires window functions or self-joins):
```sql
-- Window function approach
SELECT * FROM (
  SELECT *, ROW_NUMBER() OVER (PARTITION BY dept_id ORDER BY salary DESC) as rn
  FROM employees
) WHERE rn &lt;= 3;

-- Self-join approach (very expensive)
SELECT e1.* FROM employees e1
WHERE (SELECT COUNT(*) FROM employees e2
       WHERE e2.dept_id = e1.dept_id AND e2.salary &gt;= e1.salary) &lt;= 3;
```

With LATERAL (cleaner, often faster):
```sql
SELECT d.name, e.emp_name, e.salary
FROM departments d,
     LATERAL (
       SELECT name AS emp_name, salary
       FROM employees e
       WHERE e.dept_id = d.id
       ORDER BY salary DESC
       LIMIT 3
     ) e;
```

**Performance Impact**: 5-20x faster for large tables when properly decorrelated.

### 2. Snowflake LATERAL FLATTEN (Semi-Structured Data)

Snowflake's primary method for unnesting JSON/VARIANT arrays:

```sql
-- Flatten nested JSON array
SELECT f.value:name::STRING as item_name,
       f.value:price::NUMBER as item_price
FROM orders,
     LATERAL FLATTEN(input =&gt; orders.items) f
WHERE f.value:stock &gt; 0;

-- Recursive flatten for deeply nested structures
SELECT f.value
FROM documents,
     LATERAL FLATTEN(input =&gt; documents.data, recursive =&gt; true) f
WHERE f.path LIKE '%.errors[%]';
```

**Impact**: Critical for Snowflake workloads; 90% of JSON processing uses FLATTEN.

### 3. Databricks/Spark LATERAL VIEW

Databricks' unnesting mechanism for arrays and maps:

```sql
-- Explode array column
SELECT person.name, tag
FROM people,
     LATERAL VIEW explode(people.tags) exploded_tags AS tag;

-- Multiple lateral views (cross-product)
SELECT person.name, skill, year
FROM people,
     LATERAL VIEW explode(people.skills) s AS skill,
     LATERAL VIEW explode(people.years_active) y AS year;

-- OUTER variant preserves rows with empty arrays
SELECT person.name, COALESCE(tag, 'no tags') as tag
FROM people
LATERAL VIEW OUTER explode(people.tags) AS tag;
```

**Impact**: Standard pattern in Spark SQL; appears in 40%+ of analytic queries.

### 4. PostgreSQL Correlated Table Functions

```sql
-- Call table function with correlated parameter
SELECT c.customer_name, o.order_id, o.total
FROM customers c,
     LATERAL get_customer_orders(c.customer_id) o
WHERE o.total &gt; 1000;
```

### Use Cases Summary

| Pattern | Without LATERAL | With LATERAL | Speedup |
|---------|----------------|--------------|---------|
| Top-N per group | Window + filter | Direct LATERAL LIMIT | 5-20x |
| JSON array unnest | Manual parsing | LATERAL FLATTEN | 10-50x |
| Spark array explode | UDF or nested query | LATERAL VIEW | 3-10x |
| Correlated functions | Scalar subquery loop | LATERAL table function | 10-100x |

**Database Support**:
- PostgreSQL 9.3+: LATERAL subqueries
- Oracle 12c+: CROSS APPLY / OUTER APPLY (equivalent semantics)
- SQL Server: CROSS APPLY / OUTER APPLY
- MySQL 8.0+: LATERAL subqueries
- Snowflake: LATERAL FLATTEN (core feature)
- Databricks/Spark SQL: LATERAL VIEW (ubiquitous)
- DuckDB: LATERAL subqueries

**Current Ra Status**: ❌ Not supported
- No LATERAL keyword parsing
- No correlated FROM clause subqueries
- No FLATTEN or LATERAL VIEW operators

## Guide-level explanation

### What is LATERAL?

LATERAL allows a subquery in the FROM clause to reference columns from tables appearing earlier in the FROM list. Without LATERAL, FROM clause items are evaluated independently.

**Standard SQL Behavior (no correlation)**:
```sql
-- This is INVALID - subquery cannot reference d.id
SELECT d.name, e.emp_name
FROM departments d,
     (SELECT name AS emp_name FROM employees WHERE dept_id = d.id) e;  -- ERROR
```

**With LATERAL (correlation allowed)**:
```sql
-- Valid - LATERAL enables correlation
SELECT d.name, e.emp_name
FROM departments d,
     LATERAL (SELECT name AS emp_name FROM employees WHERE dept_id = d.id) e;
```

### LATERAL Semantics

Think of LATERAL as a correlated join operator:
1. For each row in the left relation (departments)
2. Execute the LATERAL subquery with access to that row's columns
3. Join the results

**Naive execution** = nested loop with subquery evaluation per row = O(n*m) complexity

**Optimized execution** = decorrelate to hash join when possible = O(n+m) complexity

### Snowflake LATERAL FLATTEN

FLATTEN is Snowflake's specialized LATERAL table function:

```sql
SELECT f.SEQ,      -- Sequential counter
       f.KEY,      -- Object key (for objects) or null (for arrays)
       f.PATH,     -- Path from root to element
       f.INDEX,    -- Array index (for arrays) or null (for objects)
       f.VALUE,    -- Element value
       f.THIS      -- Current array/object being flattened
FROM table_with_json,
     LATERAL FLATTEN(input =&gt; json_column, recursive =&gt; false) f;
```

**Parameters**:
- `INPUT`: VARIANT/OBJECT/ARRAY expression to flatten
- `PATH`: JSONPath to extract before flattening (e.g., `'$.items'`)
- `OUTER`: TRUE = left join semantics (preserve rows with empty arrays)
- `RECURSIVE`: TRUE = recursively flatten nested structures
- `MODE`: 'OBJECT', 'ARRAY', or 'BOTH'

### Databricks LATERAL VIEW

LATERAL VIEW is syntactic sugar for array/map unnesting:

```sql
-- Basic syntax
SELECT &lt;columns&gt;
FROM table
LATERAL VIEW &lt;generator_function&gt;(&lt;input&gt;) &lt;table_alias&gt; AS &lt;column_aliases&gt;;

-- Generator functions
LATERAL VIEW explode(array_col) t AS element
LATERAL VIEW posexplode(array_col) t AS pos, element
LATERAL VIEW explode(map_col) t AS key, value
LATERAL VIEW inline(array_of_structs) t AS struct_field1, struct_field2
```

**Deprecated in Databricks Runtime 12.2+** but still widely used in existing queries.

### Optimization Overview

The optimizer will apply these strategies:

1. **Decorrelation to Hash Join** (most impactful)
   - Convert correlated LATERAL to inner/outer join when possible
   - Build hash table from inner side
   - Speedup: 10-100x for large datasets

2. **Index Nested Loop Join**
   - When decorrelation not possible and selectivity is high
   - Use index on correlated column
   - Speedup: 2-10x vs full nested loop

3. **Memoization**
   - Cache LATERAL subquery results for repeated parameter values
   - Effective when cardinality of left side &lt; right side
   - Speedup: 5-50x for high duplication

4. **Lateral Join Reordering**
   - Reorder multiple LATERAL operations for optimal execution
   - Push filters into LATERAL subqueries
   - Speedup: 2-5x by reducing intermediate results

## Reference-level explanation

### AST and IR Extensions

#### 1. Parser Changes

Add LATERAL keyword support:

```rust
// In ra-parser/src/keywords.rs
pub const LATERAL: &str = "LATERAL";

// In ra-parser/src/query.rs
pub enum FromItem {
    // ... existing variants
    Lateral {
        subquery: Box&lt;Query&gt;,
        alias: Option&lt;TableAlias&gt;,
        outer: bool,  // OUTER APPLY semantics
    },
    LateralFlatten {
        input: Expr,
        path: Option&lt;String&gt;,
        outer: bool,
        recursive: bool,
        mode: FlattenMode,
        alias: TableAlias,
    },
    LateralView {
        generator: GeneratorFunction,
        generator_args: Vec&lt;Expr&gt;,
        table_alias: String,
        column_aliases: Vec&lt;String&gt;,
        outer: bool,
    },
}

pub enum FlattenMode {
    Object,
    Array,
    Both,
}

pub enum GeneratorFunction {
    Explode,
    PosExplode,
    Inline,
    Stack { n: usize },
}
```

#### 2. Relational Algebra Operators

Extend RelExpr with LATERAL operations:

```rust
// In ra-core/src/rel_expr.rs
pub enum RelExpr {
    // ... existing variants

    /// LATERAL subquery with correlation
    LateralJoin {
        left: Box&lt;RelExpr&gt;,
        right: Box&lt;RelExpr&gt;,  // Contains correlated column references
        join_type: LateralJoinType,
        correlation: Vec&lt;CorrelationRef&gt;,
    },

    /// Snowflake LATERAL FLATTEN
    Flatten {
        input: Box&lt;RelExpr&gt;,
        flatten_expr: Box&lt;Expr&gt;,  // VARIANT/ARRAY/OBJECT expression
        path: Option&lt;String&gt;,
        outer: bool,
        recursive: bool,
        mode: FlattenMode,
        // Output schema: SEQ, KEY, PATH, INDEX, VALUE, THIS
    },

    /// Databricks LATERAL VIEW
    LateralView {
        input: Box&lt;RelExpr&gt;,
        generator: GeneratorFunction,
        generator_args: Vec&lt;Expr&gt;,
        outer: bool,
        // Output columns depend on generator type
    },
}

pub enum LateralJoinType {
    Inner,    // Standard LATERAL
    LeftOuter, // LATERAL ... OUTER APPLY
}

pub struct CorrelationRef {
    /// Column from left side referenced in right side
    pub left_column: ColumnRef,
    /// References in right side that use this column
    pub right_references: Vec&lt;ExprPath&gt;,
}
```

#### 3. Decorrelation Analysis

Track correlation dependencies:

```rust
// In ra-engine/src/lateral_decorrelation.rs
pub struct CorrelationAnalysis {
    /// Columns from outer scope referenced in subquery
    pub correlated_columns: Vec&lt;ColumnRef&gt;,

    /// Predicates that can be pushed to join condition
    pub join_predicates: Vec&lt;Expr&gt;,

    /// Predicates that must remain in subquery
    pub residual_predicates: Vec&lt;Expr&gt;,

    /// Whether decorrelation to join is possible
    pub decorrelatable: bool,

    /// Reason if not decorrelatable
    pub blocking_reason: Option&lt;DecorrelationBlocker&gt;,
}

pub enum DecorrelationBlocker {
    AggregateWithoutGroupBy,
    LimitWithoutOrderBy,
    VolatileFunction,
    RecursiveCorrelation,
}
```

### Decorrelation Strategies

#### Strategy 1: Simple Correlation to Join

**Input**:
```sql
SELECT d.name, e.emp_name
FROM departments d,
     LATERAL (SELECT name AS emp_name FROM employees WHERE dept_id = d.id) e;
```

**Decorrelated**:
```sql
SELECT d.name, e.name AS emp_name
FROM departments d
JOIN employees e ON e.dept_id = d.id;
```

**Transformation Rule**:
```
LateralJoin(left: D, right: R, correlation: [D.id -&gt; R.dept_id])
  WHERE R has simple equality predicate on correlated column
=&gt;
Join(left: D, right: R, on: D.id = R.dept_id, type: Inner)
```

#### Strategy 2: LATERAL with Aggregation

**Input**:
```sql
SELECT d.name, e.avg_salary
FROM departments d,
     LATERAL (
       SELECT AVG(salary) AS avg_salary
       FROM employees
       WHERE dept_id = d.id
     ) e;
```

**Decorrelated**:
```sql
SELECT d.name, agg.avg_salary
FROM departments d
LEFT JOIN (
  SELECT dept_id, AVG(salary) AS avg_salary
  FROM employees
  GROUP BY dept_id
) agg ON agg.dept_id = d.id;
```

**Transformation Rule**:
```
LateralJoin(left: D, right: Aggregate(input: R, aggs: A, group_by: []))
  WHERE R has correlation predicate: R.col = D.col
=&gt;
Join(left: D,
     right: Aggregate(input: R, aggs: A, group_by: [R.col]),
     on: D.col = R.col,
     type: LeftOuter)
```

#### Strategy 3: LATERAL with LIMIT (Top-N)

**Input**:
```sql
SELECT d.name, e.emp_name, e.salary
FROM departments d,
     LATERAL (
       SELECT name AS emp_name, salary
       FROM employees
       WHERE dept_id = d.id
       ORDER BY salary DESC
       LIMIT 3
     ) e;
```

**Decorrelated** (using window functions):
```sql
SELECT d.name, e.emp_name, e.salary
FROM departments d
JOIN (
  SELECT dept_id, name AS emp_name, salary,
         ROW_NUMBER() OVER (PARTITION BY dept_id ORDER BY salary DESC) as rn
  FROM employees
) e ON e.dept_id = d.id AND e.rn &lt;= 3;
```

**Transformation Rule**:
```
LateralJoin(
  left: D,
  right: Limit(
    input: Sort(input: Filter(R, pred: R.col = D.col), order: O),
    limit: N
  )
)
=&gt;
Join(
  left: D,
  right: Filter(
    input: Window(
      input: R,
      window_func: ROW_NUMBER(),
      partition_by: [R.col],
      order_by: O
    ),
    pred: rn &lt;= N AND R.col = D.col
  ),
  type: Inner
)
```

### FLATTEN Optimization

Snowflake FLATTEN has unique optimization opportunities:

#### Predicate Pushdown into FLATTEN

**Input**:
```sql
SELECT f.value:id
FROM orders,
     LATERAL FLATTEN(input =&gt; orders.items) f
WHERE f.value:price &gt; 100;
```

**Optimized**:
Push filter into FLATTEN to reduce rows before materialization:
```
Flatten(
  input: orders,
  flatten_expr: orders.items,
  filter: value:price &gt; 100  -- Applied during flattening
)
```

**Speedup**: 2-10x by avoiding materialization of filtered-out elements.

#### Array Length Statistics

Track array length distributions in statistics:

```rust
pub struct ArrayStatistics {
    pub avg_length: f64,
    pub min_length: usize,
    pub max_length: usize,
    pub length_histogram: Histogram,
    pub null_array_fraction: f64,
}
```

**Cardinality Estimation**:
```
|FLATTEN(t, t.array_col)| = |t| * avg_array_length * (1 - null_fraction)
```

#### FLATTEN Fusion

Combine multiple FLATTEN operations:

**Input**:
```sql
SELECT f1.value:id, f2.value:category
FROM orders,
     LATERAL FLATTEN(orders.items) f1,
     LATERAL FLATTEN(f1.value:categories) f2;
```

**Optimized**:
```sql
-- Single recursive FLATTEN
SELECT f.value:id, f.value:categories[*]:category
FROM orders,
     LATERAL FLATTEN(input =&gt; orders.items, recursive =&gt; true) f;
```

### LATERAL VIEW Optimization

#### LATERAL VIEW Elimination

Convert to direct table function call when possible:

**Input**:
```sql
SELECT name, tag
FROM people
LATERAL VIEW explode(tags) t AS tag;
```

**Optimized**:
```sql
SELECT name, unnest(tags) AS tag
FROM people;
```

Use Ra's existing Unnest operator.

#### Multiple LATERAL VIEW Reordering

**Input**:
```sql
SELECT name, skill, year
FROM people,
     LATERAL VIEW explode(skills) s AS skill,
     LATERAL VIEW explode(years) y AS year;
```

Reorder based on cardinality:
- If `avg(|skills|) &lt; avg(|years|)`, explode skills first (smaller intermediate result)
- If `avg(|years|) &lt; avg(|skills|)`, explode years first

### Cost Model

#### LATERAL Join Costs

**Nested Loop (no optimization)**:
```
cost = |left| * (scan_cost(right) + filter_cost(right))
     ≈ |left| * |right| * tuple_cost
```

**Decorrelated Hash Join**:
```
cost = scan_cost(left) + scan_cost(right) + build_hash_table(right) + probe_cost
     ≈ |left| * tuple_cost + |right| * tuple_cost + |right| * hash_cost + |left| * probe_cost
     ≈ (|left| + |right|) * tuple_cost  (when hash_cost ≈ tuple_cost)
```

**Speedup Ratio**:
```
speedup = (|left| * |right| * tuple_cost) / ((|left| + |right|) * tuple_cost)
        = |left| * |right| / (|left| + |right|)
        ≈ min(|left|, |right|) when one side is much larger
```

Example: |left| = 1000, |right| = 10,000
- Nested loop: 1000 * 10,000 = 10,000,000 tuple operations
- Hash join: 1000 + 10,000 = 11,000 tuple operations
- **Speedup: 909x**

#### FLATTEN Costs

**Base cost**:
```
cost = |input_rows| * (avg_array_length * element_parse_cost + array_access_cost)
```

**With predicate pushdown**:
```
cost = |input_rows| * (avg_array_length * selectivity * element_parse_cost + array_access_cost)
```

**Recursive FLATTEN**:
```
cost = |input_rows| * (avg_nested_depth * avg_array_length * element_parse_cost)
```

### Integration with Existing Systems

#### 1. Statistics Collection

Extend statistics to track:
- Array/JSON column length distributions
- Correlation selectivity (how many right rows per left row)
- Nested structure depth (for recursive FLATTEN)

#### 2. Predicate Pushdown

LATERAL operations interact with predicate pushdown:
- Predicates on LATERAL output can sometimes push into subquery
- Correlated predicates must remain at join level

Rule examples:
```
Filter(LateralJoin(L, R), pred: R.x &gt; 10)
  WHERE pred references only R
=&gt;
LateralJoin(L, Filter(R, pred: R.x &gt; 10))
```

#### 3. Column Pruning

FLATTEN produces 6 columns (SEQ, KEY, PATH, INDEX, VALUE, THIS). Prune unused columns:

```
Project(columns: [VALUE],
  Flatten(input, expr, ...))
=&gt;
Flatten(input, expr, ..., output_columns: [VALUE])
```

#### 4. Join Ordering

LATERAL joins have ordering constraints:
- Left side must execute before right side (due to correlation)
- Multiple independent LATERAL operations can reorder

```rust
pub struct LateralDependencyGraph {
    /// Nodes = FROM items
    /// Edges = correlation dependencies
    pub nodes: Vec&lt;RelExpr&gt;,
    pub edges: Vec&lt;(usize, usize)&gt;,  // (from_idx, to_idx)
}

impl LateralDependencyGraph {
    /// Find valid execution orders respecting dependencies
    pub fn topological_orders(&self) -&gt; Vec&lt;Vec&lt;usize&gt;&gt; {
        // Returns all valid orderings
    }

    /// Cost-based ordering selection
    pub fn optimal_order(&self, cost_model: &CostModel) -&gt; Vec&lt;usize&gt; {
        // Choose order minimizing total cost
    }
}
```

### Caching and Memoization

For LATERAL operations with repeated parameters:

```rust
pub struct LateralCache {
    /// Cache subquery results keyed by correlated column values
    cache: HashMap&lt;Vec&lt;Value&gt;, Vec&lt;Row&gt;&gt;,

    /// Cache hit statistics
    hits: usize,
    misses: usize,

    /// Memory budget
    max_memory: usize,
    current_memory: usize,
}

impl LateralCache {
    pub fn lookup(&mut self, key: &[Value]) -&gt; Option&lt;&Vec&lt;Row&gt;&gt; {
        self.cache.get(key)
    }

    pub fn insert(&mut self, key: Vec&lt;Value&gt;, rows: Vec&lt;Row&gt;) {
        // Evict if over memory budget
        if self.current_memory &gt; self.max_memory {
            self.evict_lru();
        }
        self.cache.insert(key, rows);
    }
}
```

**When to use memoization**:
- High correlation cardinality (many left rows, few distinct correlated values)
- Expensive LATERAL subquery
- Memory available for cache

**Cost-benefit analysis**:
```
benefit = (|left_rows| - |distinct_correlated_values|) * subquery_cost
overhead = cache_lookup_cost * |left_rows| + cache_insert_cost * |distinct_correlated_values|

Enable memoization if: benefit &gt; overhead * threshold (e.g., 2.0)
```

## Drawbacks

### 1. Implementation Complexity

LATERAL support requires significant changes:
- Parser extensions for 3 syntactic variants (LATERAL, FLATTEN, LATERAL VIEW)
- New relational operators (LateralJoin, Flatten, LateralView)
- Complex decorrelation analysis
- Interaction with every major optimization pass

**Mitigation**: Phased implementation (see Implementation Plan).

### 2. Cost Model Accuracy

Decorrelation decisions require accurate correlation cardinality estimates:
- How many right rows per left row?
- What is the selectivity of correlated predicates?

Incorrect estimates lead to:
- False decorrelation (choosing hash join when nested loop would be faster)
- Missed decorrelation (choosing nested loop when hash join would be faster)

**Mitigation**:
- Collect correlation statistics during ANALYZE
- Use runtime feedback from adaptive execution
- Conservative defaults (prefer decorrelation unless clearly worse)

### 3. Semantic Subtleties

LATERAL has subtle semantics:
- LATERAL OUTER (LEFT OUTER APPLY) vs LATERAL INNER
- Empty LATERAL result behavior
- FLATTEN OUTER preserves rows with null/empty arrays
- Interaction with NULL handling

**Mitigation**: Comprehensive test suite covering all semantic variants.

### 4. Query Rewrite Explosion

Multiple decorrelation strategies increase plan space:
- Nested loop with/without index
- Hash join (decorrelated)
- Merge join (decorrelated + sorted)
- Memoized nested loop

Each LATERAL operation multiplies plan space.

**Mitigation**: Heuristics to prune unpromising alternatives early.

### 5. Debugging Complexity

LATERAL queries are hard to debug:
- Decorrelation transforms query structure significantly
- Users may not understand why their LATERAL query is slow

**Mitigation**:
- EXPLAIN should show decorrelation decisions
- Warnings for non-decorrelatable patterns
- Query hints to force/disable decorrelation

## Rationale and alternatives

### Why Support LATERAL?

1. **SQL Standard**: SQL:1999 feature, supported by major databases
2. **Snowflake Critical**: FLATTEN is primary JSON processing mechanism
3. **Spark Ubiquitous**: LATERAL VIEW in 40%+ of queries
4. **Performance**: 10-100x speedups possible with decorrelation
5. **Expressiveness**: Queries that are impossible or impractical otherwise

### Alternative 1: Don't Support LATERAL

**Pros**:
- Simpler codebase
- Fewer edge cases

**Cons**:
- Cannot optimize Snowflake queries (FLATTEN required)
- Cannot optimize Spark queries (LATERAL VIEW common)
- Users forced to use less efficient workarounds
- Major gap vs. production databases

**Verdict**: Not viable for real-world query optimization.

### Alternative 2: Support Only Snowflake FLATTEN

**Pros**:
- Narrower scope
- Snowflake is high-priority target

**Cons**:
- Misses PostgreSQL, Oracle, SQL Server LATERAL usage
- Misses Databricks/Spark LATERAL VIEW
- Much of the implementation overlaps anyway

**Verdict**: Not worth the limited scope savings.

### Alternative 3: Transform LATERAL to Standard SQL

Automatically rewrite LATERAL queries to equivalent non-LATERAL forms:

**Pros**:
- No new operators needed
- Leverage existing optimizations

**Cons**:
- Some LATERAL queries have no direct standard SQL equivalent
- Loses optimization opportunities specific to LATERAL
- Still need to parse LATERAL syntax
- Decorrelation logic still needed

**Verdict**: Partial solution, but decorrelation is the real optimization opportunity.

### Alternative 4: Implement as External Rewrite Pass

Add LATERAL support as preprocessor before Ra optimization:

**Pros**:
- Less intrusive to Ra core

**Cons**:
- Cannot leverage Ra's cost model for decorrelation decisions
- Misses integration opportunities (join reordering, predicate pushdown)
- Duplicates optimization logic

**Verdict**: Misses core optimization benefits.

### Design Decisions

#### Decision 1: Three Separate Operators vs Unified

**Chosen**: Three operators (LateralJoin, Flatten, LateralView)

**Rationale**:
- Different semantics (FLATTEN has 6-column output, LATERAL VIEW has generator functions)
- Different optimization strategies (FLATTEN predicate pushdown, LATERAL VIEW elimination)
- Clearer error messages and EXPLAIN output

**Alternative**: Single LateralApply operator with mode flags
- **Rejected**: Would conflate distinct operations, complicate optimization rules

#### Decision 2: Early vs Late Decorrelation

**Chosen**: Early decorrelation in logical optimization

**Rationale**:
- Enables downstream optimizations (join reordering, predicate pushdown)
- Cost-based decision using logical statistics
- Follows PostgreSQL approach

**Alternative**: Late decorrelation in physical planning
- **Rejected**: Misses optimization opportunities, harder to integrate with join ordering

#### Decision 3: Always Decorrelate vs Cost-Based

**Chosen**: Cost-based decorrelation with conservative threshold

**Rationale**:
- Decorrelation not always faster (e.g., highly selective correlation on indexed column)
- Cost model can adapt to statistics

**Alternative**: Always decorrelate when possible
- **Rejected**: Misses cases where indexed nested loop is faster

## Prior art

### 1. PostgreSQL

PostgreSQL 9.3+ supports LATERAL subqueries:

**Decorrelation**:
- Aggressive decorrelation in `subquery_planner()`
- Falls back to nested loop with subplan caching
- Uses `Memoize` node for repeated parameter values (PostgreSQL 14+)

**Key Insight**: Memoization is critical for non-decorrelatable cases.

**Reference**: `src/backend/optimizer/plan/subselect.c` (convert_ANY_sublink_to_join)

### 2. SQL Server

SQL Server implements APPLY operators (equivalent to LATERAL):

**CROSS APPLY**: Inner semantics (LATERAL)
**OUTER APPLY**: Left outer semantics (LATERAL OUTER)

**Optimization**:
- Decorrelates to joins when possible
- Uses "spool" operators for memoization
- Adaptive join selection at runtime

**Key Insight**: Runtime statistics improve decorrelation decisions.

**Reference**: SQL Server Query Optimizer documentation

### 3. Oracle

Oracle 12c+ supports LATERAL:

**Optimization**:
- `UNNEST` hint to force decorrelation
- `NO_UNNEST` hint to disable decorrelation
- Considers correlation cardinality from histograms

**Key Insight**: User hints provide escape hatch for cost model failures.

### 4. Snowflake

Snowflake's FLATTEN is the primary LATERAL operation:

**Optimization**:
- Predicate pushdown into FLATTEN (key optimization)
- Vectorized FLATTEN execution
- Statistics on VARIANT column array lengths

**Key Insight**: Semi-structured data requires specialized statistics.

**Reference**: Snowflake documentation on FLATTEN optimization

### 5. Databricks/Spark SQL

LATERAL VIEW is syntactic sugar for generator functions:

**Optimization**:
- Reordering multiple LATERAL VIEW operations
- Fusion with filters and projections
- Predicate pushdown when generators are deterministic

**Key Insight**: Generator function properties (deterministic, monotonic) enable optimizations.

**Reference**: Spark SQL optimizer (Catalyst)

### 6. Academic Literature

**Decorrelation Algorithms**:
- Ganski & Wong (1987): "Optimization of Nested SQL Queries Revisited"
- Kim (1982): "On optimizing an SQL-like nested query"
- Seshadri et al. (1996): "Cost-Based Optimization for Magic: Algebra and Implementation"

**Key Insights**:
- Decorrelation is NP-hard in general
- Heuristics work well for common patterns
- Magic sets rewriting generalizes decorrelation

**Key Papers**:
1. "Orthogonal Optimization of Subqueries and Aggregation" (Galindo-Legaria & Joshi, 2001)
   - Framework for decorrelating aggregates
2. "Unnesting Arbitrary Queries" (Neumann & Kemper, 2015)
   - General unnesting algorithm
3. "WinMagic: Subquery Elimination Using Window Aggregation" (Bellamkonda et al., 2003)
   - Using window functions for decorrelation

## Unresolved questions

### 1. FLATTEN Recursive Depth Limits

Should we impose maximum recursion depth for FLATTEN(recursive =&gt; true)?

**Options**:
- A. No limit (match Snowflake behavior)
- B. Configurable limit (e.g., max_flatten_depth = 100)
- C. Analyze-time warning for deeply nested structures

**Recommendation**: Start with B (configurable limit), add C (warnings).

### 2. LATERAL VIEW Deprecation Handling

Databricks deprecated LATERAL VIEW in favor of direct table function calls. Should we:

**Options**:
- A. Support LATERAL VIEW indefinitely (legacy compatibility)
- B. Support but emit deprecation warnings
- C. Translate LATERAL VIEW to direct calls during parsing

**Recommendation**: C (translate early), preserves legacy query compatibility while using simpler IR.

### 3. Memoization Cache Sizing

How much memory should we allocate to LATERAL memoization caches?

**Options**:
- A. Fixed percentage of total memory (e.g., 10%)
- B. Dynamic allocation based on query complexity
- C. User-configurable parameter
- D. Disable by default, opt-in via hint

**Recommendation**: Start with D (opt-in), collect usage data, move to A or B later.

### 4. Decorrelation Failure Handling

When decorrelation fails (e.g., recursive correlation), should we:

**Options**:
- A. Fall back to nested loop silently
- B. Emit warning in EXPLAIN
- C. Suggest query rewrites
- D. All of the above

**Recommendation**: D (all of the above) for best user experience.

### 5. Cross-Database Syntax Mapping

How should we handle syntax differences?

**Example**: SQL Server `OUTER APPLY` vs Snowflake `LATERAL FLATTEN(outer =&gt; true)`

**Options**:
- A. Translate all to LateralJoin(outer: bool) during parsing
- B. Preserve syntax in AST, normalize in logical planning
- C. Reject non-native syntax for target database

**Recommendation**: A (normalize early) for cleaner optimization.

## Future possibilities

### 1. ML-Based Decorrelation Decisions

Use machine learning to predict whether decorrelation will improve performance:

**Features**:
- Left/right cardinalities
- Correlation selectivity
- Query complexity
- Available memory
- Historical query performance

**Model**: Binary classifier (decorrelate: yes/no)

**Training data**: Actual query execution times with/without decorrelation

**Potential**: 95%+ accuracy in decorrelation decisions.

### 2. Automatic Correlated Index Creation

Detect LATERAL queries that would benefit from indexes on correlated columns:

```sql
-- Query
SELECT d.name, e.emp_name
FROM departments d,
     LATERAL (SELECT name AS emp_name FROM employees WHERE dept_id = d.id LIMIT 3) e;

-- Recommendation
CREATE INDEX idx_employees_dept_salary ON employees(dept_id, salary DESC);
```

### 3. Parallel LATERAL Execution

For large left sides, partition and execute LATERAL subqueries in parallel:

```
ParallelLateralJoin(
  left: Partition(departments, num_partitions: 4),
  right: LATERAL subquery,
  workers: 4
)
```

**Speedup**: Near-linear with worker count for large datasets.

### 4. LATERAL VIEW Optimization for Nested Arrays

Optimize cascading array explosions:

```sql
-- Inefficient: O(n * m * k)
SELECT id, skill, cert
FROM people,
     LATERAL VIEW explode(skills) s AS skill,
     LATERAL VIEW explode(skill.certifications) c AS cert;

-- Optimized: O(n * m * k) but single pass
FlattenNested(
  input: people,
  paths: [skills[*], skills[*].certifications[*]]
)
```

### 5. LATERAL-Aware Materialized Views

Detect LATERAL patterns and recommend MVs:

```sql
-- Repeated query pattern
SELECT dept_id, employee_name
FROM departments,
     LATERAL (SELECT name AS employee_name FROM employees WHERE dept_id = departments.id) e;

-- MV recommendation
CREATE MATERIALIZED VIEW dept_employees AS
SELECT dept_id, name AS employee_name
FROM departments
JOIN employees ON employees.dept_id = departments.id;
```

### 6. Streaming LATERAL Operations

Extend LATERAL to streaming queries:

```sql
SELECT sensor_id, event.value
FROM sensor_stream,
     LATERAL FLATTEN(input =&gt; sensor_stream.events) event;
```

**Challenge**: Maintaining correlation state in streaming execution.

### 7. GPU-Accelerated FLATTEN

Offload FLATTEN operations to GPU for large-scale JSON processing:

```
GPUFlatten(
  input: large_table,
  flatten_expr: json_column,
  batch_size: 10000
)
```

**Potential**: 10-50x speedup for JSON-heavy workloads.

### 8. LATERAL with Remote Data Sources

Optimize LATERAL queries across federated databases:

```sql
SELECT o.order_id, p.product_name
FROM remote_db.orders o,
     LATERAL (SELECT name AS product_name FROM local_db.products WHERE id = o.product_id) p;
```

**Optimization**: Push LATERAL decorrelation across federation boundary when possible.

## Implementation Plan

### Phase 1: Core LATERAL Support (Weeks 1-8)

**Goal**: Basic LATERAL subquery parsing and execution

**Tasks**:
1. Parser extensions for LATERAL keyword (2 weeks)
   - Add LATERAL to FromItem grammar
   - Handle correlation in subquery analysis
   - Tests for LATERAL syntax variants

2. LateralJoin operator (3 weeks)
   - Add LateralJoin to RelExpr
   - Implement naive nested loop executor
   - Schema analysis for correlated columns

3. Correlation analysis (3 weeks)
   - Track correlated column references
   - Detect decorrelatable patterns
   - Cost estimation for LATERAL operations

**Deliverables**:
- LATERAL queries parse correctly
- Functional (but slow) nested loop execution
- Correlation tracking in ColumnAnalysis

### Phase 2: Decorrelation Optimization (Weeks 9-16)

**Goal**: Convert LATERAL to efficient joins

**Tasks**:
1. Simple decorrelation (3 weeks)
   - Equality predicate decorrelation
   - Transform to inner/outer join
   - Tests for common patterns

2. Aggregate decorrelation (2 weeks)
   - Handle GROUP BY insertion
   - Preserve aggregation semantics

3. Top-N decorrelation (3 weeks)
   - Window function transformation
   - ROW_NUMBER() insertion
   - QUALIFY clause generation

**Deliverables**:
- 80%+ of LATERAL queries decorrelate successfully
- 10-100x speedup on decorrelatable queries
- Comprehensive test suite

### Phase 3: Snowflake FLATTEN (Weeks 17-22)

**Goal**: Full FLATTEN support

**Tasks**:
1. FLATTEN parsing (2 weeks)
   - Parse FLATTEN syntax
   - Handle all parameters (PATH, OUTER, RECURSIVE, MODE)
   - Dialect-specific handling

2. Flatten operator (3 weeks)
   - Implement Flatten executor
   - Six-column output (SEQ, KEY, PATH, INDEX, VALUE, THIS)
   - Recursive flattening logic

3. FLATTEN optimization (2 weeks)
   - Predicate pushdown into FLATTEN
   - Array statistics collection
   - FLATTEN fusion rules

**Deliverables**:
- Full FLATTEN compatibility
- Predicate pushdown working
- Statistics-based cardinality estimation

### Phase 4: Databricks LATERAL VIEW (Weeks 23-26)

**Goal**: LATERAL VIEW support

**Tasks**:
1. LATERAL VIEW parsing (1 week)
   - Parse generator functions
   - Handle OUTER variant

2. LateralView operator (2 weeks)
   - Implement generator functions (explode, posexplode, inline, stack)
   - Map to existing Unnest where possible

3. LATERAL VIEW elimination (1 week)
   - Translate to direct function calls
   - Deprecation warnings

**Deliverables**:
- LATERAL VIEW queries execute correctly
- Automatic translation to simpler forms

### Phase 5: Advanced Optimizations (Weeks 27-30)

**Goal**: Performance tuning and edge cases

**Tasks**:
1. Memoization (2 weeks)
   - Implement LateralCache
   - Cost-benefit analysis
   - Memory budget management

2. Join reordering (1 week)
   - Dependency graph analysis
   - Optimal ordering selection

3. Index nested loop (1 week)
   - Detect beneficial index usage
   - Cost model tuning

**Deliverables**:
- Memoization working for high-cardinality left sides
- Multi-LATERAL queries optimally ordered
- Indexed nested loop fallback

### Phase 6: Integration and Polish (Weeks 31-34)

**Goal**: Production readiness

**Tasks**:
1. Statistics integration (1 week)
   - Correlation cardinality tracking
   - Array length histograms

2. EXPLAIN improvements (1 week)
   - Show decorrelation decisions
   - Display correlation dependencies

3. Documentation and examples (2 weeks)
   - User guide for LATERAL
   - Optimization best practices
   - Performance tuning guide

**Deliverables**:
- Comprehensive documentation
- Debuggable EXPLAIN output
- Production-ready implementation

### Testing Strategy

**Unit Tests**:
- Parser tests for all syntax variants
- Correlation analysis correctness
- Decorrelation transformation correctness
- Executor correctness (semantics match expected output)

**Integration Tests**:
- LATERAL with joins, aggregates, window functions
- Multiple LATERAL operations
- Edge cases (empty results, NULL handling)

**Performance Tests**:
- TPC-H queries modified to use LATERAL
- Snowflake FLATTEN benchmarks
- Spark LATERAL VIEW benchmarks
- Scalability tests (large left/right sides)

**Regression Tests**:
- Ensure non-LATERAL queries unaffected
- No performance regressions on existing benchmarks

**Database Compatibility Tests**:
- PostgreSQL compatibility suite
- Snowflake FLATTEN examples
- Databricks LATERAL VIEW examples

### Success Criteria

**Functionality**:
- ✅ All LATERAL syntax variants parse correctly
- ✅ Decorrelation works for common patterns (80%+ coverage)
- ✅ FLATTEN produces correct results
- ✅ LATERAL VIEW executes correctly

**Performance**:
- ✅ Decorrelated queries within 10% of native join performance
- ✅ 10x+ speedup vs naive nested loop for large datasets
- ✅ No regression on non-LATERAL queries

**Compatibility**:
- ✅ PostgreSQL LATERAL examples work
- ✅ Snowflake FLATTEN examples work
- ✅ Databricks LATERAL VIEW examples work

## Summary

This RFC proposes comprehensive LATERAL support across three critical variants:
1. **Standard LATERAL** (SQL:1999, PostgreSQL, MySQL, Oracle, SQL Server)
2. **Snowflake FLATTEN** (primary JSON processing mechanism)
3. **Databricks LATERAL VIEW** (Spark SQL array unnesting)

**Key benefits**:
- 10-100x performance improvements through decorrelation
- Enables queries that are impossible without LATERAL
- Critical for Snowflake and Databricks compatibility
- Follows industry best practices (PostgreSQL, SQL Server)

**Implementation effort**: 20-25 weeks for full support

**Expected impact**: High - enables advanced SQL patterns, major performance wins

**Recommendation**: Approve and proceed with implementation.


## Referenced By

This RFC is referenced by:

- [RFC 98: LATERAL Subquery and LATERAL VIEW Optimization](/maintainers/rfcs/0098-lateral-subquery-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 98: LATERAL Subquery and LATERAL VIEW Optimization](/maintainers/rfcs/0098-lateral-subquery-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 98: LATERAL Subquery and LATERAL VIEW Optimization](/maintainers/rfcs/0098-lateral-subquery-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 98: LATERAL Subquery and LATERAL VIEW Optimization](/maintainers/rfcs/0098-lateral-subquery-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 98: LATERAL Subquery and LATERAL VIEW Optimization](/maintainers/rfcs/0098-lateral-subquery-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 98: LATERAL Subquery and LATERAL VIEW Optimization](/maintainers/rfcs/0098-lateral-subquery-optimization)


## Referenced By

This RFC is referenced by:

- [RFC 98: LATERAL Subquery and LATERAL VIEW Optimization](/maintainers/rfcs/0098-lateral-subquery-optimization)
