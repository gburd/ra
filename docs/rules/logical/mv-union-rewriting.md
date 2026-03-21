# Rule: Materialized View Union Rewriting

**Category:** logical/view-rewriting
**File:** `rules/logical/view-rewriting/mv-union-rewriting.rra`

## Metadata

- **ID:** `mv-union-rewriting`
- **Version:** "1.0.0"
- **Databases:** oracle, snowflake, clickhouse
- **Tags:** logical, materialized-view, union, partial, rewriting
- **Authors:** "Larson et al., Microsoft Research"


# Materialized View Union Rewriting

## Description

When a materialized view covers part of a query range, rewrites the query
as a UNION of the MV (for the covered range) and a direct table scan (for
the uncovered range). This partial rewriting still benefits from the MV
for most of the data while correctly handling the remainder.

**When to apply**: MV predicate covers a subset of the query predicate range.

## Relational Algebra

```algebra
-- MV: sigma[year >= 2022](sales)
-- Query: sigma[year >= 2020](sales)
-- Rewrite: MV UNION ALL sigma[year >= 2020 AND year < 2022](sales)
```

## Implementation

```rust
fn try_union_rewrite(query: &Query, mv: &MaterializedView) -> Option<Plan> {
    let overlap = query.predicate().intersect(&mv.predicate());
    if overlap.is_empty() { return None; }
    let remainder = query.predicate().subtract(&mv.predicate());
    if remainder.is_empty() { return None; } // Full subsumption, use simpler rule
    Some(Plan::union_all(
        Plan::scan_mv(mv),
        Plan::filter(remainder, Plan::scan_table(&query.table())),
    ))
}
```

## Preconditions

```rust
fn applicable(query_pred: &Predicate, mv_pred: &Predicate) -> bool {
    \!query_pred.intersect(mv_pred).is_empty()
        && \!query_pred.implies(mv_pred) // partial overlap, not full
}
```

## Cost Model

```rust
fn estimated_benefit(mv_coverage_pct: f64, base_scan_cost: f64) -> f64 {
    mv_coverage_pct * 0.9 * base_scan_cost
}
```

## Test Cases

```sql
-- MV: last 2 years
CREATE MATERIALIZED VIEW recent_sales AS
SELECT * FROM sales WHERE sale_date >= '2023-01-01';

-- Positive: query needs 5 years
SELECT * FROM sales WHERE sale_date >= '2020-01-01';
-- Rewrite: recent_sales UNION ALL
--          (SELECT * FROM sales WHERE sale_date >= '2020-01-01' AND sale_date < '2023-01-01')

-- Negative: no overlap
SELECT * FROM sales WHERE sale_date < '2020-01-01';
```

## References

- Larson, P. et al., "Partial Materialized View Rewriting", Microsoft Research Technical Report, 2004
