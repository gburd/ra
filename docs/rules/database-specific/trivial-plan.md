# Rule: mssql Trivial Plan Optimization

**Category:** database-specific/mssql
**File:** `rules/database-specific/mssql/trivial-plan.rra`

## Metadata

- **ID:** `mssql-trivial-plan`
- **Version:** "1.0.0"
- **Databases:** mssql
- **Tags:** database-specific, mssql, trivial, plan, compilation, simple
- **Authors:** "RA Contributors"


# mssql Trivial Plan Optimization

## Description

Bypasses the full cost-based optimizer for simple queries that have
only one reasonable execution plan.  mssql's trivial plan
optimization phase detects queries where the optimal plan is obvious
(e.g., single-table point lookups, simple inserts) and produces the
plan without invoking the expensive search phases of the optimizer.

**When to apply**: A query is simple enough that cost-based
optimization cannot improve the plan (e.g., single-table SELECT with
a unique index seek, INSERT into a heap).

**Why it works**: The full optimizer explores many alternative plans,
which takes CPU time.  For trivial queries, there is only one viable
plan, so optimization effort is wasted.  Trivial plan skips directly
to plan generation, reducing compilation time from milliseconds to
microseconds.

**Database version**: mssql 2000+

## Relational Algebra

```algebra
-- Trivial: single-table point lookup
index-seek(T, pk_index, id = @id)
-- No alternatives to consider; trivial plan

-- Non-trivial: multi-table join with choices
R join[R.a = S.b] S
-- Multiple join orders, algorithms; needs cost-based optimization
```

## Implementation

```rust
use egg::{rewrite as rw, *};

rw!("mssql-trivial-plan-detection";
    "(filter (eq ?pk (param ?p)) (scan ?table))" =>
    "(trivial-plan (index-seek ?table (pk-index ?table) ?pk ?p))"
    if is_database("mssql")
    if is_primary_key("?table", "?pk")
    if single_table_query()
),
```

## Preconditions

```rust
fn applicable(query: &LogicalPlan) -> bool {
    query.table_count() <= 1
    && !query.has_subqueries()
    && !query.has_aggregation()
    && !query.has_distinct()
    && (query.is_point_lookup()
        || query.is_simple_insert()
        || query.is_simple_update())
}
```

**Restrictions:**
- Multi-table queries always go through full optimization
- Queries with subqueries, CTEs, or aggregation are not trivial
- Views may prevent trivial plan if the view definition is complex
- Parameterized queries may still get trivial plans

## Cost Model

```rust
fn compilation_benefit(
    full_optimization_time_us: f64,
    trivial_plan_time_us: f64,
    executions_per_sec: f64,
) -> f64 {
    let saved_per_compile =
        full_optimization_time_us - trivial_plan_time_us;
    saved_per_compile * executions_per_sec * 0.000001
}
```

**Typical benefit**: Reduces compilation from ~1ms to ~10us for
simple OLTP queries, significant at 10K+ compilations/second.

## Test Cases

```sql
-- Positive: simple point lookup
SELECT * FROM users WHERE id = @user_id;
-- Trivial plan: index seek on PK, no optimization needed
```

```sql
-- Positive: simple INSERT
INSERT INTO log_entries (message, ts) VALUES (@msg, GETDATE());
-- Trivial plan: direct heap/clustered insert
```

```sql
-- Negative: multi-table join requires optimization
SELECT u.name, o.amount FROM users u
JOIN orders o ON u.id = o.user_id WHERE o.total > 100;
-- Full optimization: join order, algorithm, index choices
```

## References

mssql: Query Processing Architecture (Trivial Plan phase)
mssql: sys.dm_exec_query_optimizer_info (trivial plan counter)
mssql: SET STATISTICS XML (optimization phases)
